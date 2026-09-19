use serde::{Deserialize, Serialize};

pub const DEFAULT_BIND: &str = "127.0.0.1:7420";
pub const DEFAULT_MODEL: &str = "llama3.2";
pub const DEFAULT_OLLAMA: &str = "http://127.0.0.1:11434";
pub const DEFAULT_PACK: &str = "hearthside";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub home: String,
    pub bind: String,
    pub pack: String,
    pub model: String,
    pub ollama: String,
    pub ollama_reachable: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResponse {
    pub pack: String,
    pub memory_context: String,
    pub reply: String,
    pub model_used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

/// How long a grant remains usable after the user decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantScope {
    /// Single write/action; consumed after one successful use.
    Once,
    /// Valid for the duration of the current task/session (until revoke or daemon restart).
    Task,
}

/// User decision on a pending grant request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantDecision {
    AllowOnce,
    AllowTask,
    Deny,
}

/// Lifecycle of a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantStatus {
    Pending,
    Allowed,
    Denied,
    Consumed,
}

/// A capability request that must be approved when `writes_require_ask` is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub created_at: String,
    /// Short machine action key, e.g. `plot.write`, `mail.send`.
    pub action: String,
    /// Human-readable summary shown in CLI / future Ask banner.
    pub summary: String,
    /// Optional adapter or tool that requested the grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    pub status: GrantStatus,
    /// Set when status is Allowed (Once or Task).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<GrantScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGrantRequest {
    pub action: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideGrantRequest {
    pub decision: GrantDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantListResponse {
    pub grants: Vec<Grant>,
}

/// Result of checking whether a write may proceed under the active charter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePermission {
    /// Charter does not require Ask for writes, or a matching Allowed grant exists.
    Allowed,
    /// Charter has `writes_require_ask` and no usable grant for this action.
    NeedsGrant,
    /// A pending grant already exists for this action (wait for user decision).
    PendingExists,
}

// --- Capability tokens (shrink-only) ---

/// Issue a root token with full charter rights, or a subset if `rights` is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTokenRequest {
    /// Optional subset of charter rights. If empty/omitted, issue full charter set.
    #[serde(default)]
    pub rights: Vec<String>,
    /// Optional TTL in seconds. Tokens without TTL last until daemon restart (MAC key is ephemeral).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
}

/// Attenuate an existing token to a subset of its rights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttenuateTokenRequest {
    pub token: serde_json::Value,
    /// Rights to keep (must be a subset of the parent token).
    pub keep: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyTokenRequest {
    pub token: serde_json::Value,
    /// Optional right that must be present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyTokenResponse {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<Vec<String>>,
}
