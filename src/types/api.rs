//! API types — content blocks, deltas, SSE events, usage.
//!
//! 1:1 mapping from `@anthropic-ai/sdk` `messages.d.ts`.
//! Every type, every field, every variant — no shortcuts.

use serde::{Deserialize, Serialize};

use super::newtypes::{ModelId, StopReason, StopSequence, ToolName, ToolUseId};
use super::tools::ToolResultStatus;

// ─── Content block newtypes ─────────────────────────────────────

/// Text content in a message block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TextContent(pub(super) String);

impl From<String> for TextContent {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TextContent {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Thinking content from the model's reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThinkingContent(pub(super) String);

/// Cryptographic signature for a thinking block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThinkingSignature(pub(super) String);

/// Redacted thinking data (opaque blob from the API).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedactedData(pub(super) String);

// ─── Citations ──────────────────────────────────────────────────

/// Citation pointing to a character range in a plain-text document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationCharLocation {
    pub cited_text: String,
    pub document_index: u32,
    pub document_title: Option<String>,
    pub end_char_index: u32,
    pub start_char_index: u32,
}

/// Citation pointing to a page range in a PDF document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationPageLocation {
    pub cited_text: String,
    pub document_index: u32,
    pub document_title: Option<String>,
    pub end_page_number: u32,
    pub start_page_number: u32,
}

/// Citation pointing to a content block range in a content document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationContentBlockLocation {
    pub cited_text: String,
    pub document_index: u32,
    pub document_title: Option<String>,
    pub end_block_index: u32,
    pub start_block_index: u32,
}

/// Citation pointing to a web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationsWebSearchResultLocation {
    pub cited_text: String,
    pub encrypted_index: String,
    pub title: Option<String>,
    pub url: String,
}

/// Union of all citation types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
#[allow(clippy::enum_variant_names)] // variant names match the API type strings
pub enum TextCitation {
    #[serde(rename = "char_location")]
    CharLocation(CitationCharLocation),
    #[serde(rename = "page_location")]
    PageLocation(CitationPageLocation),
    #[serde(rename = "content_block_location")]
    ContentBlockLocation(CitationContentBlockLocation),
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation(CitationsWebSearchResultLocation),
}

// ─── Web search types ───────────────────────────────────────────

/// A single web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResultBlock {
    pub encrypted_content: String,
    pub page_age: Option<String>,
    pub title: String,
    pub url: String,
}

/// Error from the web search tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchToolResultError {
    pub error_code: String,
}

/// Content of a web search tool result — either results or an error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum WebSearchToolResultContent {
    Error(WebSearchToolResultError),
    Results(Vec<WebSearchResultBlock>),
}

// ─── Content blocks ─────────────────────────────────────────────

/// All content block types returned in API responses.
/// 1:1 with SDK `ContentBlock` union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ContentBlock {
    /// Regular text content, optionally with citations.
    #[serde(rename = "text")]
    Text {
        text: TextContent,
        #[serde(default)]
        citations: Option<Vec<TextCitation>>,
    },
    /// Tool use requested by the model.
    #[serde(rename = "tool_use")]
    ToolUse {
        id: ToolUseId,
        name: ToolName,
        input: serde_json::Value,
    },
    /// Tool result sent back to the model (input-side, but appears in messages).
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: ToolUseId,
        content: serde_json::Value,
        #[serde(default)]
        is_error: ToolResultStatus,
    },
    /// Server-side tool use (e.g. web search).
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: ToolUseId,
        name: ToolName,
        input: serde_json::Value,
    },
    /// Result from a server-side web search tool.
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        tool_use_id: ToolUseId,
        content: WebSearchToolResultContent,
    },
    /// Extended thinking block with cryptographic signature.
    #[serde(rename = "thinking")]
    Thinking {
        thinking: ThinkingContent,
        signature: ThinkingSignature,
    },
    /// Redacted thinking block (opaque data).
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: RedactedData },
}

// ─── Token counts ───────────────────────────────────────────────

/// Single-message token count from API — distinct from cumulative `TokenCount`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct ApiTokens(u64);

impl ApiTokens {
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Server tool usage stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolUsage {
    pub web_search_requests: u32,
}

/// Token usage for a full API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)] // serde field names match the API
pub struct Usage {
    pub input_tokens: ApiTokens,
    pub output_tokens: ApiTokens,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<ApiTokens>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<ApiTokens>,
    #[serde(default)]
    pub server_tool_use: Option<ServerToolUsage>,
}

/// Token usage delta in `message_delta` events (all fields nullable except `output_tokens`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct MessageDeltaUsage {
    #[serde(default)]
    pub input_tokens: Option<ApiTokens>,
    pub output_tokens: ApiTokens,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<ApiTokens>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<ApiTokens>,
    #[serde(default)]
    pub server_tool_use: Option<ServerToolUsage>,
}

// ─── API response ───────────────────────────────────────────────

/// API-assigned message ID (e.g. `msg_01XFDUDYJgAACzvnptvVoYEL`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
#[allow(dead_code, reason = "required by serde deserialization")]
pub struct ApiResponseId(pub(super) String);

/// Full API response (non-streaming) — fields required for serde deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code, reason = "required by serde deserialization")]
pub struct ApiResponse {
    pub id: ApiResponseId,
    pub model: ModelId,
    pub role: super::message::Role,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<StopReason>,
    pub usage: Usage,
}

// ─── SSE streaming events ───────────────────────────────────────

/// SSE content block index — positional index within a streamed message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
#[allow(dead_code, reason = "required by serde deserialization")]
pub struct BlockIndex(usize);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
#[allow(dead_code, reason = "fields required by serde deserialization")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: ApiResponse },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: BlockIndex,
        content_block: ContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: BlockIndex,
        delta: Delta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: BlockIndex },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaBody,
        usage: Option<MessageDeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "error")]
    Error { error: ApiError },
}

// ─── Delta types ────────────────────────────────────────────────
// 1:1 with SDK `RawContentBlockDelta` union:
//   TextDelta | InputJSONDelta | CitationsDelta | ThinkingDelta | SignatureDelta

/// Text fragment from an SSE `text_delta` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeltaText(pub(super) String);

/// Partial JSON fragment from an SSE `input_json_delta` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartialJson(pub(super) String);

/// Thinking fragment from an SSE `thinking_delta` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeltaThinking(pub(super) String);

/// Signature fragment from an SSE `signature_delta` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeltaSignature(pub(super) String);

/// All delta types that can appear inside a `content_block_delta` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: DeltaText },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: PartialJson },
    #[serde(rename = "thinking_delta")]
    Thinking { thinking: DeltaThinking },
    #[serde(rename = "signature_delta")]
    Signature { signature: DeltaSignature },
    #[serde(rename = "citations_delta")]
    Citations { citation: TextCitation },
}

/// Body of a `message_delta` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaBody {
    pub stop_reason: Option<StopReason>,
    #[serde(default)]
    pub stop_sequence: Option<StopSequence>,
}

// ─── Error types (API-side) ─────────────────────────────────────

/// API error type string (e.g. `rate_limit_error`, `invalid_request_error`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ErrorType(pub(super) String);

impl From<&str> for ErrorType {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// API error message — human-readable description from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ErrorMessage(pub(super) String);

impl From<String> for ErrorMessage {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ErrorMessage {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code, reason = "required by serde deserialization")]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    pub message: ErrorMessage,
}
