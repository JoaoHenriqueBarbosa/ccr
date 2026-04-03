//! Authentication — API key resolution from environment, OAuth, and credentials file.
//!
//! Mirrors TS `getAnthropicApiKey` — checks env, then `CLAUDE_CODE_OAUTH_TOKEN`,
//! then `~/.claude/.credentials.json → claudeAiOauth.accessToken`.

use crate::types::{ApiKey, AppError};

/// Resolve the API key from environment variables or credentials file.
pub fn resolve_api_key() -> crate::types::Result<ApiKey> {
    let raw = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("CLAUDE_CODE_OAUTH_TOKEN"))
        .ok()
        .or_else(read_credentials_file)
        .ok_or(AppError::NoApiKey)?;

    Ok(ApiKey::new(raw))
}

/// Read the access token from `~/.claude/.credentials.json`.
/// Returns `None` if not found, unreadable, or malformed.
fn read_credentials_file() -> Option<String> {
    let config_dir = dirs::home_dir()?.join(".claude");
    let credentials_path = config_dir.join(".credentials.json");

    let data = std::fs::read_to_string(credentials_path).ok()?;
    let creds: serde_json::Value = serde_json::from_str(&data).ok()?;

    creds
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(String::from)
}
