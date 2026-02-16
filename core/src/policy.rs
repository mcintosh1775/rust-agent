use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    ObjectRead,
    ObjectWrite,
    MessageSend,
    DbQuery,
    HttpRequest,
}

impl CapabilityKind {
    pub fn from_action_type(action_type: &str) -> Option<Self> {
        match action_type {
            "object.read" => Some(Self::ObjectRead),
            "object.write" => Some(Self::ObjectWrite),
            "message.send" => Some(Self::MessageSend),
            "db.query" => Some(Self::DbQuery),
            "http.request" => Some(Self::HttpRequest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityLimits {
    pub max_payload_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub kind: CapabilityKind,
    pub scope: String,
    #[serde(default)]
    pub limits: CapabilityLimits,
}

impl CapabilityGrant {
    pub fn new(kind: CapabilityKind, scope: impl Into<String>) -> Self {
        Self {
            kind,
            scope: scope.into(),
            limits: CapabilityLimits::default(),
        }
    }

    pub fn with_max_payload_bytes(mut self, max_payload_bytes: u64) -> Self {
        self.limits.max_payload_bytes = Some(max_payload_bytes);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSet {
    pub grants: Vec<CapabilityGrant>,
}

impl GrantSet {
    pub fn new(grants: Vec<CapabilityGrant>) -> Self {
        Self { grants }
    }

    pub fn is_action_allowed(&self, request: &ActionRequest) -> PolicyDecision {
        is_action_allowed(self, request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub action_type: String,
    pub scope: String,
    #[serde(default)]
    pub payload_bytes: u64,
}

impl ActionRequest {
    pub fn new(
        action_type: impl Into<String>,
        scope: impl Into<String>,
        payload_bytes: u64,
    ) -> Self {
        Self {
            action_type: action_type.into(),
            scope: scope.into(),
            payload_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    UnknownActionType,
    CapabilityMissing,
    ScopeMismatch,
    PayloadTooLarge,
}

impl DenyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownActionType => "unknown_action_type",
            Self::CapabilityMissing => "capability_missing",
            Self::ScopeMismatch => "scope_mismatch",
            Self::PayloadTooLarge => "payload_too_large",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", content = "reason", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Deny(DenyReason),
}

pub fn is_action_allowed(grants: &GrantSet, request: &ActionRequest) -> PolicyDecision {
    let Some(kind) = CapabilityKind::from_action_type(request.action_type.as_str()) else {
        return PolicyDecision::Deny(DenyReason::UnknownActionType);
    };

    let kind_grants: Vec<&CapabilityGrant> = grants
        .grants
        .iter()
        .filter(|grant| grant.kind == kind)
        .collect();
    if kind_grants.is_empty() {
        return PolicyDecision::Deny(DenyReason::CapabilityMissing);
    }

    let scoped_grants: Vec<&CapabilityGrant> = kind_grants
        .into_iter()
        .filter(|grant| scope_matches(grant.scope.as_str(), request.scope.as_str()))
        .collect();
    if scoped_grants.is_empty() {
        return PolicyDecision::Deny(DenyReason::ScopeMismatch);
    }

    let within_limit = scoped_grants.iter().any(|grant| {
        grant
            .limits
            .max_payload_bytes
            .map_or(true, |max_payload_bytes| {
                request.payload_bytes <= max_payload_bytes
            })
    });
    if within_limit {
        PolicyDecision::Allow
    } else {
        PolicyDecision::Deny(DenyReason::PayloadTooLarge)
    }
}

fn scope_matches(grant_scope: &str, requested_scope: &str) -> bool {
    match grant_scope.strip_suffix('*') {
        Some(prefix) => requested_scope.starts_with(prefix),
        None => grant_scope == requested_scope,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_action_allowed, ActionRequest, CapabilityGrant, CapabilityKind, DenyReason, GrantSet,
        PolicyDecision,
    };

    #[test]
    fn deny_unknown_action_type() {
        let grants = GrantSet::new(vec![CapabilityGrant::new(
            CapabilityKind::ObjectWrite,
            "shownotes/*",
        )]);
        let request = ActionRequest::new("object.delete", "shownotes/ep245.md", 42);

        let decision = is_action_allowed(&grants, &request);
        assert_eq!(
            decision,
            PolicyDecision::Deny(DenyReason::UnknownActionType)
        );
    }

    #[test]
    fn deny_when_capability_missing() {
        let grants = GrantSet::new(vec![CapabilityGrant::new(
            CapabilityKind::ObjectRead,
            "podcasts/*",
        )]);
        let request = ActionRequest::new("object.write", "shownotes/ep245.md", 42);

        let decision = is_action_allowed(&grants, &request);
        assert_eq!(
            decision,
            PolicyDecision::Deny(DenyReason::CapabilityMissing)
        );
    }

    #[test]
    fn deny_when_scope_mismatch() {
        let grants = GrantSet::new(vec![CapabilityGrant::new(
            CapabilityKind::ObjectWrite,
            "shownotes/*",
        )]);
        let request = ActionRequest::new("object.write", "podcasts/ep245.md", 42);

        let decision = is_action_allowed(&grants, &request);
        assert_eq!(decision, PolicyDecision::Deny(DenyReason::ScopeMismatch));
    }

    #[test]
    fn deny_when_payload_exceeds_limits() {
        let grants = GrantSet::new(vec![CapabilityGrant::new(
            CapabilityKind::ObjectWrite,
            "shownotes/*",
        )
        .with_max_payload_bytes(100)]);
        let request = ActionRequest::new("object.write", "shownotes/ep245.md", 101);

        let decision = is_action_allowed(&grants, &request);
        assert_eq!(decision, PolicyDecision::Deny(DenyReason::PayloadTooLarge));
    }

    #[test]
    fn allow_when_exact_capability_and_scope_match() {
        let grants = GrantSet::new(vec![CapabilityGrant::new(
            CapabilityKind::ObjectWrite,
            "shownotes/*",
        )
        .with_max_payload_bytes(500_000)]);
        let request = ActionRequest::new("object.write", "shownotes/ep245.md", 2300);

        let decision = is_action_allowed(&grants, &request);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn deny_reason_strings_are_stable() {
        assert_eq!(
            DenyReason::UnknownActionType.as_str(),
            "unknown_action_type"
        );
        assert_eq!(DenyReason::CapabilityMissing.as_str(), "capability_missing");
        assert_eq!(DenyReason::ScopeMismatch.as_str(), "scope_mismatch");
        assert_eq!(DenyReason::PayloadTooLarge.as_str(), "payload_too_large");
    }
}
