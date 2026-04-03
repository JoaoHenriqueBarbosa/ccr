//! TUI — chat interface with ratatui + crossterm.
//!
//! Architecture: two async tasks communicating via channels.
//!   - **UI task** (main thread): renders frames, handles keyboard/mouse input
//!   - **Backend task** (spawned): streams API responses, executes tools, loops
//!
//! Split into submodules:
//!   - `state` — `App`, `RunState`, `BackendEvent`
//!   - `render` — typestate render pipeline, draw functions
//!   - `backend` — API streaming, tool execution loop

mod backend;
mod markdown;
mod render;
mod state;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use ratatui::DefaultTerminal;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::api::AnthropicClient;
use crate::tools::get_tool_definitions;
use crate::types::{
    ApiKey, ApiMessage, ContentBlock, ConversationMessage, MessageOrigin, MessageUuid, ModelId,
    Role, ScrollOffset, SystemPrompt, ToolDefinition, TurnTimer,
};

use state::{App, BackendEvent, RunState};

// ─── Entry point ─────────────────────────────────────────────────

pub fn run(api_key: ApiKey, model: ModelId) -> crate::types::Result<()> {
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    let result = run_app(&mut terminal, api_key, model);

    execute!(std::io::stdout(), DisableMouseCapture)?;
    ratatui::restore();

    result
}

fn run_app(
    terminal: &mut DefaultTerminal,
    api_key: ApiKey,
    model: ModelId,
) -> crate::types::Result<()> {
    let mut app = App::new(&model);
    let client = Arc::new(AnthropicClient::new(api_key, model));
    let tools: Arc<[ToolDefinition]> = get_tool_definitions().into();
    let system_prompt = Arc::new(SystemPrompt::new(&app.cwd));

    let mut backend_rx: Option<mpsc::UnboundedReceiver<BackendEvent>> = None;

    loop {
        app.refresh_git_branch();
        terminal.draw(|f| render::draw(f, &mut app))?;

        drain_backend_events(&mut app, &mut backend_rx);

        if !event::poll(std::time::Duration::from_millis(16))? {
            continue;
        }

        match event::read()? {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => app.scroll = app.scroll.scroll_up(3),
                MouseEventKind::ScrollDown => app.scroll = app.scroll.scroll_down(3),
                _ => {}
            },
            Event::Key(key) => {
                handle_key_event(
                    &mut app,
                    key,
                    &mut backend_rx,
                    &client,
                    &system_prompt,
                    &tools,
                );
            }
            _ => {}
        }

        if app.state == RunState::Quitting {
            break;
        }
    }

    Ok(())
}

// ─── Event handling ─────────────────────────────────────────────

/// Drain all pending backend events without blocking.
fn drain_backend_events(
    app: &mut App,
    backend_rx: &mut Option<mpsc::UnboundedReceiver<BackendEvent>>,
) {
    let Some(rx) = backend_rx else { return };

    loop {
        match rx.try_recv() {
            Ok(evt) => match evt {
                BackendEvent::StreamDelta(text) => {
                    app.streaming.push(text.as_ref());
                    app.scroll = ScrollOffset::default();
                }
                BackendEvent::ThinkingDelta(text) => {
                    app.streaming.push(text.as_ref());
                }
                BackendEvent::AssistantMessage(msg) => {
                    app.streaming.clear();
                    if let Some(u) = &msg.usage {
                        app.tokens.add(u);
                    }
                    app.messages.push(msg);
                }
                BackendEvent::ToolStart { name } => {
                    app.streaming.clear();
                    app.streaming.push(&format!("⚡ Running {name}..."));
                }
                BackendEvent::ToolResult(msg) => {
                    app.streaming.clear();
                    app.messages.push(msg);
                }
                BackendEvent::Error(err) => {
                    app.streaming.clear();
                    app.state = RunState::Idle;
                    app.messages.push(ConversationMessage {
                        uuid: MessageUuid::new(),
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text {
                            text: format!("[Error: {err}]").into(),
                            citations: None,
                        }],
                        origin: MessageOrigin::ApiError,
                        stop_reason: None,
                        usage: None,
                    });
                    *backend_rx = None;
                    return;
                }
                BackendEvent::TurnDone => {
                    app.state = RunState::Idle;
                    app.turn_timer = std::mem::take(&mut app.turn_timer).finish();
                    *backend_rx = None;
                    return;
                }
            },
            Err(mpsc::error::TryRecvError::Empty) => return,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.state = RunState::Idle;
                *backend_rx = None;
                return;
            }
        }
    }
}

/// Handle a single key event — input, scrolling, cancel, submit.
fn handle_key_event(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    backend_rx: &mut Option<mpsc::UnboundedReceiver<BackendEvent>>,
    client: &Arc<AnthropicClient>,
    system_prompt: &Arc<SystemPrompt>,
    tools: &Arc<[ToolDefinition]>,
) {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            handle_ctrl_c(app, backend_rx);
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.state = RunState::Quitting;
        }
        KeyCode::Enter => {
            handle_enter(app, backend_rx, client, system_prompt, tools);
        }
        KeyCode::Backspace => app.input.backspace(),
        KeyCode::Left => app.input.move_left(),
        KeyCode::Right => app.input.move_right(),
        KeyCode::Up => app.scroll = app.scroll.scroll_up(3),
        KeyCode::Down => app.scroll = app.scroll.scroll_down(3),
        KeyCode::Char(c) => app.input.insert(c),
        _ => {}
    }
}

/// Handle Ctrl+C — cancel streaming, clear input, or quit.
fn handle_ctrl_c(app: &mut App, backend_rx: &mut Option<mpsc::UnboundedReceiver<BackendEvent>>) {
    if app.is_streaming() {
        app.state = RunState::Idle;
        if !app.streaming.is_empty() {
            app.messages.push(ConversationMessage {
                uuid: MessageUuid::new(),
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: app.streaming.take().into(),
                    citations: None,
                }],
                origin: MessageOrigin::Normal,
                stop_reason: Some("interrupted".into()),
                usage: None,
            });
        }
        *backend_rx = None;
    } else if app.input.is_empty() {
        app.state = RunState::Quitting;
    } else {
        app.input.clear();
    }
}

/// Handle Enter — submit user message and start backend turn.
fn handle_enter(
    app: &mut App,
    backend_rx: &mut Option<mpsc::UnboundedReceiver<BackendEvent>>,
    client: &Arc<AnthropicClient>,
    system_prompt: &Arc<SystemPrompt>,
    tools: &Arc<[ToolDefinition]>,
) {
    if app.input.is_empty() || app.is_streaming() {
        return;
    }

    let user_text = app.input.take();
    app.scroll = ScrollOffset::default();
    app.messages
        .push(ConversationMessage::user_text(&user_text));
    app.state = RunState::Streaming;
    app.streaming.clear();
    app.turn_timer = TurnTimer::start();

    let (tx, rx) = mpsc::unbounded_channel();
    *backend_rx = Some(rx);

    let msgs: Vec<ApiMessage> = app
        .messages
        .iter()
        .map(ConversationMessage::to_api_message)
        .collect();
    let c = Arc::clone(client);
    let sys = Arc::clone(system_prompt);
    let tls = Arc::clone(tools);
    let cwd = app.cwd.clone();

    tokio::spawn(async move {
        backend::backend_turn(c, msgs, sys, tls, cwd, tx).await;
    });
}
