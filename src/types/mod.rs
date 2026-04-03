//! Domain types — 1:1 mapping from `src/types/message.ts` and
//! `@anthropic-ai/sdk` content block types.
//!
//! All domain strings are newtypes — no raw `String` for IDs, names, or keys.
//! Split into submodules by domain:
//!   - `newtypes` — domain IDs, keys, names
//!   - `api` — content blocks, deltas, SSE events, usage
//!   - `message` — conversation messages, roles
//!   - `error` — `AppError` enum and `Result` alias
//!   - `tui` — terminal rendering types
//!   - `tools` — tool definitions

mod api;
mod error;
mod message;
mod newtypes;
mod tools;
mod tui_types;

// ─── String newtype standard traits ─────────────────────────────
//
// Every string newtype gets Display + AsRef<str> via this macro.
// New newtype(String) types MUST be added to the invocation below.

macro_rules! impl_string_newtype {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl std::fmt::Display for $ty {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(&self.0)
                }
            }

            impl AsRef<str> for $ty {
                fn as_ref(&self) -> &str {
                    &self.0
                }
            }
        )+
    };
}

impl_string_newtype!(
    // newtypes
    newtypes::StopReason,
    newtypes::ToolUseId,
    newtypes::ToolName,
    newtypes::ModelId,
    newtypes::ShortModelName,
    newtypes::ApiKey,
    newtypes::SessionId,
    newtypes::DeviceId,
    newtypes::AccountUuid,
    newtypes::MessageUuid,
    newtypes::RequestId,
    newtypes::ApiUrl,
    newtypes::StopSequence,
    // api
    api::ErrorType,
    api::ErrorMessage,
    api::ApiResponseId,
    api::DeltaText,
    api::PartialJson,
    api::DeltaThinking,
    api::DeltaSignature,
    api::TextContent,
    api::ThinkingContent,
    api::ThinkingSignature,
    // error
    error::ResponseBody,
    // tui
    tui_types::ShortPath,
    tui_types::WorkingDir,
    tui_types::SystemPrompt,
    tui_types::StreamingBuffer,
    // tools
    tools::ToolDescription,
    tools::CommandDescription,
    tools::CommandText,
    tools::FilePath,
    tools::FileContent,
    tools::GlobPattern,
    tools::SearchPattern,
    tools::SearchPath,
    tools::GlobFilter,
    tools::OldString,
    tools::NewString,
    tools::PdfPages,
    tools::RgFileType,
    tools::FetchUrl,
    tools::FetchPrompt,
);

// Re-export types consumed by other modules via `use crate::types::Foo`.
mod reexports {
    pub use super::api::{ContentBlock, Delta, DeltaText, DeltaThinking, StreamEvent, Usage};
    pub use super::error::{AppError, ResponseBody, Result};
    pub use super::message::{ApiMessage, ConversationMessage, MessageOrigin, Role};
    pub use super::newtypes::{
        AccountUuid, ApiKey, ApiUrl, DeviceId, DeviceIdentity, MaxTokens, MessageUuid, ModelId,
        RequestId, SessionId, ShortModelName, StopReason, ToolName, ToolUseId,
    };
    pub use super::tools::{
        BashInput, BuiltinTool, CaseSensitivity, EditInput, ExecutionMode, ExitCode,
        FetchTimeoutSecs, FileEncoding, FileSizeBytes, GlobInput, GlobResultLimit, GlobResultOffset,
        GrepInput, GrepOutputMode, HeadLimit, LargeOutputThreshold, LineEndings, LineLimit,
        LineNumberDisplay, LineOffset, MaxHttpContentLength, MaxMarkdownLength, MaxOutputLen,
        MaxReadFileSize, MaxUrlLength, MaxWriteFileSize, MultilineSearch, PreviewLen, ReadInput,
        ResultOffset, TimeoutMs, ToolDefinition, ToolResultStatus, UserShell, WebFetchInput,
        WriteInput,
    };
    pub use super::tui_types::{
        InputBuffer, InputHistory, ScrollOffset, ShortPath, StreamingBuffer, SystemPrompt,
        TermRows, TokenCount, ToolOutput, TurnTimer, WorkingDir,
    };
}
pub use reexports::*;

#[cfg(test)]
mod tests;
