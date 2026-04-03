//! Tool definitions, result status, and typed tool dispatch.

use serde::{Deserialize, Serialize};

use super::newtypes::ToolName;

/// Tool description for the API tool definition.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ToolDescription(pub(super) String);

impl From<&str> for ToolDescription {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Whether a tool execution succeeded or failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum ToolResultStatus {
    #[default]
    Success,
    Error,
}

impl ToolResultStatus {
    #[must_use]
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

impl Serialize for ToolResultStatus {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_bool(self.is_error())
    }
}

impl<'de> Deserialize<'de> for ToolResultStatus {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b { Self::Error } else { Self::Success })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: ToolName,
    pub description: ToolDescription,
    pub input_schema: serde_json::Value,
}

/// Enum of all built-in tools — exhaustive match, no string dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum BuiltinTool {
    Bash,
    Read,
    Write,
    Edit,
    Glob,
    Grep,
    WebFetch,
}

impl BuiltinTool {
    /// Try to parse a `ToolName` into a known built-in tool.
    #[must_use]
    pub fn from_name(name: &ToolName) -> Option<Self> {
        match name.as_ref() {
            "Bash" => Some(Self::Bash),
            "Read" => Some(Self::Read),
            "Write" => Some(Self::Write),
            "Edit" => Some(Self::Edit),
            "Glob" => Some(Self::Glob),
            "Grep" => Some(Self::Grep),
            "WebFetch" => Some(Self::WebFetch),
            _ => None,
        }
    }
}

// ─── Tool input newtypes ───────────────────────────────────────

/// Shell command text for the Bash tool.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct CommandText(pub(super) String);

/// Optional description of what a bash command does (purely informational).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct CommandDescription(pub(super) String);

/// Whether a command runs in the foreground or background.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum ExecutionMode {
    #[default]
    Foreground,
    Background,
}

impl ExecutionMode {
    #[must_use]
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Background)
    }
}

impl<'de> Deserialize<'de> for ExecutionMode {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b {
            Self::Background
        } else {
            Self::Foreground
        })
    }
}

/// The user's login shell, detected from `$SHELL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum UserShell {
    Bash,
    Zsh,
}

impl UserShell {
    /// Detect the user's shell from the `SHELL` environment variable.
    /// Defaults to `Bash` if unset or unrecognized.
    pub fn from_env() -> Self {
        std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                if s.ends_with("/zsh") || s.ends_with("/zsh5") {
                    Some(Self::Zsh)
                } else {
                    None
                }
            })
            .unwrap_or(Self::Bash)
    }

    /// The shell binary name.
    #[must_use]
    pub fn program(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
        }
    }
}

/// Timeout in milliseconds for tool execution.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct TimeoutMs(u64);

impl TimeoutMs {
    /// Maximum allowed timeout (10 minutes).
    pub const MAX: Self = Self(600_000);
    /// Default timeout (2 minutes).
    pub const DEFAULT: Self = Self(120_000);

    pub fn clamped(self) -> Self {
        Self(self.0.min(Self::MAX.0))
    }

    #[must_use]
    pub fn as_millis(self) -> u64 {
        self.0
    }
}

/// File path provided as tool input.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct FilePath(pub(super) String);

/// Line offset for file reading (1-based input, internally 0-based).
///
/// The API accepts 1-based offsets (line 1 = first line). An input of 0
/// is treated as "start from the beginning" (same as omitting the field).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct LineOffset(usize);

impl LineOffset {
    /// Convert 1-based API input to 0-based internal index.
    /// Input of 0 is treated as "start from beginning".
    #[must_use]
    pub fn value(self) -> usize {
        self.0.saturating_sub(1)
    }

    /// The raw 1-based value as provided by the caller.
    #[must_use]
    pub fn raw(self) -> usize {
        self.0
    }
}

/// Maximum number of lines to read.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct LineLimit(usize);

impl LineLimit {
    /// Default limit when not specified.
    pub const DEFAULT: Self = Self(2000);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// File content text for the Write tool.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct FileContent(pub(super) String);

/// Glob pattern for file matching.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct GlobPattern(pub(super) String);

/// Regex pattern for text search.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct SearchPattern(pub(super) String);

/// Search path for glob/grep tools.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct SearchPath(pub(super) String);

/// Glob filter for grep tool.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct GlobFilter(pub(super) String);

/// The text to find and replace (`old_string` in Edit tool).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct OldString(pub(super) String);

impl OldString {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The replacement text (`new_string` in Edit tool).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct NewString(pub(super) String);

/// Whether to replace all occurrences or just the first unique match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum ReplaceMode {
    /// Replace only the first (unique) match.
    #[default]
    First,
    /// Replace all occurrences.
    All,
}

impl ReplaceMode {
    #[must_use]
    pub fn enabled(self) -> bool {
        matches!(self, Self::All)
    }
}

impl<'de> Deserialize<'de> for ReplaceMode {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b { Self::All } else { Self::First })
    }
}

/// Maximum file size allowed for reading (default 10 MB).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxReadFileSize(u64);

impl MaxReadFileSize {
    /// 10 MB default limit.
    pub const DEFAULT: Self = Self(10_485_760);

    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// PDF page range (e.g. `"1-5"`, `"3"`, `"10-20"`).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct PdfPages(pub(super) String);

// ─── Typed tool inputs ─────────────────────────────────────────

/// Typed input for the Bash tool.
#[derive(Debug, Deserialize)]
pub struct BashInput {
    pub command: CommandText,
    pub timeout: Option<TimeoutMs>,
    #[allow(
        dead_code,
        reason = "informational field for UI display, not used in execution"
    )]
    pub description: Option<CommandDescription>,
    #[serde(default)]
    pub run_in_background: Option<ExecutionMode>,
}

/// Typed input for the Read tool.
#[derive(Debug, Deserialize)]
pub struct ReadInput {
    pub file_path: FilePath,
    pub offset: Option<LineOffset>,
    pub limit: Option<LineLimit>,
    pub pages: Option<PdfPages>,
}

/// Typed input for the Write tool.
#[derive(Debug, Deserialize)]
pub struct WriteInput {
    pub file_path: FilePath,
    pub content: FileContent,
}

/// Maximum number of glob results to return.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct GlobResultLimit(usize);

impl GlobResultLimit {
    /// Default limit when not specified.
    pub const DEFAULT: Self = Self(100);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Offset into glob results for pagination.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct GlobResultOffset(usize);

impl GlobResultOffset {
    /// Default offset (start from beginning).
    pub const DEFAULT: Self = Self(0);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Typed input for the Glob tool.
#[derive(Debug, Deserialize)]
pub struct GlobInput {
    pub pattern: GlobPattern,
    pub path: Option<SearchPath>,
    pub limit: Option<GlobResultLimit>,
    pub offset: Option<GlobResultOffset>,
}

/// Output mode for the Grep tool — determines `rg` flags and output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum GrepOutputMode {
    /// Show matching lines with line numbers (`rg -n`).
    Content,
    /// Show only file paths with matches (`rg -l`).
    #[default]
    FilesWithMatches,
    /// Show per-file match counts (`rg -c`).
    Count,
}

impl<'de> Deserialize<'de> for GrepOutputMode {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "content" => Ok(Self::Content),
            "files_with_matches" => Ok(Self::FilesWithMatches),
            "count" => Ok(Self::Count),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["content", "files_with_matches", "count"],
            )),
        }
    }
}

/// Number of context lines for `rg -A`, `-B`, or `-C`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct ContextLines(u16);

impl ContextLines {
    #[must_use]
    pub fn value(self) -> u16 {
        self.0
    }
}

/// Case sensitivity for search (`rg -i`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum CaseSensitivity {
    #[default]
    Sensitive,
    Insensitive,
}

impl CaseSensitivity {
    #[must_use]
    pub fn enabled(self) -> bool {
        matches!(self, Self::Insensitive)
    }
}

impl<'de> Deserialize<'de> for CaseSensitivity {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b {
            Self::Insensitive
        } else {
            Self::Sensitive
        })
    }
}

/// Whether to show line numbers in grep output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum LineNumberDisplay {
    Hide,
    Show,
}

impl LineNumberDisplay {
    /// Default: line numbers are shown.
    pub const DEFAULT: Self = Self::Show;

    #[must_use]
    pub fn enabled(self) -> bool {
        matches!(self, Self::Show)
    }
}

impl<'de> Deserialize<'de> for LineNumberDisplay {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b { Self::Show } else { Self::Hide })
    }
}

/// File type filter for `rg --type TYPE` (e.g. `"js"`, `"py"`, `"rust"`).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct RgFileType(pub(super) String);

/// Single-line or multiline search mode (`rg -U --multiline-dotall`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub enum MultilineSearch {
    #[default]
    SingleLine,
    Multiline,
}

impl MultilineSearch {
    #[must_use]
    pub fn enabled(self) -> bool {
        matches!(self, Self::Multiline)
    }
}

impl<'de> Deserialize<'de> for MultilineSearch {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let b = bool::deserialize(deserializer)?;
        Ok(if b { Self::Multiline } else { Self::SingleLine })
    }
}

/// Maximum number of output entries to return after `offset`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct HeadLimit(u32);

impl HeadLimit {
    /// Default limit when not specified.
    pub const DEFAULT: Self = Self(250);

    #[must_use]
    pub fn value(self) -> u32 {
        self.0
    }
}

/// Number of entries to skip before applying `HeadLimit`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct ResultOffset(u32);

impl ResultOffset {
    /// Default offset (start from beginning).
    pub const DEFAULT: Self = Self(0);

    #[must_use]
    pub fn value(self) -> u32 {
        self.0
    }
}

/// Typed input for the Grep tool.
#[derive(Debug, Deserialize)]
pub struct GrepInput {
    pub pattern: SearchPattern,
    pub path: Option<SearchPath>,
    pub glob: Option<GlobFilter>,
    pub output_mode: Option<GrepOutputMode>,
    #[serde(alias = "-A")]
    pub context_after: Option<ContextLines>,
    #[serde(alias = "-B")]
    pub context_before: Option<ContextLines>,
    #[serde(alias = "-C")]
    pub context: Option<ContextLines>,
    #[serde(alias = "-i")]
    pub case_insensitive: Option<CaseSensitivity>,
    #[serde(alias = "-n")]
    pub line_numbers: Option<LineNumberDisplay>,
    #[serde(rename = "type")]
    pub file_type: Option<RgFileType>,
    pub multiline: Option<MultilineSearch>,
    pub head_limit: Option<HeadLimit>,
    pub offset: Option<ResultOffset>,
}

/// Typed input for the Edit tool.
#[derive(Debug, Deserialize)]
pub struct EditInput {
    pub file_path: FilePath,
    pub old_string: OldString,
    pub new_string: NewString,
    #[serde(default)]
    pub replace_all: ReplaceMode,
}

// ─── Bash output control newtypes ─────────────────────────────

/// Maximum character length for command output before truncation.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxOutputLen(usize);

impl MaxOutputLen {
    /// Default maximum output length (80,000 chars).
    pub const DEFAULT: Self = Self(80_000);

    /// Construct from a raw value (useful for testing custom limits).
    #[cfg(test)]
    pub fn from_value(v: usize) -> Self {
        Self(v)
    }

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Character threshold above which output is persisted to disk.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct LargeOutputThreshold(usize);

impl LargeOutputThreshold {
    /// Default threshold (30,000 chars).
    pub const DEFAULT: Self = Self(30_000);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Number of bytes for the large-output preview snippet.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct PreviewLen(usize);

impl PreviewLen {
    /// Default preview length (2048 chars).
    pub const DEFAULT: Self = Self(2048);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Exit code from a completed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ExitCode(i32);

impl ExitCode {
    pub fn new(code: i32) -> Self {
        Self(code)
    }

    #[must_use]
    pub fn value(self) -> i32 {
        self.0
    }

    #[must_use]
    pub fn is_success(self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── WebFetch newtypes ──────────────────────────────────────────

/// URL to fetch content from.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct FetchUrl(pub(super) String);

/// Prompt describing what information to extract from fetched content.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct FetchPrompt(pub(super) String);

/// Maximum allowed URL length (2000 chars, matching TS).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxUrlLength(usize);

impl MaxUrlLength {
    pub const DEFAULT: Self = Self(2000);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Maximum markdown content length before truncation (100K chars).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxMarkdownLength(usize);

impl MaxMarkdownLength {
    pub const DEFAULT: Self = Self(100_000);

    #[must_use]
    pub fn value(self) -> usize {
        self.0
    }
}

/// Maximum HTTP response body size (10 MB).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxHttpContentLength(u64);

impl MaxHttpContentLength {
    pub const DEFAULT: Self = Self(10 * 1024 * 1024);

    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Timeout for HTTP fetch requests (60 seconds).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct FetchTimeoutSecs(u64);

impl FetchTimeoutSecs {
    pub const DEFAULT: Self = Self(60);

    #[must_use]
    pub fn as_secs(self) -> u64 {
        self.0
    }
}

/// Typed input for the `WebFetch` tool.
#[derive(Debug, Deserialize)]
pub struct WebFetchInput {
    pub url: FetchUrl,
    pub prompt: FetchPrompt,
}

// ─── Write tool encoding types ──────────────────────────────────

/// Detected encoding of an existing file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum FileEncoding {
    /// Standard UTF-8.
    Utf8,
    /// UTF-16 Little-Endian with BOM.
    Utf16Le,
}

/// Detected line ending style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum LineEndings {
    /// Unix-style `\n`.
    Lf,
    /// Windows-style `\r\n`.
    CrLf,
}

/// Maximum allowed file size for write operations (default 1 GiB).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct MaxWriteFileSize(usize);

impl MaxWriteFileSize {
    /// 1 GiB — same as the TS implementation.
    pub const DEFAULT: Self = Self(1_073_741_824);

    #[must_use]
    pub fn as_bytes(self) -> usize {
        self.0
    }
}

/// Human-readable file size in bytes, used in error messages.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct FileSizeBytes(usize);

impl FileSizeBytes {
    /// Wrap a raw byte count.
    pub fn new(bytes: usize) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Display for FileSizeBytes {
    #[allow(
        clippy::cast_precision_loss,
        reason = "display-only formatting; exact precision not needed"
    )]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const KIB: usize = 1024;
        const MIB: usize = 1024 * KIB;
        const GIB: usize = 1024 * MIB;

        if self.0 >= GIB {
            write!(f, "{:.1} GiB", self.0 as f64 / GIB as f64)
        } else if self.0 >= MIB {
            write!(f, "{:.1} MiB", self.0 as f64 / MIB as f64)
        } else if self.0 >= KIB {
            write!(f, "{:.1} KiB", self.0 as f64 / KIB as f64)
        } else {
            write!(f, "{} bytes", self.0)
        }
    }
}
