//! Conversation messages — internal representation mirroring `src/types/message.ts`.

use serde::{Deserialize, Serialize};

use super::api::{ContentBlock, TextContent, Usage};
use super::newtypes::{MessageUuid, StopReason, ToolUseId};
use super::tools::ToolResultStatus;
use super::tui_types::ToolOutput;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Whether a message is an API error or a normal message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum MessageOrigin {
    Normal,
    ApiError,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields required for conversation history")]
pub struct ConversationMessage {
    pub uuid: MessageUuid,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub origin: MessageOrigin,
    pub stop_reason: Option<StopReason>,
    pub usage: Option<Usage>,
}

impl ConversationMessage {
    #[must_use]
    pub fn user(content: Vec<ContentBlock>) -> Self {
        Self {
            uuid: MessageUuid::new(),
            role: Role::User,
            content,
            origin: MessageOrigin::Normal,
            stop_reason: None,
            usage: None,
        }
    }

    #[must_use]
    pub fn user_text(text: &str) -> Self {
        Self::user(vec![ContentBlock::Text {
            text: TextContent::from(text),
            citations: None,
        }])
    }

    #[must_use]
    pub fn tool_result(tool_use_id: &ToolUseId, output: &ToolOutput, is_error: ToolResultStatus) -> Self {
        Self::user(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: serde_json::Value::String(output.as_ref().to_string()),
            is_error,
        }])
    }

    #[must_use]
    pub fn to_api_message(&self) -> ApiMessage {
        ApiMessage {
            role: self.role,
            content: self.content.clone(),
        }
    }

    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text, .. } => Some(text.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
