//! Monitoring contract and validation. No network, UI, or operating-system access.
pub mod auth;
pub mod updates;

use serde::{Deserialize, Serialize};

/// Declared by the registered sudo itself, never chosen in the badge.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum TargetEnvironment {
    Production,
    Staging,
    Development,
}
impl TargetEnvironment {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "production" => Some(Self::Production),
            "staging" => Some(Self::Staging),
            "development" => Some(Self::Development),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Production => "Production",
            Self::Staging => "Staging",
            Self::Development => "Développement",
        }
    }
    pub fn is_production(self) -> bool {
        self == Self::Production
    }
}

/// A registered sudo. Persisted in the preferences; the bearer stays in the Keychain.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Target {
    /// Local identifier, also the Keychain account.
    pub id: String,
    /// URL typed by the user, normalized by `auth::normalize_base`.
    pub base: String,
    pub name: String,
    pub environment: TargetEnvironment,
    pub summary: String,
    pub open: String,
}
impl Target {
    pub fn title(&self) -> String {
        if self.environment.is_production() {
            self.name.clone()
        } else {
            format!("{} · {}", self.name, self.environment.label())
        }
    }
}

/// Orders targets so that the environments of one project are adjacent around the badge.
pub fn sort_targets(targets: &mut [Target]) {
    targets.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.environment.cmp(&b.environment))
            .then(a.base.cmp(&b.base))
    });
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Operational,
    #[default]
    Unknown,
    Watch,
    Degraded,
    Outage,
}
impl Status {
    pub fn color(self) -> u32 {
        match self {
            Self::Operational => 0x00ca91,
            Self::Unknown => 0x98a6b5,
            Self::Watch => 0xd4e157,
            Self::Degraded => 0xfb923c,
            Self::Outage => 0xff5b64,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Operational => "Disponible",
            Self::Unknown => "Inconnu",
            Self::Watch => "À surveiller",
            Self::Degraded => "Dégradé",
            Self::Outage => "Indisponible",
        }
    }
    /// Protocol order: operational < unknown < watch < degraded < outage.
    pub fn severity(self) -> u8 {
        match self {
            Self::Operational => 0,
            Self::Unknown => 1,
            Self::Watch => 2,
            Self::Degraded => 3,
            Self::Outage => 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Section {
    pub id: String,
    pub label: String,
    pub status: Status,
    pub count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub updated_at: Option<u64>,
    pub status: Status,
    pub count: u64,
    pub sections: Vec<Section>,
}
impl Summary {
    pub fn validate(&self) -> bool {
        self.count <= 9_007_199_254_740_991
            && self.sections.len() <= 64
            && self.sections.iter().all(|s| {
                s.count <= 9_007_199_254_740_991
                    && !s.id.is_empty()
                    && s.id.len() <= 64
                    && s.id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    && !s.label.trim().is_empty()
                    && s.label.chars().count() <= 120
                    && !s.label.chars().any(char::is_control)
                    && self
                        .sections
                        .iter()
                        .filter(|other| other.id == s.id)
                        .count()
                        == 1
            })
    }
    pub fn stale(&self, now_ms: u64) -> bool {
        self.updated_at
            .is_none_or(|at| now_ms.saturating_sub(at) > 30_000 || at > now_ms + 60_000)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TargetState {
    pub authenticated: bool,
    pub summary: Option<Summary>,
    pub offline: bool,
    pub message: Option<String>,
}
impl TargetState {
    pub fn requires_login(&self) -> bool {
        !self.authenticated
    }
    pub fn indicator(&self, now_ms: u64) -> (Status, u64) {
        let (status, count) = self
            .summary
            .as_ref()
            .map(|s| (s.status, s.count))
            .unwrap_or_default();
        // Only connection freshness is local. The server owns the global status and count.
        if status == Status::Operational
            && (self.offline || self.summary.as_ref().is_none_or(|s| s.stale(now_ms)))
        {
            (Status::Unknown, count)
        } else {
            (status, count)
        }
    }
    pub fn locked(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

/// Status of the central circle.
///
/// Production targets decide it: a staging or development outage only colours its own
/// satellite. Without any production target, every target counts. A locked target
/// counts as unknown, so a lost session never leaves the centre green.
pub fn combined<'a>(
    targets: impl IntoIterator<Item = (&'a Target, &'a TargetState)>,
    now_ms: u64,
) -> Option<(Status, u64)> {
    let targets: Vec<_> = targets.into_iter().collect();
    let production = targets
        .iter()
        .any(|(target, _)| target.environment.is_production());
    targets
        .into_iter()
        .filter(|(target, _)| !production || target.environment.is_production())
        .map(|(_, state)| {
            if state.requires_login() {
                (Status::Unknown, 0)
            } else {
                state.indicator(now_ms)
            }
        })
        .reduce(|(a, ca), (b, cb)| {
            (
                if b.severity() > a.severity() { b } else { a },
                ca.saturating_add(cb),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn summary() -> Summary {
        serde_json::from_str(include_str!("../../../fixtures/monitoring-summary.json")).unwrap()
    }
    fn target(name: &str, environment: TargetEnvironment) -> Target {
        Target {
            id: name.into(),
            base: format!("https://sudo.{name}.test"),
            name: name.into(),
            environment,
            summary: format!("https://api.{name}.test/summary"),
            open: format!("https://sudo.{name}.test"),
        }
    }
    fn connected(status: Status, count: u64) -> TargetState {
        let mut s = summary();
        s.status = status;
        s.count = count;
        s.updated_at = Some(100_000);
        TargetState {
            authenticated: true,
            summary: Some(s),
            ..Default::default()
        }
    }
    #[test]
    fn centre_follows_production_and_ignores_other_environments() {
        let prod = target("delta", TargetEnvironment::Production);
        let other = target("cobalt", TargetEnvironment::Production);
        let staging = target("delta", TargetEnvironment::Staging);
        let ok = connected(Status::Operational, 0);
        let watch = connected(Status::Watch, 2);
        let down = connected(Status::Outage, 5);
        assert_eq!(
            combined([(&prod, &ok), (&other, &watch), (&staging, &down)], 110_000),
            Some((Status::Watch, 2))
        );
        assert_eq!(
            combined([(&staging, &down)], 110_000),
            Some((Status::Outage, 5))
        );
        let locked = TargetState::default();
        assert_eq!(
            combined([(&prod, &ok), (&other, &locked)], 110_000),
            Some((Status::Unknown, 0))
        );
        assert_eq!(combined([], 110_000), None);
    }
    #[test]
    fn environments_of_a_project_stay_adjacent() {
        let mut targets = vec![
            target("delta", TargetEnvironment::Staging),
            target("cobalt", TargetEnvironment::Production),
            target("Delta", TargetEnvironment::Production),
        ];
        sort_targets(&mut targets);
        assert_eq!(
            targets
                .iter()
                .map(|t| (t.name.as_str(), t.environment))
                .collect::<Vec<_>>(),
            [
                ("cobalt", TargetEnvironment::Production),
                ("Delta", TargetEnvironment::Production),
                ("delta", TargetEnvironment::Staging)
            ]
        );
    }
    #[test]
    fn uses_the_server_status_and_count_without_aggregating_sections() {
        let mut s = summary();
        s.status = Status::Degraded;
        s.count = 7;
        s.sections = vec![Section {
            id: "new-category".into(),
            label: "Nouvelle catégorie".into(),
            status: Status::Outage,
            count: 100,
        }];
        assert!(s.validate());
        let state = TargetState {
            authenticated: true,
            summary: Some(s),
            ..Default::default()
        };
        assert_eq!(state.indicator(110_000), (Status::Degraded, 7));
    }
    #[test]
    fn incomplete_contracts_duplicate_ids_and_invalid_values_are_rejected() {
        for missing in ["status", "count"] {
            let mut value = serde_json::to_value(summary()).unwrap();
            value.as_object_mut().unwrap().remove(missing);
            assert!(serde_json::from_value::<Summary>(value).is_err());
        }
        let mut s = summary();
        s.sections.push(s.sections[0].clone());
        assert!(!s.validate());
        for label in [
            "".to_string(),
            " ".into(),
            "x".repeat(121),
            "two\nlines".into(),
        ] {
            let mut s = summary();
            s.sections[0].label = label;
            assert!(!s.validate());
        }
        for id in ["".to_string(), "x".repeat(65), "invalid/path".into()] {
            let mut s = summary();
            s.sections[0].id = id;
            assert!(!s.validate());
        }
        let mut s = summary();
        s.count = 9_007_199_254_740_992;
        assert!(!s.validate());
    }
    #[test]
    fn stale_or_offline_data_never_looks_green_and_preserves_the_server_alarm() {
        let mut state = connected(Status::Operational, 0);
        assert_eq!(state.indicator(110_000).0, Status::Operational);
        assert_eq!(state.indicator(140_000).0, Status::Unknown);
        state.offline = true;
        assert_eq!(state.indicator(110_000).0, Status::Unknown);
        for status in [Status::Watch, Status::Degraded, Status::Outage] {
            state.summary.as_mut().unwrap().status = status;
            assert_eq!(state.indicator(140_000).0, status);
        }
    }
}
