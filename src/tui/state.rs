//! Application state and typed events between backend and UI.

use std::collections::HashMap;

use ratatui::text::Line;

use crate::types::{
    AppError, ConversationMessage, DeltaText, DeltaThinking, InputBuffer, InputHistory,
    MessageUuid, ModelId, ScrollOffset, ShortModelName, ShortPath, StreamingBuffer, TokenCount,
    ToolName, TurnTimer, WorkingDir,
};

// ─── TUI-local newtypes ─────────────────────────────────────────

/// Implement `Display` and `AsRef<str>` for string newtypes local to the TUI module.
macro_rules! impl_tui_string_newtype {
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

/// Git branch name for status bar display.
#[derive(Debug, Clone)]
pub struct GitBranch(String);

/// Username for status bar display — read once at startup.
#[derive(Debug, Clone)]
pub struct Username(String);

/// Hostname for status bar display — read once at startup.
#[derive(Debug, Clone)]
pub struct Hostname(String);

impl_tui_string_newtype!(GitBranch, Username, Hostname);

// ─── Backend → UI events ────────────────────────────────────────

/// Messages sent from backend task → UI thread.
pub enum BackendEvent {
    StreamDelta(DeltaText),
    ThinkingDelta(DeltaThinking),
    AssistantMessage(ConversationMessage),
    ToolStart { name: ToolName },
    ToolResult(ConversationMessage),
    Error(AppError),
    TurnDone,
}

/// Top-level run state of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RunState {
    Idle,
    Streaming,
    Quitting,
}

// ─── Refresh interval ──────────────────────────────────────────

/// Minimum seconds between git branch refreshes.
#[derive(Debug, Clone, Copy)]
struct RefreshInterval(u64);

impl RefreshInterval {
    const GIT_BRANCH: Self = Self(5);

    fn is_elapsed(self, last: std::time::Instant) -> bool {
        last.elapsed().as_secs() >= self.0
    }
}

// ─── App state ──────────────────────────────────────────────────

/// All mutable application state — owned by the UI thread.
pub struct App {
    pub messages: Vec<ConversationMessage>,
    pub input: InputBuffer,
    pub history: InputHistory,
    pub scroll: ScrollOffset,
    pub total_content_height: crate::types::TermRows,
    pub streaming: StreamingBuffer,
    pub state: RunState,
    pub cwd: WorkingDir,
    pub turn_timer: TurnTimer,
    pub tokens: TokenCount,
    pub git_branch: Option<GitBranch>,
    pub username: Username,
    pub hostname: Hostname,
    pub display_path: ShortPath,
    pub display_model: ShortModelName,
    /// Cache of rendered markdown lines per message UUID.
    /// Only populated for completed messages — streaming content is re-rendered each frame.
    pub markdown_cache: HashMap<MessageUuid, Vec<Line<'static>>>,
    git_branch_updated: std::time::Instant,
}

impl App {
    pub fn new(model: &ModelId) -> Self {
        let cwd = WorkingDir::current();
        let git_branch = read_git_branch(&cwd);
        let display_path = cwd.short_path();
        let display_model = model.short_name();
        Self {
            messages: Vec::new(),
            input: InputBuffer::default(),
            history: InputHistory::default(),
            scroll: ScrollOffset::default(),
            total_content_height: crate::types::TermRows::default(),
            streaming: StreamingBuffer::default(),
            state: RunState::Idle,
            cwd,
            turn_timer: TurnTimer::default(),
            tokens: TokenCount::default(),
            git_branch,
            username: Username(std::env::var("USER").unwrap_or_else(|_| "user".into())),
            hostname: Hostname(
                hostname::get().map_or_else(|_| "host".into(), |h| h.to_string_lossy().to_string()),
            ),
            display_path,
            display_model,
            markdown_cache: HashMap::new(),
            git_branch_updated: std::time::Instant::now(),
        }
    }

    /// Refresh git branch if stale (>5 seconds).
    pub fn refresh_git_branch(&mut self) {
        if RefreshInterval::GIT_BRANCH.is_elapsed(self.git_branch_updated) {
            self.git_branch = read_git_branch(&self.cwd);
            self.git_branch_updated = std::time::Instant::now();
        }
    }

    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.state == RunState::Streaming
    }
}

fn read_git_branch(cwd: &WorkingDir) -> Option<GitBranch> {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd.as_ref())
        .output()
        .ok()
        .and_then(|o| {
            o.status
                .success()
                .then(|| GitBranch(String::from_utf8_lossy(&o.stdout).trim().to_string()))
        })
}
