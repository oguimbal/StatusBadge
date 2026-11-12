use crossbeam_channel::{Receiver, Sender};
use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password, PasswordOptions,
};
use serde::{Deserialize, Serialize};
use statusbadge_core::{Summary, Target, TargetState};
use std::{
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEYCHAIN_SERVICE: &str = "fr.gmbl.statusbadge.session";
const POLL: Duration = Duration::from_secs(3);

/// Keychain item content. Deliberately no Debug.
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    /// Seconds since the Unix epoch, as declared by the sudo.
    pub expires: Option<u64>,
}
impl Session {
    fn expired(&self) -> bool {
        self.expires.is_some_and(|exp| exp <= now_ms() / 1000)
    }
}

pub enum Command {
    /// Token received from a validated callback, with the metadata the sudo declared.
    Connect(Target, Session),
    /// Target removed by the user: forget its Keychain item and stop.
    Remove,
}
pub enum Event {
    State(String, TargetState),
    /// First summary accepted for a new token: the target can be saved.
    Connected(Target),
    /// New token refused or summary unreachable: nothing was saved.
    Rejected(String, &'static str),
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
/// `fresh` is set for a connection in progress; otherwise the session is read from the Keychain.
pub fn spawn(target: Target, fresh: Option<Session>, events: Sender<Event>) -> Sender<Command> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(target, fresh, rx, events));
    tx
}

enum PollResult {
    Summary(Summary),
    Unauthorized,
    Unavailable,
}
fn load_summary(agent: &ureq::Agent, endpoint: &str, token: &str) -> PollResult {
    let response = match agent
        .get(endpoint)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(401 | 403, _)) => return PollResult::Unauthorized,
        _ => return PollResult::Unavailable,
    };
    let mut bytes = Vec::new();
    if response
        .into_reader()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 64 * 1024
    {
        return PollResult::Unavailable;
    }
    match serde_json::from_slice::<Summary>(&bytes) {
        Ok(summary) if summary.validate() => PollResult::Summary(summary),
        _ => PollResult::Unavailable,
    }
}

fn read_session(id: &str) -> Result<Option<Session>, ()> {
    match generic_password(PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, id)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|_| ()),
        Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
        Err(_) => Err(()),
    }
}
fn save_session(id: &str, session: &Session) -> bool {
    serde_json::to_vec(session)
        .is_ok_and(|bytes| set_generic_password(KEYCHAIN_SERVICE, id, &bytes).is_ok())
}
pub fn remove_session(id: &str) -> bool {
    match delete_generic_password(KEYCHAIN_SERVICE, id) {
        Ok(()) => true,
        Err(error) => error.code() == -25300,
    }
}

fn connected(summary: Summary, unsaved: bool) -> TargetState {
    TargetState {
        authenticated: true,
        summary: Some(summary),
        offline: false,
        message: unsaved
            .then(|| "Session active, enregistrement dans le Trousseau impossible.".into()),
    }
}

fn run(
    mut target: Target,
    fresh: Option<Session>,
    commands: Receiver<Command>,
    events: Sender<Event>,
) {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build();
    // A pending session is verified once before it replaces anything in the Keychain.
    let mut pending = fresh;
    let mut session = None;
    let mut state = TargetState::default();
    let mut unsaved = false;
    if pending.is_none() {
        match read_session(&target.id) {
            Ok(Some(saved)) => session = Some(saved),
            Ok(None) => state = TargetState::locked("Connexion requise"),
            Err(()) => {
                state = TargetState::locked("Trousseau inaccessible. Réessayer la connexion.")
            }
        }
    } else {
        state = TargetState::locked("Vérification de la session…");
    }
    loop {
        if let Some(candidate) = pending.take() {
            let result = if candidate.expired() {
                PollResult::Unauthorized
            } else {
                load_summary(&agent, &target.summary, &candidate.token)
            };
            match result {
                PollResult::Summary(summary) => {
                    unsaved = !save_session(&target.id, &candidate);
                    session = Some(candidate);
                    state = connected(summary, unsaved);
                    let _ = events.send(Event::Connected(target.clone()));
                }
                PollResult::Unauthorized => {
                    let _ = events.send(Event::Rejected(
                        target.id.clone(),
                        "Jeton refusé par le résumé. Connexion non enregistrée.",
                    ));
                }
                PollResult::Unavailable => {
                    let _ = events.send(Event::Rejected(
                        target.id.clone(),
                        "Résumé injoignable ou invalide. Connexion non enregistrée.",
                    ));
                }
            }
        } else if let Some(current) = &session {
            let result = if current.expired() {
                PollResult::Unauthorized
            } else {
                load_summary(&agent, &target.summary, &current.token)
            };
            match result {
                PollResult::Summary(summary) => state = connected(summary, unsaved),
                PollResult::Unauthorized => {
                    session = None;
                    state = TargetState::locked(if remove_session(&target.id) {
                        "Session expirée ou refusée. Se reconnecter."
                    } else {
                        "Session refusée. Trousseau inaccessible."
                    });
                }
                PollResult::Unavailable => {
                    state.offline = true;
                    state.message =
                        Some("Résumé injoignable. Nouvelle tentative automatique.".into());
                }
            }
        }
        if events
            .send(Event::State(target.id.clone(), state.clone()))
            .is_err()
        {
            return;
        }
        match commands.recv_timeout(POLL) {
            Ok(Command::Connect(updated, candidate)) => {
                target = updated;
                pending = Some(candidate);
            }
            Ok(Command::Remove) => {
                remove_session(&target.id);
                return;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};
    fn request(status: u16, body: &str) -> (PollResult, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/summary", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let size = socket.read(&mut request).unwrap();
            socket.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request[..size]).into_owned()
        });
        let agent = ureq::AgentBuilder::new().redirects(0).build();
        let result = load_summary(&agent, &url, "test-token");
        (result, handle.join().unwrap())
    }
    #[test]
    fn loads_the_server_global_status_and_arbitrary_categories() {
        let body = r#"{"updatedAt":100000,"status":"degraded","count":7,"sections":[{"id":"new-category","label":"Nouvelle catégorie","status":"outage","count":100}],"extra":true}"#;
        let (PollResult::Summary(summary), sent) = request(200, body) else {
            panic!("valid server summary rejected");
        };
        assert_eq!(summary.status, statusbadge_core::Status::Degraded);
        assert_eq!(summary.count, 7);
        assert!(sent.contains("Bearer test-token"));
        assert!(!sent.to_ascii_lowercase().contains("origin:"));
        assert!(matches!(
            request(200, &body.replace("\"status\":\"degraded\",", "")).0,
            PollResult::Unavailable
        ));
    }
    #[test]
    fn only_authentication_failures_lock_the_target() {
        assert!(matches!(request(401, "{}").0, PollResult::Unauthorized));
        assert!(matches!(request(403, "{}").0, PollResult::Unauthorized));
        for status in [429, 500, 503, 302] {
            assert!(matches!(request(status, "{}").0, PollResult::Unavailable));
        }
        assert!(matches!(
            request(200, "<html>proxy error</html>").0,
            PollResult::Unavailable
        ));
    }
    #[test]
    fn declared_expiry_locks_without_decoding_the_token() {
        let past = Session {
            token: "opaque".into(),
            expires: Some(1),
        };
        let open = Session {
            token: "opaque".into(),
            expires: None,
        };
        assert!(past.expired());
        assert!(!open.expired());
    }
}
