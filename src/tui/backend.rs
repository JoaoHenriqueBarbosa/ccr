//! Backend task — streams API responses, executes tools, loops.
//!
//! Runs in a `tokio::spawn` task, communicates with the UI via `BackendEvent` channel.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::api::AnthropicClient;
use crate::tools::execute_tool;
use crate::types::{
    ApiMessage, AppError, ContentBlock, ConversationMessage, Delta, MessageOrigin,
    MessageUuid, Role, StopReason, StreamEvent, StreamingBuffer, SystemPrompt,
    ToolDefinition, ToolName, ToolUseId, Usage, WorkingDir,
};

use super::state::BackendEvent;

/// Stream accumulator — typestate `BlockAccum` transitions:
/// `Idle → Text { buf } | Tool { id, name, json_buf } → Idle`
///
/// Returns `None` if the stream produced an error (already sent to UI).
async fn accumulate_stream(
    stream_rx: &mut mpsc::UnboundedReceiver<StreamEvent>,
    tx: &mpsc::UnboundedSender<BackendEvent>,
) -> Option<(Vec<ContentBlock>, Option<StopReason>, Option<Usage>)> {
    enum BlockAccum {
        Idle,
        Text { buf: StreamingBuffer },
        Tool { id: ToolUseId, name: ToolName, json_buf: StreamingBuffer },
    }

    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut accum = BlockAccum::Idle;
    let mut stop_reason = None;
    let mut usage = None;

    while let Some(event) = stream_rx.recv().await {
        match &event {
            StreamEvent::ContentBlockStart { content_block, .. } => {
                accum = match content_block {
                    ContentBlock::Text { .. } => BlockAccum::Text { buf: StreamingBuffer::default() },
                    ContentBlock::ToolUse { id, name, .. } => BlockAccum::Tool {
                        id: id.clone(),
                        name: name.clone(),
                        json_buf: StreamingBuffer::default(),
                    },
                    _ => BlockAccum::Idle,
                };
            }
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                Delta::Text { text } => {
                    if let BlockAccum::Text { buf } = &mut accum {
                        buf.push(text.as_ref());
                    }
                    let _ = tx.send(BackendEvent::StreamDelta(text.clone()));
                }
                Delta::InputJson { partial_json } => {
                    if let BlockAccum::Tool { json_buf, .. } = &mut accum {
                        json_buf.push(partial_json.as_ref());
                    }
                }
                Delta::Thinking { thinking } => {
                    let _ = tx.send(BackendEvent::ThinkingDelta(thinking.clone()));
                }
                // Signature deltas are accumulated server-side; we don't render them.
                Delta::Signature { .. } | Delta::Citations { .. } => {}
            },
            StreamEvent::ContentBlockStop { .. } => {
                match std::mem::replace(&mut accum, BlockAccum::Idle) {
                    BlockAccum::Tool { id, name, json_buf } => {
                        let input: serde_json::Value =
                            serde_json::from_str(json_buf.as_ref()).unwrap_or_default();
                        content_blocks.push(ContentBlock::ToolUse { id, name, input });
                    }
                    BlockAccum::Text { buf } if !buf.is_empty() => {
                        content_blocks.push(ContentBlock::Text { text: buf.into_string().into(), citations: None });
                    }
                    _ => {}
                }
            }
            StreamEvent::MessageDelta { delta, usage: u } => {
                stop_reason.clone_from(&delta.stop_reason);
                if let Some(u) = u {
                    usage = Some(Usage {
                        input_tokens: u.input_tokens.unwrap_or_default(),
                        output_tokens: u.output_tokens,
                        cache_creation_input_tokens: u.cache_creation_input_tokens,
                        cache_read_input_tokens: u.cache_read_input_tokens,
                        server_tool_use: u.server_tool_use.clone(),
                    });
                }
            }
            StreamEvent::Error { error } => {
                let _ = tx.send(BackendEvent::Error(AppError::ApiStreamError {
                    message: error.message.clone(),
                }));
                return None;
            }
            StreamEvent::MessageStop => break,
            _ => {}
        }
    }

    Some((content_blocks, stop_reason, usage))
}

pub async fn backend_turn(
    client: Arc<AnthropicClient>,
    mut messages: Vec<ApiMessage>,
    system_prompt: Arc<SystemPrompt>,
    tools: Arc<[ToolDefinition]>,
    cwd: WorkingDir,
    tx: mpsc::UnboundedSender<BackendEvent>,
) {
    loop {
        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamEvent>();
        let stream_client = Arc::clone(&client);
        let api_msgs = messages.clone();
        let sys = Arc::clone(&system_prompt);
        let tls = Arc::clone(&tools);
        let err_tx = tx.clone();

        tokio::spawn(async move {
            if let Err(e) = stream_client.stream(&api_msgs, &sys, &tls, stream_tx).await {
                let _ = err_tx.send(BackendEvent::Error(e));
            }
        });

        let Some((content_blocks, stop_reason, usage)) =
            accumulate_stream(&mut stream_rx, &tx).await
        else {
            return;
        };

        // Extract tool uses before moving content_blocks into the message
        let tool_uses: Vec<(ToolUseId, ToolName, serde_json::Value)> = content_blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();

        let assistant_msg = ConversationMessage {
            uuid: MessageUuid::new(),
            role: Role::Assistant,
            content: content_blocks,
            origin: MessageOrigin::Normal,
            stop_reason,
            usage,
        };

        messages.push(assistant_msg.to_api_message());

        if tx.send(BackendEvent::AssistantMessage(assistant_msg)).is_err() {
            return;
        }

        if tool_uses.is_empty() {
            let _ = tx.send(BackendEvent::TurnDone);
            return;
        }

        for (tool_id, tool_name, tool_input) in &tool_uses {
            let _ = tx.send(BackendEvent::ToolStart { name: tool_name.clone() });

            let (result, is_error) = execute_tool(tool_name, tool_input, &cwd).await;

            let result_msg = ConversationMessage::tool_result(tool_id, &result, is_error);
            messages.push(result_msg.to_api_message());

            if tx.send(BackendEvent::ToolResult(result_msg)).is_err() {
                return;
            }
        }
    }
}
