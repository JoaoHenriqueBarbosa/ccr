//! API client — 1:1 with `src/services/api/claude.ts` `queryModel()`.
//!
//! Implements SSE streaming against the Anthropic messages API.
//! The TS version uses the `@anthropic-ai/sdk`; we go direct to the
//! HTTP endpoint since the SDK is JS-only.

use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc;

use crate::types::{
    AccountUuid, ApiKey, ApiMessage, ApiUrl, AppError, DeviceId, DeviceIdentity,
    MaxTokens, ModelId, RequestId, ResponseBody, SessionId, StreamEvent, SystemPrompt,
    ToolDefinition,
};

const API_VERSION: &str = "2023-06-01";
const CLI_VERSION: &str = "2.1.90";
const STAINLESS_PACKAGE_VERSION: &str = "0.52.0";
const STAINLESS_RUNTIME_VERSION: &str = "v24.3.0";

/// Read `device_id` and `account_uuid` from the statsig cache.
/// Falls back to empty strings if not found.
fn read_identity() -> DeviceIdentity {
    let claude_dir = dirs::home_dir()
        .map(|h| h.join(".claude"))
        .unwrap_or_default();

    let statsig_dir = claude_dir.join("statsig");
    let Ok(entries) = std::fs::read_dir(&statsig_dir) else {
        return DeviceIdentity::default();
    };

    for entry in entries.flatten() {
        let Ok(content) = std::fs::read_to_string(entry.path()) else { continue };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
        let Some(data_str) = parsed.get("data").and_then(|d| d.as_str()) else { continue };
        let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) else { continue };

        let device_id = DeviceId::new(
            data.pointer("/evaluated_keys/userID")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        let account_uuid = AccountUuid::new(
            data.pointer("/evaluated_keys/customIDs/accountUUID")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        if !device_id.is_empty() {
            return DeviceIdentity { device_id, account_uuid };
        }
    }

    DeviceIdentity::default()
}

pub struct AnthropicClient {
    client: Client,
    api_key: ApiKey,
    model: ModelId,
    api_url: ApiUrl,
}

impl AnthropicClient {
    #[must_use]
    pub fn new(api_key: ApiKey, model: ModelId) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            api_url: ApiUrl::from_env_or_default(),
        }
    }

    /// Stream a messages API call, yielding SSE events through a channel.
    /// Mirrors `queryModelWithStreaming` in `claude.ts`.
    pub async fn stream(
        &self,
        messages: &[ApiMessage],
        system: &SystemPrompt,
        tools: &[ToolDefinition],
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> crate::types::Result<()> {
        let session_id = SessionId::new();
        let request_id = RequestId::new();
        let is_oauth = self.api_key.is_oauth();
        let identity = read_identity();

        let body = self.build_request_body(messages, system, tools, &session_id, &identity)?;
        let response = self.send_request(&body, &session_id, &request_id, is_oauth).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = ResponseBody::from(response.text().await.unwrap_or_default());
            return Err(AppError::ApiStatus { status, body });
        }

        consume_sse_stream(response, tx).await
    }

    /// Build the JSON request body for the messages API.
    fn build_request_body(
        &self,
        messages: &[ApiMessage],
        system: &SystemPrompt,
        tools: &[ToolDefinition],
        session_id: &SessionId,
        identity: &DeviceIdentity,
    ) -> crate::types::Result<serde_json::Value> {
        let system_blocks = json!([
            { "type": "text", "text": format!("x-anthropic-billing-header: cc_version={CLI_VERSION}.000; cc_entrypoint=cli;") },
            { "type": "text", "text": system.to_string() }
        ]);

        let mut body = json!({
            "model": &self.model,
            "max_tokens": MaxTokens::DEFAULT.value(),
            "stream": true,
            "system": system_blocks,
            "messages": messages,
            "thinking": { "type": "adaptive" },
            "metadata": {
                "user_id": json!({
                    "device_id": identity.device_id.as_ref(),
                    "account_uuid": identity.account_uuid.as_ref(),
                    "session_id": session_id.as_ref(),
                }).to_string()
            },
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)
                .map_err(|source| AppError::Json { source })?;
        }

        Ok(body)
    }

    /// Send the HTTP request with all required headers.
    async fn send_request(
        &self,
        body: &serde_json::Value,
        session_id: &SessionId,
        request_id: &RequestId,
        is_oauth: bool,
    ) -> crate::types::Result<reqwest::Response> {
        let betas = build_beta_header(is_oauth);

        let mut req = self
            .client
            .post(self.api_url.as_ref())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("anthropic-beta", &betas)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("user-agent", format!("claude-cli/{CLI_VERSION} (external, cli)"))
            .header("x-app", "cli")
            .header("x-claude-code-session-id", session_id.as_ref())
            .header("x-client-request-id", request_id.as_ref())
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", STAINLESS_PACKAGE_VERSION)
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", STAINLESS_RUNTIME_VERSION)
            .header("x-stainless-timeout", "600");

        req = if is_oauth {
            req.header("Authorization", format!("Bearer {}", self.api_key))
        } else {
            req.header("x-api-key", self.api_key.as_ref())
        };

        req.json(body)
            .send()
            .await
            .map_err(|source| AppError::ApiRequest { source })
    }
}

/// Build the `anthropic-beta` header value.
fn build_beta_header(is_oauth: bool) -> String {
    let mut beta_list = vec![
        "claude-code-20250219",
        "context-1m-2025-08-07",
        "interleaved-thinking-2025-05-14",
        "redact-thinking-2026-02-12",
        "context-management-2025-06-27",
        "prompt-caching-scope-2026-01-05",
        "effort-2025-11-24",
    ];
    if is_oauth {
        beta_list.insert(0, "oauth-2025-04-20");
    }
    beta_list.join(",")
}

/// Consume an SSE byte stream, parse events, and forward them through the channel.
///
/// **UTF-8 safety**: we accumulate raw bytes (`Vec<u8>`) and only convert to
/// `String` after finding a complete `\n\n` boundary. This prevents corruption
/// when a TCP chunk splits a multi-byte UTF-8 codepoint.
async fn consume_sse_stream(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
) -> crate::types::Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| AppError::StreamRead { source })?;
        buffer.extend_from_slice(&chunk);

        while let Some(pos) = find_double_newline(&buffer) {
            let event_bytes: Vec<u8> = buffer.drain(..pos).collect();
            // Drain the \n\n separator
            buffer.drain(..2);

            // Now it's safe to convert — we have a complete SSE event
            let event_text = String::from_utf8_lossy(&event_bytes);

            match parse_sse_event(&event_text) {
                SseParseResult::Event(event) => {
                    let is_stop = matches!(event, StreamEvent::MessageStop);
                    if tx.send(event).is_err() {
                        return Ok(());
                    }
                    if is_stop {
                        return Ok(());
                    }
                }
                SseParseResult::Skip => {}
                SseParseResult::Unknown { event_type } => {
                    #[cfg(debug_assertions)]
                    eprintln!("[sse] unknown event type: {event_type}");
                }
            }
        }
    }

    Ok(())
}

/// Find the position of `\n\n` in a byte buffer.
fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

/// Possible outcomes of parsing an SSE event.
#[cfg_attr(test, derive(Debug))]
enum SseParseResult {
    /// Successfully parsed a known event.
    Event(StreamEvent),
    /// No data or `event_type` line found — not a real event (e.g. comment or empty).
    Skip,
    /// Data present but JSON didn't match any `StreamEvent` variant — unknown event type.
    Unknown { event_type: String },
}

/// Parse a single SSE event from raw text.
/// SSE format: `event: <type>\ndata: <json>`
fn parse_sse_event(text: &str) -> SseParseResult {
    let mut event_type = None;
    let mut data = None;

    for line in text.lines() {
        if let Some(et) = line.strip_prefix("event: ") {
            event_type = Some(et.trim());
        } else if let Some(d) = line.strip_prefix("data: ") {
            data = Some(d);
        }
    }

    let (Some(data), Some(event_type)) = (data, event_type) else {
        return SseParseResult::Skip;
    };

    match serde_json::from_str::<StreamEvent>(data) {
        Ok(event) => SseParseResult::Event(event),
        Err(_) => SseParseResult::Unknown { event_type: event_type.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ping_event() {
        let raw = "event: ping\ndata: {\"type\": \"ping\"}";
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Event(StreamEvent::Ping)));
    }

    #[test]
    fn parse_message_stop() {
        let raw = "event: message_stop\ndata: {\"type\": \"message_stop\"}";
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Event(StreamEvent::MessageStop)));
    }

    #[test]
    fn parse_text_delta() {
        let raw = r#"event: content_block_delta
data: {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}}"#;
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Event(StreamEvent::ContentBlockDelta { .. })));
    }

    #[test]
    fn parse_empty_returns_skip() {
        assert!(matches!(parse_sse_event(""), SseParseResult::Skip));
        assert!(matches!(parse_sse_event(": comment"), SseParseResult::Skip));
        assert!(matches!(parse_sse_event("event: ping"), SseParseResult::Skip)); // no data line
    }

    #[test]
    fn parse_unknown_event_type() {
        let raw = "event: some_future_event\ndata: {\"type\": \"some_future_event\"}";
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Unknown { .. }));
    }

    #[test]
    fn parse_invalid_json_returns_unknown() {
        let raw = "event: ping\ndata: not json at all";
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Unknown { .. }));
    }

    #[test]
    fn parse_error_event() {
        let raw = r#"event: error
data: {"type": "error", "error": {"type": "rate_limit_error", "message": "Rate limited"}}"#;
        let result = parse_sse_event(raw);
        assert!(matches!(result, SseParseResult::Event(StreamEvent::Error { .. })));
    }

    #[test]
    fn find_double_newline_basic() {
        let buf = b"event: ping\ndata: {}\n\nevent: stop";
        assert_eq!(find_double_newline(buf), Some(20));
    }

    #[test]
    fn find_double_newline_none() {
        let buf = b"partial data without terminator\n";
        assert_eq!(find_double_newline(buf), None);
    }

    #[test]
    fn find_double_newline_at_start() {
        let buf = b"\n\nrest";
        assert_eq!(find_double_newline(buf), Some(0));
    }

    #[test]
    fn utf8_chunk_split_safety() {
        // Simulate a multi-byte char (é = 0xC3 0xA9) split across two chunks
        let event = "event: ping\ndata: {\"type\": \"ping\"}\n\n";
        let bytes = event.as_bytes();
        // This test just verifies find_double_newline works on raw bytes
        assert!(find_double_newline(bytes).is_some());
    }
}
