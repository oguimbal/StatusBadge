//! Statusbadge protocol v1, badge side. See docs/protocol.md.
use crate::TargetEnvironment;
use url::Url;

pub const LOGIN_TTL_MS: u64 = 10 * 60 * 1000;
const CALLBACK_KEYS: [&str; 8] = [
    "v",
    "state",
    "token",
    "name",
    "environment",
    "summary",
    "open",
    "expires",
];

/// HTTPS, or HTTP on the loopback interface only (local development servers).
fn allowed(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && match url.scheme() {
            "https" => url.host_str().is_some(),
            "http" => {
                matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                    || url
                        .host_str()
                        .is_some_and(|host| host.ends_with(".localhost"))
            }
            _ => false,
        }
}

/// Normalizes the URL typed by the user to its origin. Users paste whatever page of the
/// sudo they have open (`…/dashboard`); the protocol route always sits at the root.
pub fn normalize_base(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 2048 {
        return None;
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&with_scheme).ok()?;
    if !allowed(&url) {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

/// Validates a URL received from a sudo (summary, open).
pub fn parse_endpoint(raw: &str) -> Option<String> {
    if raw.len() > 2048 {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (allowed(&url) && url.fragment().is_none()).then(|| url.into())
}

// Deliberately no Debug: neither callback nor bearer belongs in logs.
pub struct PendingLogin {
    pub base: String,
    /// Target being reconnected, if any.
    pub target: Option<String>,
    pub state: String,
    pub created_at: u64,
}
pub struct Callback {
    pub state: String,
    pub token: String,
    pub name: String,
    pub environment: TargetEnvironment,
    pub summary: String,
    pub open: String,
    /// Seconds since the Unix epoch.
    pub expires: Option<u64>,
}
impl PendingLogin {
    pub fn login_url(&self) -> String {
        let mut url = Url::parse(&format!("{}/statusbadge", self.base)).expect("normalized base");
        url.query_pairs_mut().append_pair("state", &self.state);
        url.into()
    }
    pub fn accepts(&self, callback: &Callback, now_ms: u64) -> bool {
        callback.state == self.state
            && now_ms >= self.created_at
            && now_ms - self.created_at <= LOGIN_TTL_MS
    }
}

pub fn parse_callback(raw: &str) -> Option<Callback> {
    if raw.len() > 24_576 {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "statusbadge"
        || url.host_str() != Some("connect")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
    {
        return None;
    }
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(url.fragment()?.as_bytes())
        .into_owned()
        .collect();
    // Exactly the protocol keys, each at most once.
    for (index, (key, _)) in pairs.iter().enumerate() {
        if !CALLBACK_KEYS.contains(&key.as_str()) || pairs[..index].iter().any(|(k, _)| k == key) {
            return None;
        }
    }
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    if get("v")? != "1" {
        return None;
    }
    let state = get("state")?;
    let token = get("token")?;
    let name = get("name")?.trim();
    let expires = match get("expires") {
        Some(value) => Some(value.parse::<u64>().ok()?),
        None => None,
    };
    if state.len() != 64
        || !state.bytes().all(|b| b.is_ascii_hexdigit())
        || token.is_empty()
        || token.len() > 12_000
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~+/=-".contains(&b))
        || name.is_empty()
        || name.chars().count() > 40
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(Callback {
        state: state.into(),
        token: token.into(),
        name: name.into(),
        environment: TargetEnvironment::parse(get("environment")?)?,
        summary: parse_endpoint(get("summary")?)?,
        open: parse_endpoint(get("open")?)?,
        expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(state: &str) -> String {
        let fragment = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("v", "1")
            .append_pair("state", state)
            .append_pair("token", "aaa.bbb.ccc")
            .append_pair("name", "Delta")
            .append_pair("environment", "staging")
            .append_pair(
                "summary",
                "https://api.staging.delta.example/monitoring/summary",
            )
            .append_pair(
                "open",
                "https://staging.delta-admin.example/monitoring/route-me",
            )
            .append_pair("expires", "1900000000")
            .finish();
        format!("statusbadge://connect#{fragment}")
    }
    #[test]
    fn callbacks_require_exact_route_fragment_nonce_and_deadline() {
        let state = "a".repeat(64);
        let cb = parse_callback(&raw(&state)).unwrap();
        assert_eq!(cb.name, "Delta");
        assert_eq!(cb.environment, TargetEnvironment::Staging);
        assert_eq!(cb.expires, Some(1_900_000_000));
        let pending = PendingLogin {
            base: "https://sudo.delta.example".into(),
            target: None,
            state: state.clone(),
            created_at: 1000,
        };
        assert!(pending.accepts(&cb, 2000));
        assert!(!pending.accepts(&cb, LOGIN_TTL_MS + 1001));
        assert!(!pending.accepts(&parse_callback(&raw(&"b".repeat(64))).unwrap(), 2000));
        let good = raw(&state);
        for bad in [
            good.replace('#', "?"),
            good.replace("connect#", "evil#"),
            good.replace("connect#", "connect/x#"),
            good.replace("connect#", "user@connect#"),
            good.replace("v=1", "v=2"),
            good.replace("environment=staging", "environment=prod"),
            good.replace("summary=https", "summary=http"),
            good.replace("open=https", "open=javascript"),
            good.replace("expires=1900000000", "expires=soon"),
            good.replace("name=Delta", "name="),
            good.replace("token=aaa.bbb.ccc", "token=a%20b"),
            format!("{good}&token=other"),
            format!("{good}&redirect=https%3A%2F%2Fevil.test"),
            good.replace(&format!("state={state}"), "state=abc"),
            good.replace("&expires=1900000000", "")
                .replace("&open=", "&x="),
        ] {
            assert!(parse_callback(&bad).is_none(), "{bad}");
        }
        assert!(parse_callback(&good.replace("&expires=1900000000", "")).is_some());
    }
    #[test]
    fn base_urls_are_https_or_loopback_without_query() {
        assert_eq!(
            normalize_base(" sudo.delta.example/ ").as_deref(),
            Some("https://sudo.delta.example")
        );
        assert_eq!(
            normalize_base("https://staging.delta-admin.example/dashboard?x=1#y").as_deref(),
            Some("https://staging.delta-admin.example")
        );
        assert_eq!(
            normalize_base("http://devtools.app.localhost:5150").as_deref(),
            Some("http://devtools.app.localhost:5150")
        );
        for bad in [
            "http://sudo.delta.example",
            "ftp://x.test",
            "https://user:pw@x.test",
            "",
            "javascript:alert(1)",
        ] {
            assert!(normalize_base(bad).is_none(), "{bad}");
        }
        let pending = PendingLogin {
            base: "https://app.test/__devtools".into(),
            target: None,
            state: "c".repeat(64),
            created_at: 0,
        };
        assert_eq!(
            pending.login_url(),
            format!(
                "https://app.test/__devtools/statusbadge?state={}",
                "c".repeat(64)
            )
        );
    }
}
