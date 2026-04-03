//! Domain newtypes — IDs, keys, names, identities.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stop reason from the API (e.g. `end_turn`, `tool_use`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StopReason(pub(super) String);

impl From<&str> for StopReason {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique ID for a `tool_use` block, assigned by the API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolUseId(pub(super) String);

/// Registered tool name (e.g. `Bash`, `Read`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolName(pub(super) String);

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// UUID assigned to each `ConversationMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageUuid(pub(super) String);

impl MessageUuid {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// API model identifier (e.g. `claude-opus-4-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub(super) String);

/// Abbreviated model name for status bar display (e.g. `opus-4-6`).
#[derive(Debug, Clone)]
#[must_use]
pub struct ShortModelName(pub(super) String);

impl ModelId {
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self(raw)
    }

    /// Produce a shortened display name (strips `claude-` prefix and date suffixes).
    pub fn short_name(&self) -> ShortModelName {
        ShortModelName(
            self.0
                .replace("claude-", "")
                .replace("-20250514", ""),
        )
    }
}

/// API key — either OAuth token or direct API key.
#[derive(Debug, Clone)]
pub struct ApiKey(pub(super) String);

impl ApiKey {
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self(raw)
    }

    /// OAuth tokens from Anthropic start with `sk-ant-oat`.
    #[must_use]
    pub fn is_oauth(&self) -> bool {
        self.0.starts_with("sk-ant-oat") || self.0.starts_with("ey")
    }
}

/// Session ID for the current Claude Code session.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SessionId(pub(super) String);

impl SessionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// Statsig device ID (hex string from `evaluated_keys.userID`).
#[derive(Debug, Clone)]
pub struct DeviceId(pub(super) String);

impl DeviceId {
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self(raw)
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Anthropic account UUID (from `evaluated_keys.customIDs.accountUUID`).
#[derive(Debug, Clone)]
pub struct AccountUuid(pub(super) String);

impl AccountUuid {
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self(raw)
    }
}

/// Device identity read from `~/.claude/statsig` cache.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    pub device_id: DeviceId,
    pub account_uuid: AccountUuid,
}

impl Default for DeviceIdentity {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new(String::new()),
            account_uuid: AccountUuid::new(String::new()),
        }
    }
}

/// Unique request ID for tracing a single API call.
#[derive(Debug, Clone)]
#[must_use]
pub struct RequestId(pub(super) String);

impl RequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// API base URL (e.g. `https://api.anthropic.com/v1/messages`).
#[derive(Debug, Clone)]
#[must_use]
pub struct ApiUrl(pub(super) String);

impl ApiUrl {
    pub fn from_env_or_default() -> Self {
        Self(
            std::env::var("CLAUDE_API_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com/v1/messages".into()),
        )
    }
}

/// Maximum tokens the API should generate per response.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxTokens(u64);

impl MaxTokens {
    pub const DEFAULT: Self = Self(64000);

    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Stop sequence string from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StopSequence(pub(super) String);
