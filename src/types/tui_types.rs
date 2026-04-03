//! TUI-specific types — terminal rendering, input, timing.

use std::fmt;

use super::api::Usage;

/// Height or offset measured in terminal rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[must_use]
pub struct TermRows(u16);

impl TermRows {
    #[must_use]
    pub fn as_u16(self) -> u16 {
        self.0
    }

    #[must_use]
    pub fn as_i32(self) -> i32 {
        i32::from(self.0)
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub fn min(self, rhs: Self) -> Self {
        Self(self.0.min(rhs.0))
    }
}

impl std::ops::Add for TermRows {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

// NOTE: Sub intentionally removed — use saturating_sub() for safe arithmetic.

impl std::iter::Sum for TermRows {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self(0), |acc, x| acc + x)
    }
}

impl From<u16> for TermRows {
    fn from(v: u16) -> Self {
        Self(v)
    }
}

/// Width or offset measured in terminal columns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[must_use]
pub struct TermCols(u16);

impl TermCols {
    #[must_use]
    pub fn as_u16(self) -> u16 {
        self.0
    }
}

impl From<u16> for TermCols {
    fn from(v: u16) -> Self {
        Self(v)
    }
}

/// Token count — wraps u64 with display formatting.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct TokenCount(u64);

impl TokenCount {
    pub fn add(&mut self, usage: &Usage) {
        self.0 += usage.input_tokens.value() + usage.output_tokens.value();
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[allow(clippy::cast_precision_loss)]
impl fmt::Display for TokenCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 >= 1_000_000 {
            write!(f, "{:.1}M tokens", self.0 as f64 / 1_000_000.0)
        } else {
            write!(f, "{}k tokens", self.0 / 1000)
        }
    }
}

/// Scroll offset in terminal rows.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct ScrollOffset(u16);

impl ScrollOffset {
    #[must_use]
    pub fn value(self) -> u16 {
        self.0
    }

    pub fn scroll_up(self, lines: u16) -> Self {
        Self(self.0.saturating_add(lines))
    }

    pub fn scroll_down(self, lines: u16) -> Self {
        Self(self.0.saturating_sub(lines))
    }
}

/// Display path with `~` for home (e.g. `~/projects/foo`).
#[derive(Debug, Clone)]
#[must_use]
pub struct ShortPath(pub(super) String);

/// Working directory — absolute path, threaded through tools and TUI.
#[derive(Debug, Clone)]
pub struct WorkingDir(pub(super) String);

impl WorkingDir {
    #[must_use]
    pub fn current() -> Self {
        Self(
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        )
    }

    /// Display path with ~ for home directory.
    pub fn short_path(&self) -> ShortPath {
        if let Some(home) = dirs::home_dir() {
            let home_str = home.to_string_lossy();
            if self.0.starts_with(home_str.as_ref()) {
                return ShortPath(self.0.replacen(home_str.as_ref(), "~", 1));
            }
        }
        ShortPath(self.0.clone())
    }

    /// Validate that a given path resolves inside this working directory.
    /// Returns the canonical path on success.
    pub fn validate_path(&self, file_path: &str) -> crate::types::Result<String> {
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            std::path::PathBuf::from(&self.0).join(file_path)
        };

        // Resolve the path as much as possible (parent dirs must exist for canonicalize).
        // For write operations the file might not exist yet, so we canonicalize the parent.
        let resolved = if path.exists() {
            path.canonicalize().unwrap_or_else(|_| path.clone())
        } else if let Some(parent) = path.parent() {
            if parent.exists() {
                let canon_parent = parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf());
                canon_parent.join(path.file_name().unwrap_or_default())
            } else {
                path.clone()
            }
        } else {
            path.clone()
        };

        let resolved_str = resolved.to_string_lossy().to_string();
        let cwd_canonical = std::path::Path::new(&self.0)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&self.0));
        let cwd_str = cwd_canonical.to_string_lossy();

        // Allow paths inside cwd OR in standard temp directories
        if resolved_str.starts_with(cwd_str.as_ref())
            || resolved_str.starts_with("/tmp")
            || is_safe_dev_path(&resolved_str)
        {
            Ok(resolved_str)
        } else {
            Err(super::error::AppError::PathTraversal {
                attempted: resolved_str,
            })
        }
    }
}

/// Device paths that are safe for tools to read/write.
const SAFE_DEV_PATHS: &[&str] = &["/dev/null", "/dev/stderr", "/dev/stdout"];

/// Check if a `/dev/` path is in the safe allowlist.
fn is_safe_dev_path(path: &str) -> bool {
    SAFE_DEV_PATHS.contains(&path)
}

/// System prompt sent as the second block (after billing header).
#[derive(Debug, Clone)]
#[must_use]
pub struct SystemPrompt(pub(super) String);

impl SystemPrompt {
    pub fn new(cwd: &WorkingDir) -> Self {
        Self(format!(
            "You are Claude Code, an AI assistant. Working directory: {cwd}",
        ))
    }
}

/// Raw text content of the input line.
#[derive(Debug, Clone, Default)]
struct InputText(String);

/// Byte offset into an `InputText` — always at a valid UTF-8 char boundary.
#[derive(Debug, Clone, Copy, Default)]
struct ByteOffset(usize);

/// Text input buffer with UTF-8-safe cursor navigation.
#[derive(Debug, Clone, Default)]
pub struct InputBuffer {
    text: InputText,
    cursor: ByteOffset,
}

impl InputBuffer {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.0.is_empty()
    }

    /// Insert a char at cursor, advance cursor past it.
    pub fn insert(&mut self, c: char) {
        self.text.0.insert(self.cursor.0, c);
        self.cursor.0 += c.len_utf8();
    }

    /// Insert a string at cursor (e.g. from paste). Advances cursor past it.
    pub fn insert_str(&mut self, s: &str) {
        self.text.0.insert_str(self.cursor.0, s);
        self.cursor.0 += s.len();
    }

    /// Delete the char before cursor.
    pub fn backspace(&mut self) {
        if self.cursor.0 > 0 {
            let prev = self.text.0[..self.cursor.0]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            self.text.0.drain(prev..self.cursor.0);
            self.cursor.0 = prev;
        }
    }

    /// Move cursor one char left.
    pub fn move_left(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 = self.text.0[..self.cursor.0]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    /// Move cursor one char right.
    pub fn move_right(&mut self) {
        if self.cursor.0 < self.text.0.len() {
            self.cursor.0 = self.text.0[self.cursor.0..]
                .char_indices()
                .nth(1)
                .map_or(self.text.0.len(), |(i, _)| self.cursor.0 + i);
        }
    }

    /// Take the text out and reset cursor. Returns the text.
    #[must_use]
    pub fn take(&mut self) -> String {
        self.cursor = ByteOffset(0);
        std::mem::take(&mut self.text.0)
    }

    /// Clear without returning.
    pub fn clear(&mut self) {
        self.text.0.clear();
        self.cursor = ByteOffset(0);
    }

    /// Char count up to cursor (for display positioning — column offset).
    #[allow(clippy::cast_possible_truncation)]
    pub fn display_cursor_offset(&self) -> TermCols {
        TermCols(self.text.0[..self.cursor.0].chars().count() as u16)
    }

    /// Replace the entire buffer contents, placing cursor at end.
    pub fn set_text(&mut self, s: &str) {
        self.text.0.clear();
        self.text.0.push_str(s);
        self.cursor.0 = self.text.0.len();
    }
}

/// History index into `InputHistory` entries.
#[derive(Debug, Clone, Copy, Default)]
struct HistoryIndex(usize);

/// Input history — stores previous submissions for Up/Down navigation.
#[derive(Debug, Clone, Default)]
pub struct InputHistory {
    entries: Vec<String>,
    /// Current position in history. `entries.len()` means "new input" (not browsing).
    pos: HistoryIndex,
    /// Stash of the current (unsent) input when user starts browsing history.
    stash: String,
}

impl InputHistory {
    /// Record a submitted input line.
    pub fn push(&mut self, text: String) {
        if !text.is_empty() {
            // Avoid consecutive duplicates.
            if self.entries.last() != Some(&text) {
                self.entries.push(text);
            }
        }
        self.pos.0 = self.entries.len();
        self.stash.clear();
    }

    /// Navigate up (older). Returns the text to display, if changed.
    #[must_use]
    pub fn up(&mut self, current_input: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.pos.0 == self.entries.len() {
            // Entering history — stash current input.
            current_input.clone_into(&mut self.stash);
        }
        if self.pos.0 > 0 {
            self.pos.0 -= 1;
            Some(&self.entries[self.pos.0])
        } else {
            None
        }
    }

    /// Navigate down (newer). Returns the text to display, if changed.
    #[must_use]
    pub fn down(&mut self) -> Option<&str> {
        if self.pos.0 >= self.entries.len() {
            return None;
        }
        self.pos.0 += 1;
        if self.pos.0 == self.entries.len() {
            Some(&self.stash)
        } else {
            Some(&self.entries[self.pos.0])
        }
    }
}

// ─── TurnTimer — real typestate with separate structs ──────────

/// Timer in idle state — no turn in progress.
#[derive(Debug, Clone, Default)]
#[allow(dead_code, reason = "typestate marker — documents the Idle phase")]
pub struct IdleTimer;

/// Timer actively timing a backend turn.
#[derive(Debug, Clone)]
pub struct TimingTimer(std::time::Instant);

/// Timer for a completed turn — records final duration.
#[derive(Debug, Clone)]
pub struct CompletedTimer(std::time::Duration);

/// Typestate turn timer — wraps the three phases in an enum for storage
/// while the real transitions are enforced by `start()` → `TimingTimer`
/// and `finish()` → `CompletedTimer`.
///
/// Transitions: `Idle → start() → Timing → finish() → Completed`
#[derive(Debug, Clone, Default)]
pub enum TurnTimer {
    #[default]
    Idle,
    Timing(TimingTimer),
    Completed(CompletedTimer),
}

impl TimingTimer {
    #[must_use]
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }

    /// Milliseconds since the turn started.
    #[must_use]
    pub fn elapsed_ms(&self) -> u128 {
        self.0.elapsed().as_millis()
    }

    /// Transition `Timing → Completed`. Consumes self — can't reuse.
    #[must_use]
    pub fn finish(self) -> CompletedTimer {
        CompletedTimer(self.0.elapsed())
    }
}

impl CompletedTimer {
    /// Duration in seconds.
    #[must_use]
    pub fn as_secs_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }
}

impl TurnTimer {
    /// Start timing. Returns `TurnTimer::Timing`.
    #[must_use]
    pub fn start() -> Self {
        Self::Timing(TimingTimer::new())
    }

    /// Finish timing. Panics in debug if not in `Timing` state.
    #[must_use]
    pub fn finish(self) -> Self {
        match self {
            Self::Timing(t) => Self::Completed(t.finish()),
            other => {
                debug_assert!(false, "finish() called on non-Timing TurnTimer");
                other
            }
        }
    }

    /// Milliseconds since turn started (only while `Timing`).
    #[must_use]
    pub fn elapsed_ms(&self) -> Option<u128> {
        match self {
            Self::Timing(t) => Some(t.elapsed_ms()),
            _ => None,
        }
    }

    /// Completed duration in seconds (only while `Completed`).
    #[must_use]
    pub fn completed_secs(&self) -> Option<f64> {
        match self {
            Self::Completed(c) => Some(c.as_secs_f64()),
            _ => None,
        }
    }
}

/// Buffer for text streaming from API — accumulated token by token.
#[derive(Debug, Clone, Default)]
pub struct StreamingBuffer(pub(super) String);

impl StreamingBuffer {
    pub fn push(&mut self, text: &str) {
        self.0.push_str(text);
    }

    #[must_use]
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.0)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Output text from a tool execution.
#[derive(Debug, Clone)]
#[must_use]
pub struct ToolOutput(String);

impl ToolOutput {
    pub fn new(text: String) -> Self {
        Self(text)
    }
}

impl std::fmt::Display for ToolOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ToolOutput {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for ToolOutput {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ToolOutput {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}
