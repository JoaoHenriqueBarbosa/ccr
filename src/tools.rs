//! Built-in tools — mirrors `BashTool`, `FileReadTool`, `FileWriteTool`, `FileEditTool`.
//!
//! Each tool follows the same pattern as the TS: validate input → execute → return result.
//! Tool dispatch is exhaustive via `BuiltinTool` enum — no string matching.

use std::fmt::Write;

use tokio::process::Command;

use crate::types::{
    BashInput, BuiltinTool, CaseInsensitive, EditInput, ExitCode, FetchTimeoutSecs,
    FileEncoding, FileSizeBytes, GlobInput, GlobResultLimit, GlobResultOffset, GrepInput,
    GrepOutputMode, HeadLimit,
    LargeOutputThreshold, LineLimit, LineOffset, MaxHttpContentLength, MaxMarkdownLength,
    MaxOutputLen, MaxReadFileSize, MaxUrlLength, MultilineMode, PreviewLen, ReadInput,
    LineEndings, MaxWriteFileSize, ResultOffset, RunInBackground, ShowLineNumbers, TimeoutMs,
    ToolDefinition, ToolName, ToolOutput, ToolResultStatus, WebFetchInput, WorkingDir,
    WriteInput,
};

/// Execute a tool by name with given input. Returns `(result_text, status)`.
/// Mirrors the TS `runTools()` in `src/services/tools/toolOrchestration.ts`.
pub async fn execute_tool(
    name: &ToolName,
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let Some(tool) = BuiltinTool::from_name(name) else {
        return (ToolOutput::new(format!("Unknown tool: {name}")), ToolResultStatus::Error);
    };

    match tool {
        BuiltinTool::Bash => execute_bash(input, cwd).await,
        BuiltinTool::Read => execute_read(input, cwd).await,
        BuiltinTool::Write => execute_write(input, cwd).await,
        BuiltinTool::Edit => execute_edit(input, cwd).await,
        BuiltinTool::Glob => execute_glob(input, cwd).await,
        BuiltinTool::Grep => execute_grep(input, cwd).await,
        BuiltinTool::WebFetch => execute_webfetch(input).await,
    }
}

/// Get tool definitions for the API. Mirrors `src/tools/*/prompt.ts`.
#[must_use]
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        bash_definition(),
        read_definition(),
        write_definition(),
        edit_definition(),
        glob_definition(),
        grep_definition(),
        webfetch_definition(),
    ]
}

fn bash_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Bash".into(),
        description: "Executes a bash command and returns its output.\n\n\
            The working directory persists between commands, but shell state does not.\n\
            Avoid using this for tasks that have dedicated tools (Read, Edit, Grep, Glob).\n\
            Default timeout: 120000ms. Max: 600000ms.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to execute" },
                "timeout": { "type": "number", "description": "Optional timeout in milliseconds (max 600000)" },
                "description": { "type": "string", "description": "Human-readable description of what this command does" },
                "run_in_background": { "type": "boolean", "description": "Run command in background (not yet implemented)" }
            },
            "required": ["command"]
        }),
    }
}

fn read_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Read".into(),
        description: "Reads a file from the local filesystem.\n\n\
            Returns content with line numbers in `line_number\\tcontent` format.\n\
            Default limit is 2000 lines. Use `offset` (1-based) and `limit` to \
            read specific ranges in large files.\n\
            Cannot read directories — use `ls` via the Bash tool instead."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "number",
                    "description": "Line number to start reading from (1-based)"
                },
                "limit": {
                    "type": "number",
                    "description": "Number of lines to read (default 2000)"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range for PDF files (e.g. \"1-5\"). Only for PDFs."
                }
            },
            "required": ["file_path"]
        }),
    }
}

fn write_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Write".into(),
        description: "Writes a file to the local filesystem.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "The absolute path to the file to write" },
                "content": { "type": "string", "description": "The content to write to the file" }
            },
            "required": ["file_path", "content"]
        }),
    }
}

fn edit_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Edit".into(),
        description: "Performs exact string replacements in files.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "The absolute path to the file to modify" },
                "old_string": { "type": "string", "description": "The text to replace" },
                "new_string": { "type": "string", "description": "The text to replace it with (must be different from old_string)" },
                "replace_all": { "type": "boolean", "default": false, "description": "Replace all occurrences of old_string (default false)" }
            },
            "required": ["file_path", "old_string", "new_string"]
        }),
    }
}

fn glob_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Glob".into(),
        description: "Fast file pattern matching tool that works with any codebase size. \
            Supports glob patterns like `**/*.js` or `src/**/*.ts`. \
            Returns matching file paths sorted by modification time. \
            Use this tool when you need to find files by name patterns."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "The glob pattern to match files against" },
                "path": { "type": "string", "description": "The directory to search in. Defaults to cwd if omitted." },
                "limit": { "type": "number", "description": "Maximum number of results to return (default 100)" },
                "offset": { "type": "number", "description": "Number of results to skip for pagination (default 0)" }
            },
            "required": ["pattern"]
        }),
    }
}

fn grep_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Grep".into(),
        description: "A powerful search tool built on ripgrep.\n\n\
            Supports full regex syntax (e.g. `log.*Error`, `function\\s+\\w+`).\n\
            Filter files with `glob` parameter (e.g. `*.js`, `**/*.tsx`) or \
            `type` parameter (e.g. `js`, `py`, `rust`).\n\
            Output modes: `content` shows matching lines, `files_with_matches` shows \
            only file paths (default), `count` shows match counts.\n\
            Pattern syntax: Uses ripgrep (not grep) — literal braces need escaping.\n\
            Multiline matching: By default patterns match within single lines only. \
            For cross-line patterns, use `multiline: true`."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regular expression pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in (rg PATH). Defaults to current working directory."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. `*.js`, `*.{ts,tsx}`) — maps to rg --glob"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output mode: `content` shows matching lines, `files_with_matches` shows file paths (default), `count` shows match counts."
                },
                "-A": {
                    "type": "number",
                    "description": "Number of lines to show after each match (rg -A). Only applies when output_mode is `content`."
                },
                "-B": {
                    "type": "number",
                    "description": "Number of lines to show before each match (rg -B). Only applies when output_mode is `content`."
                },
                "-C": {
                    "type": "number",
                    "description": "Number of lines to show before and after each match (rg -C). Only applies when output_mode is `content`."
                },
                "-i": {
                    "type": "boolean",
                    "description": "Case insensitive search (rg -i)"
                },
                "-n": {
                    "type": "boolean",
                    "description": "Show line numbers in output (rg -n). Defaults to true. Only applies when output_mode is `content`."
                },
                "type": {
                    "type": "string",
                    "description": "File type to search (rg --type). Common types: js, py, rust, go, java, etc."
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline mode where `.` matches newlines and patterns can span lines (rg -U --multiline-dotall). Default: false."
                },
                "head_limit": {
                    "type": "number",
                    "description": "Limit output to first N entries. Defaults to 250 when unspecified. Pass 0 for unlimited."
                },
                "offset": {
                    "type": "number",
                    "description": "Skip first N entries before applying `head_limit`. Defaults to 0."
                }
            },
            "required": ["pattern"]
        }),
    }
}

fn webfetch_definition() -> ToolDefinition {
    ToolDefinition {
        name: "WebFetch".into(),
        description: "Fetches content from a URL and extracts information.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch content from" },
                "prompt": { "type": "string", "description": "What information to extract from the page" }
            },
            "required": ["url", "prompt"]
        }),
    }
}

// ─── Shared utilities ───────────────────────────────────────────

/// Expand `~` to the user's home directory and trim whitespace.
/// Falls back to the original (trimmed) path if home cannot be resolved.
#[must_use]
pub fn expand_path(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            let home_str = home.to_string_lossy();
            return format!("{home_str}{rest}");
        }
    }
    trimmed.to_string()
}

/// Detect the encoding and line-ending style of raw file bytes.
pub fn detect_file_encoding(bytes: &[u8]) -> (FileEncoding, LineEndings) {
    let encoding = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        FileEncoding::Utf16Le
    } else {
        FileEncoding::Utf8
    };
    let has_crlf = bytes.windows(2).any(|w| w == b"\r\n");
    let endings = if has_crlf { LineEndings::CrLf } else { LineEndings::Lf };
    (encoding, endings)
}

/// Re-encode content preserving original encoding and line endings.
/// Input `content` is LF-normalized; CRLF is restored if original used it.
#[must_use]
pub fn encode_for_write(content: &str, encoding: FileEncoding, endings: LineEndings) -> Vec<u8> {
    let with_endings = match endings {
        LineEndings::Lf => content.to_string(),
        LineEndings::CrLf => content.replace('\n', "\r\n"),
    };
    match encoding {
        FileEncoding::Utf8 => with_endings.into_bytes(),
        FileEncoding::Utf16Le => {
            let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
            for code_unit in with_endings.encode_utf16() {
                bytes.extend_from_slice(&code_unit.to_le_bytes());
            }
            bytes
        }
    }
}

// ─── Tool implementations ────────────────────────────────────────

/// Mirrors `src/tools/BashTool/BashTool.ts`.
async fn execute_bash(input: &serde_json::Value, cwd: &WorkingDir) -> (ToolOutput, ToolResultStatus) {
    let parsed: BashInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid Bash input: {e}")), ToolResultStatus::Error),
    };

    // Stub: background execution not yet implemented
    if parsed.run_in_background.is_some_and(RunInBackground::is_enabled) {
        return (
            ToolOutput::new("Background execution not yet implemented".into()),
            ToolResultStatus::Error,
        );
    }

    let timeout = parsed.timeout
        .map_or(TimeoutMs::DEFAULT, TimeoutMs::clamped);

    let shell = crate::types::UserShell::from_env();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(timeout.as_millis()),
        Command::new(shell.program())
            .arg("-l")
            .arg("-c")
            .arg(parsed.command.as_ref())
            .current_dir(cwd.as_ref())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => format_command_output(parsed.command.as_ref(), &output),
        Ok(Err(e)) => (ToolOutput::new(format!("Command execution failed: {e}")), ToolResultStatus::Error),
        Err(_) => (ToolOutput::new(format!("Command timed out after {}ms", timeout.as_millis())), ToolResultStatus::Error),
    }
}

/// Format stdout + stderr from a completed command.
///
/// Applies: empty-line stripping, exit code annotation, command-semantic
/// exit code interpretation, output truncation, and large output persistence.
fn format_command_output(
    command: &str,
    output: &std::process::Output,
) -> (ToolOutput, ToolResultStatus) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = String::new();

    match (stdout.is_empty(), stderr.is_empty()) {
        (false, true) => result.push_str(&stdout),
        (true, false) => result.push_str(&stderr),
        (false, false) => {
            result.push_str(&stdout);
            result.push_str("\nstderr:\n");
            result.push_str(&stderr);
        }
        (true, true) => {}
    }

    // Strip leading and trailing blank lines.
    result = strip_blank_lines(&result);

    let exit_code = ExitCode::new(output.status.code().unwrap_or(-1));
    let is_semantic_success = is_benign_exit(command, exit_code);

    // Append exit code for non-zero exits.
    if !exit_code.is_success() {
        let _ = write!(result, "\nExit code: {exit_code}");
    }

    if result.is_empty() {
        result = "(no output)".into();
    }

    // Truncate oversized output.
    result = truncate_output(&result, MaxOutputLen::DEFAULT);

    // Persist to disk if still large.
    result = persist_large_output(&result, LargeOutputThreshold::DEFAULT);

    let status = if exit_code.is_success() || is_semantic_success {
        ToolResultStatus::Success
    } else {
        ToolResultStatus::Error
    };

    (ToolOutput::new(result), status)
}

/// Strip leading and trailing blank lines from output, preserving internal content.
#[must_use]
fn strip_blank_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.trim().is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// Extract the base command name from a shell command string.
///
/// Handles pipelines by using the last command. Strips `sudo`, env vars,
/// and path prefixes.
#[must_use]
fn extract_base_command(command: &str) -> &str {
    // Use the last command in a pipeline.
    let segment = command.rsplit('|').next().unwrap_or(command).trim();

    // Skip leading env vars (FOO=bar) and sudo.
    let token = segment
        .split_whitespace()
        .find(|tok| !tok.contains('=') && *tok != "sudo")
        .unwrap_or("");

    // Strip path prefix: `/usr/bin/grep` -> `grep`
    token.rsplit('/').next().unwrap_or(token)
}

/// Determine whether a non-zero exit code is benign for the given command.
///
/// - `grep` exit 1 = no matches (not an error)
/// - `diff` exit 1 = differences found (not an error)
/// - `find` exit 1 = partial results (not an error)
#[must_use]
fn is_benign_exit(command: &str, code: ExitCode) -> bool {
    if code.is_success() {
        return true;
    }
    let base = extract_base_command(command);
    match base {
        "grep" | "egrep" | "fgrep" | "rg" | "ag"
        | "diff" | "colordiff"
        | "find" | "fd" => code.value() == 1,
        _ => false,
    }
}

/// Truncate output to `max_len` chars at a line boundary.
///
/// If truncated, appends a note with the number of lines removed.
#[must_use]
fn truncate_output(output: &str, max_len: MaxOutputLen) -> String {
    if output.len() <= max_len.value() {
        return output.to_string();
    }

    // Find last newline within the limit to avoid cutting mid-line.
    let cut = output[..max_len.value()]
        .rfind('\n')
        .map_or(max_len.value(), |pos| pos + 1);

    let kept = &output[..cut];
    let removed_lines = output[cut..].lines().count();

    format!("{kept}\n\n... [{removed_lines} lines truncated] ...")
}

/// If output exceeds the large-output threshold, persist to disk and return
/// a summary with a preview.
#[must_use]
fn persist_large_output(output: &str, threshold: LargeOutputThreshold) -> String {
    if output.len() <= threshold.value() {
        return output.to_string();
    }

    let dir = "/tmp/claude-tool-results";
    // Best-effort directory creation (synchronous — tiny I/O).
    let _ = std::fs::create_dir_all(dir);

    let id = uuid::Uuid::new_v4();
    let path = format!("{dir}/{id}.txt");

    if std::fs::write(&path, output).is_err() {
        // If we cannot persist, return the output as-is.
        return output.to_string();
    }

    let preview_end = PreviewLen::DEFAULT.value().min(output.len());
    // Snap to char boundary.
    let preview_end = floor_char_boundary(output, preview_end);
    let preview = &output[..preview_end];

    format!(
        "Output too large ({} bytes). Full output saved to: {path}\n\n\
         Preview (first 2KB):\n{preview}",
        output.len(),
    )
}

/// Find the largest byte index <= `i` that is a valid char boundary.
///
/// Equivalent to `str::floor_char_boundary` (nightly). Provided here for
/// stable Rust compatibility.
#[must_use]
fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut pos = i;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Mirrors `src/tools/FileReadTool/FileReadTool.ts`.
async fn execute_read(
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: ReadInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return read_err(format!("Invalid Read input: {e}")),
    };

    // PDF pages stub
    if parsed.pages.is_some() {
        return read_err("PDF reading not yet implemented.".into());
    }

    let path = match cwd.validate_path(parsed.file_path.as_ref()) {
        Ok(p) => p,
        Err(e) => return read_err(format!("{e}")),
    };

    // Blocked device/special paths
    if let Some(msg) = check_blocked_path(&path) {
        return read_err(msg);
    }

    // Binary extension check
    if let Some(msg) = check_binary_extension(&path) {
        return read_err(msg);
    }

    // File size guard
    if let Err(msg) = check_file_size(&path).await {
        if !msg.is_empty() {
            return read_err(msg);
        }
    }

    // Binary content detection (first 8192 bytes)
    if let Some(msg) = check_binary_content(&path).await {
        return read_err(msg);
    }

    // Read file content
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return read_err(file_not_found_message(&path, cwd));
        }
        Err(e) => return read_err(format!("Error reading {path}: {e}")),
    };

    format_read_output(&content, &parsed, &path)
}

/// Format the read output with line numbers, handling empty/offset-past-end cases.
fn format_read_output(
    content: &str,
    parsed: &ReadInput,
    path: &str,
) -> (ToolOutput, ToolResultStatus) {
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() && content.is_empty() {
        return (
            ToolOutput::new(
                "Warning: the file exists but the contents are empty.".into(),
            ),
            ToolResultStatus::Success,
        );
    }

    let offset = parsed.offset.map_or(0, LineOffset::value);
    let limit = parsed
        .limit
        .map_or(LineLimit::DEFAULT.value(), LineLimit::value);

    if offset >= lines.len() {
        let raw_offset = parsed.offset.map_or(0, LineOffset::raw);
        return (
            ToolOutput::new(format!(
                "The file has {} lines, but the offset is {raw_offset}. \
                 The file is shorter than the provided offset.",
                lines.len(),
            )),
            ToolResultStatus::Error,
        );
    }

    let start = offset;
    let end = (start + limit).min(lines.len());

    let mut result = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let _ = writeln!(result, "{}\t{line}", start + i + 1);
    }

    if result.is_empty() {
        let _ = write!(result, "Warning: no lines to display from {path}.");
    }

    (ToolOutput::new(result), ToolResultStatus::Success)
}

/// Shorthand for returning an error from `execute_read`.
fn read_err(msg: String) -> (ToolOutput, ToolResultStatus) {
    (ToolOutput::new(msg), ToolResultStatus::Error)
}

// ─── Read tool guards ──────────────────────────────────────────

/// Known binary file extensions that cannot be read as text.
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "mp3",
    "mp4", "avi", "mov", "mkv", "wav", "flac", "ogg", "pdf", "zip",
    "tar", "gz", "bz2", "xz", "7z", "rar", "exe", "dll", "so",
    "dylib", "o", "a", "class", "pyc", "wasm", "bin", "dat", "db",
    "sqlite", "sqlite3",
];

/// Dangerous device/special paths that could hang or leak data.
const BLOCKED_DEVICE_PATHS: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/stdin",
    "/dev/tty",
    "/dev/null",
];

/// Check if the path matches a blocked device or special file.
fn check_blocked_path(path: &str) -> Option<String> {
    if BLOCKED_DEVICE_PATHS.contains(&path) {
        return Some(format!("Cannot read device/special file: {path}"));
    }
    if path.starts_with("/proc/") && path.contains("/fd/") {
        return Some(format!("Cannot read device/special file: {path}"));
    }
    None
}

/// Check file extension against the binary blocklist.
fn check_binary_extension(path: &str) -> Option<String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)?
        .to_ascii_lowercase();

    if BINARY_EXTENSIONS.contains(&ext.as_str()) {
        Some(format!(
            "Cannot read binary file ({ext}). Use the Bash tool to inspect it."
        ))
    } else {
        None
    }
}

/// Check file size against `MaxReadFileSize::DEFAULT`.
async fn check_file_size(path: &str) -> std::result::Result<(), String> {
    let metadata = tokio::fs::metadata(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            String::new()
        } else {
            format!("Error reading file metadata: {e}")
        }
    })?;

    let size = metadata.len();
    let max = MaxReadFileSize::DEFAULT.value();
    if size > max {
        Err(format!(
            "File is too large to read: {} (max {}).",
            format_file_size(size),
            format_file_size(max),
        ))
    } else {
        Ok(())
    }
}

/// Format a byte count as human-readable size (KB/MB/GB).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "file sizes fit comfortably in f64"
)]
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Detect binary content by checking the first 8192 bytes for null bytes.
async fn check_binary_content(path: &str) -> Option<String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut buf = vec![0u8; 8192];
    let n = file.read(&mut buf).await.ok()?;

    if buf[..n].contains(&0) {
        Some(
            "Cannot read binary file (detected null bytes). \
             Use the Bash tool to inspect it."
                .into(),
        )
    } else {
        None
    }
}

/// Build "file not found" message with suggestions and CWD.
fn file_not_found_message(path: &str, cwd: &WorkingDir) -> String {
    let mut msg = format!(
        "File not found: {path}. The current working directory is {cwd}.",
    );
    if let Some(suggestion) = find_similar_file(path) {
        let _ = write!(msg, " Did you mean: {suggestion}?");
    }
    msg
}

/// Try to find a similar file by checking alternate extensions or casing.
fn find_similar_file(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    let parent = p.parent()?;

    if let Some(stem) = p.file_stem().and_then(std::ffi::OsStr::to_str) {
        for ext in find_alternate_extensions(path) {
            let candidate = parent.join(format!("{stem}.{ext}"));
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    find_case_insensitive_match(p, parent)
}

/// Check parent directory for a file with the same name but different casing.
fn find_case_insensitive_match(
    path: &std::path::Path,
    parent: &std::path::Path,
) -> Option<String> {
    let filename = path.file_name()?.to_str()?.to_ascii_lowercase();
    let original_name = path.file_name()?.to_str()?;
    let entries = std::fs::read_dir(parent).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_str()?;
        if name_str.to_ascii_lowercase() == filename && name_str != original_name {
            return Some(parent.join(name_str).to_string_lossy().to_string());
        }
    }

    None
}

/// Map file extensions to common alternates for "did you mean?" suggestions.
fn find_alternate_extensions(path: &str) -> Vec<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");

    match ext {
        "ts" => vec!["tsx", "js", "jsx"],
        "tsx" => vec!["ts", "js", "jsx"],
        "js" => vec!["jsx", "ts", "tsx"],
        "jsx" => vec!["js", "ts", "tsx"],
        "rs" => vec!["toml"],
        "toml" => vec!["rs"],
        "py" => vec!["pyi"],
        "pyi" => vec!["py"],
        "c" => vec!["h", "cpp", "hpp"],
        "h" => vec!["c", "cpp"],
        "cpp" => vec!["hpp", "h", "c"],
        "hpp" => vec!["cpp", "h"],
        _ => vec![],
    }
}

/// Mirrors `src/tools/FileWriteTool/FileWriteTool.ts`.
async fn execute_write(input: &serde_json::Value, cwd: &WorkingDir) -> (ToolOutput, ToolResultStatus) {
    let parsed: WriteInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid Write input: {e}")), ToolResultStatus::Error),
    };

    // Size guard
    if parsed.content.as_ref().len() > MaxWriteFileSize::DEFAULT.as_bytes() {
        let actual = FileSizeBytes::new(parsed.content.as_ref().len());
        let limit = FileSizeBytes::new(MaxWriteFileSize::DEFAULT.as_bytes());
        return (
            ToolOutput::new(format!("Content too large ({actual}); max allowed is {limit}")),
            ToolResultStatus::Error,
        );
    }

    let expanded = expand_path(parsed.file_path.as_ref());
    let path = match cwd.validate_path(&expanded) {
        Ok(p) => p,
        Err(e) => return (ToolOutput::new(format!("{e}")), ToolResultStatus::Error),
    };

    let file_path = std::path::Path::new(&path);
    let is_update = file_path.exists();

    // Detect encoding of existing file to preserve it
    let (encoding, endings) = if is_update {
        tokio::fs::read(file_path).await.map_or(
            (FileEncoding::Utf8, LineEndings::Lf),
            |bytes| detect_file_encoding(&bytes),
        )
    } else {
        (FileEncoding::Utf8, LineEndings::Lf)
    };

    if let Some(parent) = file_path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return (ToolOutput::new(format!("Error creating directories: {e}")), ToolResultStatus::Error);
    }

    let encoded = encode_for_write(parsed.content.as_ref(), encoding, endings);

    match tokio::fs::write(file_path, &encoded).await {
        Ok(()) => {
            let msg = if is_update {
                format!("The file {path} has been updated successfully.")
            } else {
                format!("File created successfully at: {path}")
            };
            (ToolOutput::new(msg), ToolResultStatus::Success)
        }
        Err(e) => (ToolOutput::new(format!("Error writing {path}: {e}")), ToolResultStatus::Error),
    }
}

/// Mirrors `src/tools/FileEditTool/FileEditTool.ts`.
///
/// Core algorithm:
/// 1. `old_string`="" + file missing → create new file with `new_string`
/// 2. `old_string`="" + file exists with content → error
/// 3. Find `old_string` (with curly-quote normalization fallback)
/// 4. Multiple matches + !`replace_all` → error
/// 5. Apply replacement → write to disk
async fn execute_edit(input: &serde_json::Value, cwd: &WorkingDir) -> (ToolOutput, ToolResultStatus) {
    let parsed: EditInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid Edit input: {e}")), ToolResultStatus::Error),
    };

    let path = match cwd.validate_path(parsed.file_path.as_ref()) {
        Ok(p) => p,
        Err(e) => return (ToolOutput::new(format!("{e}")), ToolResultStatus::Error),
    };

    // Reject no-op edits early
    if parsed.old_string.as_ref() == parsed.new_string.as_ref() {
        return (
            ToolOutput::new("No changes to make: old_string and new_string are exactly the same.".into()),
            ToolResultStatus::Error,
        );
    }

    let file_content = match tokio::fs::read(&path).await {
        Ok(bytes) => Some(decode_file_bytes(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return (ToolOutput::new(format!("Error reading {path}: {e}")), ToolResultStatus::Error),
    };

    // ── Case 1: file creation (old_string is empty) ──
    if parsed.old_string.is_empty() {
        return handle_empty_old_string(&path, file_content.as_deref(), &parsed).await;
    }

    // ── File must exist for a non-empty old_string ──
    let Some(content) = file_content else {
        return (
            ToolOutput::new(format!("File does not exist: {path}")),
            ToolResultStatus::Error,
        );
    };

    // ── Find the actual string (with curly-quote normalization fallback) ──
    let Some(actual_old) = find_actual_string(&content, parsed.old_string.as_ref()) else {
        return (
            ToolOutput::new(format!(
                "String to replace not found in file.\nString: {}",
                parsed.old_string.as_ref()
            )),
            ToolResultStatus::Error,
        );
    };

    // ── Uniqueness check ──
    let match_count = content.matches(&*actual_old).count();
    if match_count > 1 && !parsed.replace_all.enabled() {
        return (
            ToolOutput::new(format!(
                "Found {match_count} matches of the string to replace, but replace_all is false. \
                 To replace all occurrences, set replace_all to true. \
                 To replace only one occurrence, please provide more context to uniquely identify the instance.\n\
                 String: {}",
                parsed.old_string.as_ref()
            )),
            ToolResultStatus::Error,
        );
    }

    // ── Apply replacement ──
    let new_string = preserve_quote_style(
        parsed.old_string.as_ref(),
        &actual_old,
        parsed.new_string.as_ref(),
    );

    let updated = if parsed.replace_all.enabled() {
        content.replace(&*actual_old, &new_string)
    } else {
        content.replacen(&*actual_old, &new_string, 1)
    };

    // ── Write to disk ──
    if let Some(parent) = std::path::Path::new(&path).parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return (ToolOutput::new(format!("Error creating directories: {e}")), ToolResultStatus::Error);
    }

    match tokio::fs::write(&path, &updated).await {
        Ok(()) => (
            ToolOutput::new(format!("The file {path} has been updated successfully.")),
            ToolResultStatus::Success,
        ),
        Err(e) => (ToolOutput::new(format!("Error writing {path}: {e}")), ToolResultStatus::Error),
    }
}

/// Handle the case where `old_string` is empty — file creation or empty-file replacement.
async fn handle_empty_old_string(
    path: &str,
    file_content: Option<&str>,
    parsed: &EditInput,
) -> (ToolOutput, ToolResultStatus) {
    match file_content {
        // File doesn't exist → create it
        None => {
            if let Some(parent) = std::path::Path::new(path).parent()
                && let Err(e) = tokio::fs::create_dir_all(parent).await
            {
                return (ToolOutput::new(format!("Error creating directories: {e}")), ToolResultStatus::Error);
            }
            match tokio::fs::write(path, parsed.new_string.as_ref()).await {
                Ok(()) => (
                    ToolOutput::new(format!("Created new file {path}")),
                    ToolResultStatus::Success,
                ),
                Err(e) => (ToolOutput::new(format!("Error writing {path}: {e}")), ToolResultStatus::Error),
            }
        }
        // File exists but is empty → replace with new content
        Some(c) if c.trim().is_empty() => {
            match tokio::fs::write(path, parsed.new_string.as_ref()).await {
                Ok(()) => (
                    ToolOutput::new(format!("The file {path} has been updated successfully.")),
                    ToolResultStatus::Success,
                ),
                Err(e) => (ToolOutput::new(format!("Error writing {path}: {e}")), ToolResultStatus::Error),
            }
        }
        // File exists with content → error
        Some(_) => (
            ToolOutput::new("Cannot create new file - file already exists.".into()),
            ToolResultStatus::Error,
        ),
    }
}

// ─── Quote normalization (mirrors FileEditTool/utils.ts) ────────

/// Decode raw bytes to a String, detecting UTF-16LE BOM. Normalizes CRLF → LF.
fn decode_file_bytes(bytes: &[u8]) -> String {
    let raw = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE BOM
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    raw.replace("\r\n", "\n")
}

/// Normalize curly quotes to straight quotes for matching.
fn normalize_quotes(s: &str) -> String {
    s.replace(['\u{2018}', '\u{2019}'], "'")
     .replace(['\u{201C}', '\u{201D}'], "\"")
}

/// Find the actual string in file content, trying exact match first,
/// then falling back to curly-quote normalization.
/// Returns the actual substring from the file (preserving its original quotes).
fn find_actual_string(file_content: &str, search: &str) -> Option<String> {
    // Fast path: exact match
    if file_content.contains(search) {
        return Some(search.to_string());
    }

    // Fallback: normalize quotes in both and find by index
    let normalized_search = normalize_quotes(search);
    let normalized_file = normalize_quotes(file_content);

    let idx = normalized_file.find(&normalized_search)?;
    // The normalized version has the same byte length as the original
    // because curly quotes are multi-byte but normalize to single-byte.
    // We need to map from normalized index back to original index.
    // Since normalization can change byte lengths, count chars instead.
    let char_offset = normalized_file[..idx].chars().count();
    let char_len = normalized_search.chars().count();

    let start: usize = file_content.chars().take(char_offset).map(char::len_utf8).sum();
    let len: usize = file_content[start..].chars().take(char_len).map(char::len_utf8).sum();
    Some(file_content[start..start + len].to_string())
}

/// When `old_string` matched via quote normalization, apply the same curly-quote
/// style to `new_string` so the edit preserves the file's typography.
fn preserve_quote_style(old_string: &str, actual_old: &str, new_string: &str) -> String {
    // If they're the same, no normalization happened
    if old_string == actual_old {
        return new_string.to_string();
    }

    let has_double = actual_old.contains('\u{201C}') || actual_old.contains('\u{201D}');
    let has_single = actual_old.contains('\u{2018}') || actual_old.contains('\u{2019}');

    if !has_double && !has_single {
        return new_string.to_string();
    }

    let mut result = new_string.to_string();
    if has_double {
        result = apply_curly_double_quotes(&result);
    }
    if has_single {
        result = apply_curly_single_quotes(&result);
    }
    result
}

/// Replace straight double quotes with curly double quotes using open/close heuristic.
fn apply_curly_double_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '"' {
            result.push(if is_opening_context(&chars, i) { '\u{201C}' } else { '\u{201D}' });
        } else {
            result.push(ch);
        }
    }
    result
}

/// Replace straight single quotes with curly single quotes, preserving apostrophes in contractions.
fn apply_curly_single_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '\'' {
            let prev_letter = i > 0 && chars[i - 1].is_alphabetic();
            let next_letter = i + 1 < chars.len() && chars[i + 1].is_alphabetic();
            if prev_letter && next_letter {
                // Apostrophe in contraction → right single curly
                result.push('\u{2019}');
            } else {
                result.push(if is_opening_context(&chars, i) { '\u{2018}' } else { '\u{2019}' });
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// A quote is "opening" if preceded by whitespace, start of string, or opening punctuation.
fn is_opening_context(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    matches!(
        chars[index - 1],
        ' ' | '\t' | '\n' | '\r' | '(' | '[' | '{' | '\u{2014}' | '\u{2013}'
    )
}

/// Mirrors `src/tools/GlobTool/GlobTool.ts` — uses `rg --files` for full glob syntax.
async fn execute_glob(input: &serde_json::Value, cwd: &WorkingDir) -> (ToolOutput, ToolResultStatus) {
    let parsed: GlobInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid Glob input: {e}")), ToolResultStatus::Error),
    };

    let (search_path, pattern) = resolve_glob_path_and_pattern(&parsed, cwd);

    let search_path = match search_path {
        Ok(p) => p,
        Err(msg) => return (ToolOutput::new(msg), ToolResultStatus::Error),
    };

    if let Err(msg) = validate_search_dir(&search_path).await {
        return (ToolOutput::new(msg), ToolResultStatus::Error);
    }

    let started = std::time::Instant::now();

    let output = build_rg_glob_command(&search_path, &pattern)
        .output()
        .await;

    let elapsed_ms = started.elapsed().as_millis();

    match output {
        Ok(o) => format_glob_results(&o, &search_path, elapsed_ms, &parsed),
        Err(e) => (ToolOutput::new(format!("Glob failed: {e}")), ToolResultStatus::Error),
    }
}

/// Resolve the search directory and glob pattern, handling absolute patterns.
fn resolve_glob_path_and_pattern(
    parsed: &GlobInput,
    cwd: &WorkingDir,
) -> (std::result::Result<String, String>, String) {
    let pattern = parsed.pattern.as_ref();

    // Absolute pattern: extract base directory from the pattern itself
    if pattern.starts_with('/') {
        return extract_absolute_pattern(pattern);
    }

    let search_path = if let Some(ref p) = parsed.path {
        cwd.validate_path(p.as_ref()).map_err(|e| format!("{e}"))
    } else {
        Ok(cwd.as_ref().to_string())
    };

    (search_path, pattern.to_string())
}

/// Extract base directory and relative pattern from an absolute glob pattern.
///
/// E.g. `/home/user/src/**/*.rs` -> base dir `/home/user/src`, pattern `**/*.rs`.
fn extract_absolute_pattern(pattern: &str) -> (std::result::Result<String, String>, String) {
    let parts: Vec<&str> = pattern.split('/').collect();
    let mut base_parts = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if part.contains('*') || part.contains('?') || part.contains('[') || part.contains('{') {
            let joined = base_parts.join("/");
            let base = if joined.is_empty() { "/".to_string() } else { joined };
            let rest = parts[i..].join("/");
            return (Ok(base), rest);
        }
        base_parts.push(*part);
    }

    // No glob metacharacters — treat whole thing as literal path, pattern `*`
    (Ok(pattern.to_string()), "*".to_string())
}

/// Validate that `search_path` exists and is a directory.
async fn validate_search_dir(path: &str) -> std::result::Result<(), String> {
    match tokio::fs::metadata(path).await {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!("Path is not a directory: {path}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("Directory does not exist: {path}"))
        }
        Err(e) => Err(format!("Cannot access path {path}: {e}")),
    }
}

/// Build the `rg --files` command with glob pattern and VCS exclusions.
fn build_rg_glob_command(search_path: &str, pattern: &str) -> Command {
    let mut cmd = Command::new("rg");
    cmd.arg("--files")
        .arg("--hidden")
        .arg("--sort=modified")
        .arg("--glob").arg(pattern)
        .arg("--glob").arg("!.git")
        .arg("--glob").arg("!.svn")
        .arg("--glob").arg("!.hg")
        .arg("--glob").arg("!.bzr")
        .arg(search_path);
    cmd
}

/// Format `rg --files` output into the structured result with relative paths.
fn format_glob_results(
    output: &std::process::Output,
    search_path: &str,
    elapsed_ms: u128,
    parsed: &GlobInput,
) -> (ToolOutput, ToolResultStatus) {
    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.is_empty() {
        return (
            ToolOutput::new(format!("No files found (in {elapsed_ms}ms)")),
            ToolResultStatus::Success,
        );
    }

    let all_paths: Vec<&str> = stdout.lines().collect();
    let total = all_paths.len();
    let offset = parsed.offset.map_or(
        GlobResultOffset::DEFAULT.value(),
        GlobResultOffset::value,
    );
    let limit = parsed.limit.map_or(
        GlobResultLimit::DEFAULT.value(),
        GlobResultLimit::value,
    );

    let page: Vec<&str> = all_paths.into_iter().skip(offset).take(limit).collect();
    let shown = page.len();

    let mut result = format!("Found {total} files (in {elapsed_ms}ms)\n");
    for path in &page {
        result.push_str(&to_relative_path(path, search_path));
        result.push('\n');
    }

    if shown < total.saturating_sub(offset) {
        let _ = write!(
            result,
            "\n(Showing {limit} of {total} results. Use offset to paginate.)"
        );
    }

    (ToolOutput::new(result), ToolResultStatus::Success)
}

/// Convert an absolute path to a relative path by stripping the `cwd` prefix.
///
/// If `abs_path` does not start with `cwd`, returns the original path unchanged.
#[must_use]
fn to_relative_path(abs_path: &str, cwd: &str) -> String {
    let normalized_cwd = if cwd.ends_with('/') {
        cwd.to_string()
    } else {
        format!("{cwd}/")
    };

    abs_path.strip_prefix(&normalized_cwd).map_or_else(
        || abs_path.to_string(),
        |relative| {
            if relative.is_empty() {
                ".".to_string()
            } else {
                relative.to_string()
            }
        },
    )
}

/// Maximum character length for grep output before truncation.
const GREP_MAX_OUTPUT_CHARS: usize = 20_000;

/// Mirrors `src/tools/GrepTool/GrepTool.ts` — uses `rg` with full parameter support.
async fn execute_grep(input: &serde_json::Value, cwd: &WorkingDir) -> (ToolOutput, ToolResultStatus) {
    let parsed: GrepInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid Grep input: {e}")), ToolResultStatus::Error),
    };

    let search_path = if let Some(ref p) = parsed.path {
        match cwd.validate_path(p.as_ref()) {
            Ok(validated) => validated,
            Err(e) => return (ToolOutput::new(format!("{e}")), ToolResultStatus::Error),
        }
    } else {
        cwd.as_ref().to_string()
    };

    let mode = parsed.output_mode.unwrap_or_default();
    let mut cmd = build_rg_command(&parsed, mode, &search_path);

    match cmd.output().await {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.is_empty() {
                return (ToolOutput::new("No matches found".into()), ToolResultStatus::Success);
            }
            let cwd_str = cwd.as_ref();
            format_grep_output(&stdout, mode, cwd_str, &parsed)
        }
        Err(e) => (ToolOutput::new(format!("Grep failed: {e}")), ToolResultStatus::Error),
    }
}

/// Build the `rg` command with all flags derived from `GrepInput`.
fn build_rg_command(parsed: &GrepInput, mode: GrepOutputMode, search_path: &str) -> Command {
    let mut cmd = Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--hidden")
        .arg("--max-columns").arg("500");

    // VCS exclusions
    for dir in &[".git", ".svn", ".hg", ".bzr", ".jj"] {
        cmd.arg("--glob").arg(format!("!{dir}"));
    }

    // Output mode flags
    add_output_mode_flags(&mut cmd, mode, parsed);

    // Case insensitive
    if parsed.case_insensitive.is_some_and(CaseInsensitive::enabled) {
        cmd.arg("-i");
    }

    // Multiline
    if parsed.multiline.is_some_and(MultilineMode::enabled) {
        cmd.arg("-U").arg("--multiline-dotall");
    }

    // File type
    if let Some(ref ft) = parsed.file_type {
        cmd.arg("--type").arg(ft.as_ref());
    }

    // Glob filter
    if let Some(ref glob) = parsed.glob {
        cmd.arg("--glob").arg(glob.as_ref());
    }

    // Pattern: use `-e` for dash-prefixed patterns to avoid misinterpretation
    let pattern = parsed.pattern.as_ref();
    if pattern.starts_with('-') {
        cmd.arg("-e").arg(pattern);
    } else {
        cmd.arg(pattern);
    }

    cmd.arg(search_path);
    cmd
}

/// Add output-mode-specific flags to the `rg` command.
fn add_output_mode_flags(cmd: &mut Command, mode: GrepOutputMode, parsed: &GrepInput) {
    match mode {
        GrepOutputMode::Content => {
            let show_lines = parsed.line_numbers
                .map_or(ShowLineNumbers::DEFAULT.enabled(), ShowLineNumbers::enabled);
            if show_lines {
                cmd.arg("--line-number");
            }
            if let Some(ctx) = parsed.context {
                cmd.arg("-C").arg(ctx.value().to_string());
            }
            if let Some(after) = parsed.context_after {
                cmd.arg("-A").arg(after.value().to_string());
            }
            if let Some(before) = parsed.context_before {
                cmd.arg("-B").arg(before.value().to_string());
            }
        }
        GrepOutputMode::FilesWithMatches => {
            cmd.arg("-l");
        }
        GrepOutputMode::Count => {
            cmd.arg("-c");
        }
    }
}

/// Post-process `rg` output: relative paths, `offset`/`head_limit`, count summary, truncation.
fn format_grep_output(
    stdout: &str,
    mode: GrepOutputMode,
    cwd: &str,
    parsed: &GrepInput,
) -> (ToolOutput, ToolResultStatus) {
    let offset = parsed.offset.map_or(ResultOffset::DEFAULT.value(), ResultOffset::value);
    let head_limit = parsed.head_limit.map_or(HeadLimit::DEFAULT.value(), HeadLimit::value);

    // Convert absolute paths to relative in each line
    let lines: Vec<String> = stdout
        .lines()
        .map(|line| relativize_grep_line(line, cwd))
        .collect();

    let total = lines.len();
    let offset_usize = offset as usize;

    let page: Vec<&str> = if head_limit == 0 {
        // 0 means unlimited
        lines.iter().skip(offset_usize).map(String::as_str).collect()
    } else {
        lines.iter().skip(offset_usize).take(head_limit as usize).map(String::as_str).collect()
    };

    let shown = page.len();
    let mut result = page.join("\n");

    // Count summary for count mode
    if mode == GrepOutputMode::Count {
        let (occurrences, files) = summarize_counts(&page);
        let _ = write!(result, "\nFound {occurrences} occurrences across {files} files");
    }

    // Truncation note
    let effective_total = total.saturating_sub(offset_usize);
    if head_limit > 0 && shown < effective_total {
        let _ = write!(result, "\n(Showing {shown} of {total} results)");
    }

    // Max output size cap
    if result.len() > GREP_MAX_OUTPUT_CHARS {
        result = truncate_at_line_boundary(&result, GREP_MAX_OUTPUT_CHARS);
    }

    (ToolOutput::new(result), ToolResultStatus::Success)
}

/// Convert absolute paths in a grep output line to relative paths.
///
/// Handles formats like `/abs/path:10:content` and `/abs/path:10` and `/abs/path`.
fn relativize_grep_line(line: &str, cwd: &str) -> String {
    if !line.starts_with('/') {
        return line.to_string();
    }
    if let Some(colon_idx) = line.find(':') {
        let path_part = &line[..colon_idx];
        let rest = &line[colon_idx..];
        format!("{}{rest}", to_relative_path(path_part, cwd))
    } else {
        to_relative_path(line, cwd)
    }
}

/// Sum count entries from `rg -c` output (format: `file:N`).
fn summarize_counts(lines: &[&str]) -> (u64, u64) {
    let mut total_occurrences: u64 = 0;
    let mut file_count: u64 = 0;
    for line in lines {
        if let Some(colon_idx) = line.rfind(':') {
            if let Ok(n) = line[colon_idx + 1..].trim().parse::<u64>() {
                total_occurrences = total_occurrences.saturating_add(n);
                file_count = file_count.saturating_add(1);
            }
        }
    }
    (total_occurrences, file_count)
}

/// Truncate output at a line boundary, appending a note.
fn truncate_at_line_boundary(text: &str, max_chars: usize) -> String {
    let truncated = &text[..max_chars];
    let cut_point = truncated.rfind('\n').unwrap_or(max_chars);
    let mut result = text[..cut_point].to_string();
    let _ = write!(result, "\n(Output truncated — exceeded {max_chars} character limit)");
    result
}

/// Mirrors `src/tools/WebFetchTool/WebFetchTool.ts`.
///
/// Fetches a URL, converts HTML to markdown, truncates to `MaxMarkdownLength`.
/// The TS version runs a secondary Haiku call to summarize — deferred until
/// the architecture supports tool→model calls.
async fn execute_webfetch(input: &serde_json::Value) -> (ToolOutput, ToolResultStatus) {
    let parsed: WebFetchInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => return (ToolOutput::new(format!("Invalid WebFetch input: {e}")), ToolResultStatus::Error),
    };

    if let Err(msg) = validate_fetch_url(parsed.url.as_ref()) {
        return (ToolOutput::new(msg), ToolResultStatus::Error);
    }

    let fetch_url = upgrade_to_https(parsed.url.as_ref());
    let start = std::time::Instant::now();

    match fetch_with_redirect_policy(&fetch_url).await {
        Ok(FetchResult::CrossHostRedirect { original, redirect, status }) => {
            let msg = format!(
                "REDIRECT DETECTED: The URL redirects to a different host.\n\n\
                 Original URL: {original}\nRedirect URL: {redirect}\nStatus: {status}\n\n\
                 To complete your request, use WebFetch again with:\n\
                 - url: \"{redirect}\"\n- prompt: \"{}\"",
                parsed.prompt.as_ref()
            );
            (ToolOutput::new(msg), ToolResultStatus::Success)
        }
        Ok(FetchResult::Success(response)) => {
            build_fetch_output(response, &fetch_url, &parsed, start).await
        }
        Err(e) => (ToolOutput::new(format!("Fetch failed: {e}")), ToolResultStatus::Error),
    }
}

/// Build the final output from a successful HTTP response.
async fn build_fetch_output(
    response: reqwest::Response,
    fetch_url: &str,
    parsed: &WebFetchInput,
    start: std::time::Instant,
) -> (ToolOutput, ToolResultStatus) {
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => return (ToolOutput::new(format!("Error reading response body: {e}")), ToolResultStatus::Error),
    };

    if bytes.len() as u64 > MaxHttpContentLength::DEFAULT.value() {
        return (
            ToolOutput::new(format!(
                "Response too large ({} bytes, max {} bytes)",
                bytes.len(),
                MaxHttpContentLength::DEFAULT.value()
            )),
            ToolResultStatus::Error,
        );
    }

    let raw_text = String::from_utf8_lossy(&bytes);
    let markdown = if content_type.contains("text/html") {
        html2md::parse_html(&raw_text)
    } else {
        raw_text.into_owned()
    };

    let max_len = MaxMarkdownLength::DEFAULT.value();
    let content = if markdown.len() > max_len {
        format!(
            "{}\n\n[Content truncated ({} chars, max {max_len})]",
            &markdown[..max_len],
            markdown.len()
        )
    } else {
        markdown
    };

    let elapsed = start.elapsed();
    let result = format!(
        "URL: {fetch_url}\nStatus: {} {}\nBytes: {}\nDuration: {}ms\n\
         Prompt: {}\n\n---\n\n{content}",
        status.as_u16(),
        status.canonical_reason().unwrap_or(""),
        bytes.len(),
        elapsed.as_millis(),
        parsed.prompt.as_ref(),
    );

    (ToolOutput::new(result), ToolResultStatus::Success)
}

// ─── WebFetch helpers ───────────────────────────────────────────

/// Validate a URL for fetching — scheme, length, no credentials, valid hostname.
fn validate_fetch_url(url: &str) -> std::result::Result<(), String> {
    if url.len() > MaxUrlLength::DEFAULT.value() {
        return Err(format!(
            "URL too long ({} chars, max {})",
            url.len(),
            MaxUrlLength::DEFAULT.value()
        ));
    }

    let parsed = url::Url::parse(url)
        .map_err(|e| format!("Invalid URL \"{url}\": {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("Unsupported URL scheme: {scheme}")),
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URLs with credentials are not supported".into());
    }

    let host = parsed.host_str().unwrap_or("");
    if !host.contains('.') {
        return Err(format!("Invalid hostname: {host}"));
    }

    Ok(())
}

/// Upgrade `http://` URLs to `https://`.
fn upgrade_to_https(url: &str) -> String {
    url.strip_prefix("http://").map_or_else(
        || url.to_string(),
        |rest| format!("https://{rest}"),
    )
}

/// Result of a fetch — success or a cross-host redirect for the model to handle.
enum FetchResult {
    Success(reqwest::Response),
    CrossHostRedirect {
        original: String,
        redirect: String,
        status: u16,
    },
}

/// Fetch a URL following only same-host redirects (max 10 hops).
/// Cross-host redirects are returned as `FetchResult::CrossHostRedirect`.
async fn fetch_with_redirect_policy(url: &str) -> std::result::Result<FetchResult, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(
            FetchTimeoutSecs::DEFAULT.as_secs(),
        ))
        .user_agent("ClaudeCode/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut current_url = url.to_string();
    let max_redirects = 10u8;

    for _ in 0..max_redirects {
        let resp = client
            .get(&current_url)
            .header("Accept", "text/markdown, text/html, */*")
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        if !resp.status().is_redirection() {
            return Ok(FetchResult::Success(resp));
        }

        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("Redirect missing Location header")?;

        let redirect_url = url::Url::parse(location)
            .or_else(|_| {
                url::Url::parse(&current_url).and_then(|base| base.join(location))
            })
            .map_err(|e| format!("Invalid redirect URL: {e}"))?
            .to_string();

        if is_same_host(&current_url, &redirect_url) {
            current_url = redirect_url;
        } else {
            return Ok(FetchResult::CrossHostRedirect {
                original: current_url,
                redirect: redirect_url,
                status: resp.status().as_u16(),
            });
        }
    }

    Err(format!("Too many redirects (exceeded {max_redirects})"))
}

/// Check if two URLs share the same host (ignoring `www.` prefix).
fn is_same_host(a: &str, b: &str) -> bool {
    let strip_www = |h: &str| h.strip_prefix("www.").unwrap_or(h).to_lowercase();
    let host_a = url::Url::parse(a)
        .ok()
        .and_then(|u| u.host_str().map(&strip_www));
    let host_b = url::Url::parse(b)
        .ok()
        .and_then(|u| u.host_str().map(strip_www));
    host_a.is_some() && host_a == host_b
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_quotes ──

    #[test]
    fn normalize_quotes_straight_unchanged() {
        assert_eq!(normalize_quotes("hello 'world'"), "hello 'world'");
    }

    #[test]
    fn normalize_quotes_curly_to_straight() {
        assert_eq!(normalize_quotes("he said \u{201C}hello\u{201D}"), "he said \"hello\"");
        assert_eq!(normalize_quotes("\u{2018}hi\u{2019}"), "'hi'");
    }

    // ── find_actual_string ──

    #[test]
    fn find_actual_exact_match() {
        let result = find_actual_string("hello world", "world");
        assert_eq!(result, Some("world".to_string()));
    }

    #[test]
    fn find_actual_not_found() {
        assert!(find_actual_string("hello world", "xyz").is_none());
    }

    #[test]
    fn find_actual_curly_quote_fallback() {
        let file = "he said \u{201C}hello\u{201D}";
        let search = "he said \"hello\"";
        let result = find_actual_string(file, search);
        assert_eq!(result, Some("he said \u{201C}hello\u{201D}".to_string()));
    }

    #[test]
    fn find_actual_single_curly_fallback() {
        let file = "it\u{2019}s fine";
        let search = "it's fine";
        let result = find_actual_string(file, search);
        assert_eq!(result, Some("it\u{2019}s fine".to_string()));
    }

    // ── preserve_quote_style ──

    #[test]
    fn preserve_style_no_normalization() {
        let result = preserve_quote_style("hello", "hello", "world");
        assert_eq!(result, "world");
    }

    #[test]
    fn preserve_style_applies_curly_doubles() {
        // old_string was straight, actual_old was curly → new_string should get curly
        let result = preserve_quote_style(
            "said \"hi\"",
            "said \u{201C}hi\u{201D}",
            "said \"bye\"",
        );
        assert_eq!(result, "said \u{201C}bye\u{201D}");
    }

    // ── is_opening_context ──

    #[test]
    fn opening_context_at_start() {
        let chars = vec!['"'];
        assert!(is_opening_context(&chars, 0));
    }

    #[test]
    fn opening_context_after_space() {
        let chars: Vec<char> = " \"".chars().collect();
        assert!(is_opening_context(&chars, 1));
    }

    #[test]
    fn closing_context_after_letter() {
        let chars: Vec<char> = "a\"".chars().collect();
        assert!(!is_opening_context(&chars, 1));
    }

    // ── apply_curly_single_quotes contraction ──

    #[test]
    fn contraction_preserved() {
        let result = apply_curly_single_quotes("don't");
        assert!(result.contains('\u{2019}'));
        assert!(!result.contains('\u{2018}'));
    }

    // ── decode_file_bytes ──

    #[test]
    fn decode_utf8_crlf_normalized() {
        assert_eq!(decode_file_bytes(b"a\r\nb"), "a\nb");
    }

    #[test]
    fn decode_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE]; // BOM
        for &b in "hi".encode_utf16().collect::<Vec<u16>>().iter() {
            bytes.extend_from_slice(&b.to_le_bytes());
        }
        assert_eq!(decode_file_bytes(&bytes), "hi");
    }

    // ── edit_input serde ──

    #[test]
    fn edit_input_parses_typed() {
        let json = serde_json::json!({
            "file_path": "/tmp/test.rs",
            "old_string": "foo",
            "new_string": "bar"
        });
        let input: EditInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.file_path.as_ref(), "/tmp/test.rs");
        assert_eq!(input.old_string.as_ref(), "foo");
        assert_eq!(input.new_string.as_ref(), "bar");
        assert!(!input.replace_all.enabled());
    }

    #[test]
    fn edit_input_replace_all_true() {
        let json = serde_json::json!({
            "file_path": "/tmp/test.rs",
            "old_string": "foo",
            "new_string": "bar",
            "replace_all": true
        });
        let input: EditInput = serde_json::from_value(json).unwrap();
        assert!(input.replace_all.enabled());
    }

    // ── to_relative_path ──

    #[test]
    fn relative_path_strips_cwd_prefix() {
        assert_eq!(
            to_relative_path("/home/user/project/src/main.rs", "/home/user/project"),
            "src/main.rs"
        );
    }

    #[test]
    fn relative_path_cwd_with_trailing_slash() {
        assert_eq!(
            to_relative_path("/home/user/project/src/main.rs", "/home/user/project/"),
            "src/main.rs"
        );
    }

    #[test]
    fn relative_path_outside_cwd_unchanged() {
        assert_eq!(
            to_relative_path("/other/path/file.rs", "/home/user/project"),
            "/other/path/file.rs"
        );
    }

    #[test]
    fn relative_path_exact_cwd_returns_dot() {
        assert_eq!(
            to_relative_path("/home/user/project/", "/home/user/project/"),
            "."
        );
    }

    // ── extract_absolute_pattern ──

    #[test]
    fn absolute_pattern_splits_at_glob() {
        let (base, pat) = extract_absolute_pattern("/home/user/src/**/*.rs");
        assert_eq!(base.unwrap(), "/home/user/src");
        assert_eq!(pat, "**/*.rs");
    }

    #[test]
    fn absolute_pattern_glob_in_first_component() {
        let (base, pat) = extract_absolute_pattern("/*.txt");
        assert_eq!(base.unwrap(), "/");
        assert_eq!(pat, "*.txt");
    }

    #[test]
    fn absolute_pattern_no_glob_returns_literal() {
        let (base, pat) = extract_absolute_pattern("/home/user/file.txt");
        assert_eq!(base.unwrap(), "/home/user/file.txt");
        assert_eq!(pat, "*");
    }

    #[test]
    fn absolute_pattern_with_braces() {
        let (base, pat) = extract_absolute_pattern("/src/{a,b}/*.rs");
        assert_eq!(base.unwrap(), "/src");
        assert_eq!(pat, "{a,b}/*.rs");
    }

    // ── format_glob_results truncation ──

    #[test]
    fn glob_results_truncated_with_note() {
        let stdout = "/cwd/a.rs\n/cwd/b.rs\n/cwd/c.rs\n/cwd/d.rs\n/cwd/e.rs\n";
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        };

        let parsed: GlobInput = serde_json::from_value(serde_json::json!({
            "pattern": "*.rs",
            "limit": 2,
            "offset": 0
        }))
        .unwrap();

        let (result, status) = format_glob_results(&output, "/cwd", 42, &parsed);
        let text = result.as_ref();

        assert!(!status.is_error());
        assert!(text.contains("Found 5 files (in 42ms)"), "got: {text}");
        assert!(text.contains("a.rs"), "got: {text}");
        assert!(text.contains("b.rs"), "got: {text}");
        assert!(!text.contains("c.rs"), "got: {text}");
        assert!(
            text.contains("(Showing 2 of 5 results. Use offset to paginate.)"),
            "got: {text}"
        );
    }

    #[test]
    fn glob_results_within_limit_no_note() {
        let stdout = "/cwd/a.rs\n/cwd/b.rs\n";
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        };

        let parsed: GlobInput = serde_json::from_value(serde_json::json!({
            "pattern": "*.rs"
        }))
        .unwrap();

        let (result, _) = format_glob_results(&output, "/cwd", 10, &parsed);
        let text = result.as_ref();

        assert!(text.contains("Found 2 files"), "got: {text}");
        assert!(!text.contains("Showing"), "got: {text}");
    }

    #[test]
    fn glob_results_with_offset() {
        let stdout = "/cwd/a.rs\n/cwd/b.rs\n/cwd/c.rs\n";
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        };

        let parsed: GlobInput = serde_json::from_value(serde_json::json!({
            "pattern": "*.rs",
            "offset": 1,
            "limit": 1
        }))
        .unwrap();

        let (result, _) = format_glob_results(&output, "/cwd", 5, &parsed);
        let text = result.as_ref();

        assert!(text.contains("b.rs"), "got: {text}");
        assert!(!text.contains("a.rs"), "got: {text}");
    }

    // ── glob_input serde ──

    #[test]
    fn glob_input_parses_with_limit_offset() {
        let json = serde_json::json!({
            "pattern": "**/*.rs",
            "path": "/src",
            "limit": 50,
            "offset": 10
        });
        let input: GlobInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.pattern.as_ref(), "**/*.rs");
        assert_eq!(input.limit.unwrap().value(), 50);
        assert_eq!(input.offset.unwrap().value(), 10);
    }

    #[test]
    fn glob_input_defaults_without_limit_offset() {
        let json = serde_json::json!({ "pattern": "*.txt" });
        let input: GlobInput = serde_json::from_value(json).unwrap();
        assert!(input.limit.is_none());
        assert!(input.offset.is_none());
        assert!(input.path.is_none());
    }

    // ── strip_blank_lines ──

    #[test]
    fn strip_blank_lines_removes_leading_trailing() {
        let input = "\n\n  hello\n  world\n\n\n";
        assert_eq!(strip_blank_lines(input), "  hello\n  world");
    }

    #[test]
    fn strip_blank_lines_preserves_internal() {
        assert_eq!(strip_blank_lines("a\n\nb"), "a\n\nb");
    }

    #[test]
    fn strip_blank_lines_all_blank() {
        assert_eq!(strip_blank_lines("\n\n  \n"), "");
    }

    #[test]
    fn strip_blank_lines_empty_input() {
        assert_eq!(strip_blank_lines(""), "");
    }

    // ── extract_base_command ──

    #[test]
    fn extract_base_simple() {
        assert_eq!(extract_base_command("ls -la"), "ls");
    }

    #[test]
    fn extract_base_pipeline_last() {
        assert_eq!(extract_base_command("cat foo | grep bar"), "grep");
    }

    #[test]
    fn extract_base_with_path() {
        assert_eq!(extract_base_command("/usr/bin/grep -r foo"), "grep");
    }

    #[test]
    fn extract_base_with_sudo() {
        assert_eq!(extract_base_command("sudo find / -name foo"), "find");
    }

    #[test]
    fn extract_base_with_env_var() {
        assert_eq!(extract_base_command("FOO=bar grep something"), "grep");
    }

    #[test]
    fn extract_base_pipeline_with_sudo_and_env() {
        assert_eq!(
            extract_base_command("cat file | FOO=1 sudo /usr/local/bin/diff -u a b"),
            "diff"
        );
    }

    // ── is_benign_exit ──

    #[test]
    fn benign_grep_no_matches() {
        assert!(is_benign_exit("grep foo bar.txt", ExitCode::new(1)));
    }

    #[test]
    fn benign_grep_real_error() {
        assert!(!is_benign_exit("grep foo bar.txt", ExitCode::new(2)));
    }

    #[test]
    fn benign_diff_differences() {
        assert!(is_benign_exit("diff a.txt b.txt", ExitCode::new(1)));
    }

    #[test]
    fn benign_find_partial() {
        assert!(is_benign_exit("find / -name foo", ExitCode::new(1)));
    }

    #[test]
    fn benign_pipeline_last_is_grep() {
        assert!(is_benign_exit("cat file | grep pattern", ExitCode::new(1)));
    }

    #[test]
    fn not_benign_unknown_command() {
        assert!(!is_benign_exit("cargo build", ExitCode::new(1)));
    }

    #[test]
    fn benign_zero_always() {
        assert!(is_benign_exit("anything", ExitCode::new(0)));
    }

    // ── truncate_output ──

    #[test]
    fn truncate_within_limit() {
        assert_eq!(truncate_output("short", MaxOutputLen::DEFAULT), "short");
    }

    #[test]
    fn truncate_exceeds_limit() {
        let line = "abcdefghij\n"; // 11 chars per line
        let output: String = line.repeat(10);
        let max = MaxOutputLen::from_value(30);
        let result = truncate_output(&output, max);
        assert!(result.contains("lines truncated"));
        assert!(result.starts_with("abcdefghij\nabcdefghij\n"));
    }

    #[test]
    fn truncate_at_line_boundary() {
        let output = "line1\nline2\nline3\nline4\n";
        let max = MaxOutputLen::from_value(12);
        let result = truncate_output(&output, max);
        assert!(result.starts_with("line1\nline2\n"));
        assert!(result.contains("truncated"));
    }

    // ── persist_large_output ──

    #[test]
    fn persist_below_threshold() {
        assert_eq!(
            persist_large_output("small output", LargeOutputThreshold::DEFAULT),
            "small output"
        );
    }

    #[test]
    fn persist_above_threshold() {
        let output: String = "x".repeat(31_000);
        let result = persist_large_output(&output, LargeOutputThreshold::DEFAULT);
        assert!(result.starts_with("Output too large (31000 bytes)"));
        assert!(result.contains("/tmp/claude-tool-results/"));
        assert!(result.contains("Preview (first 2KB):"));
    }

    // ── floor_char_boundary ──

    #[test]
    fn floor_boundary_ascii() {
        assert_eq!(floor_char_boundary("hello", 3), 3);
    }

    #[test]
    fn floor_boundary_multibyte() {
        // em-dash (\u{2014}) is 3 bytes in UTF-8
        let s = "a\u{2014}b"; // byte layout: [a][e2][80][94][b]
        assert_eq!(floor_char_boundary(s, 2), 1); // mid em-dash snaps to after 'a'
        assert_eq!(floor_char_boundary(s, 1), 1); // right after 'a'
        assert_eq!(floor_char_boundary(s, 4), 4); // end of em-dash
    }

    #[test]
    fn floor_boundary_beyond_len() {
        assert_eq!(floor_char_boundary("hi", 100), 2);
    }

    // ── grep_input serde ──

    #[test]
    fn grep_input_parses_all_params() {
        let json = serde_json::json!({
            "pattern": "fn\\s+\\w+",
            "path": "/src",
            "glob": "*.rs",
            "output_mode": "content",
            "-A": 3,
            "-B": 2,
            "-C": 1,
            "-i": true,
            "-n": false,
            "type": "rust",
            "multiline": true,
            "head_limit": 100,
            "offset": 5
        });
        let input: GrepInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.pattern.as_ref(), "fn\\s+\\w+");
        assert_eq!(input.path.unwrap().as_ref(), "/src");
        assert_eq!(input.glob.unwrap().as_ref(), "*.rs");
        assert_eq!(input.output_mode.unwrap(), GrepOutputMode::Content);
        assert_eq!(input.context_after.unwrap().value(), 3);
        assert_eq!(input.context_before.unwrap().value(), 2);
        assert_eq!(input.context.unwrap().value(), 1);
        assert!(input.case_insensitive.unwrap().enabled());
        assert!(!input.line_numbers.unwrap().enabled());
        assert_eq!(input.file_type.unwrap().as_ref(), "rust");
        assert!(input.multiline.unwrap().enabled());
        assert_eq!(input.head_limit.unwrap().value(), 100);
        assert_eq!(input.offset.unwrap().value(), 5);
    }

    #[test]
    fn grep_input_parses_minimal() {
        let json = serde_json::json!({ "pattern": "TODO" });
        let input: GrepInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.pattern.as_ref(), "TODO");
        assert!(input.output_mode.is_none());
        assert!(input.context_after.is_none());
        assert!(input.head_limit.is_none());
    }

    #[test]
    fn grep_input_alias_names_work() {
        let json = serde_json::json!({
            "pattern": "test",
            "context_after": 5,
            "context_before": 3,
            "case_insensitive": true,
            "line_numbers": true
        });
        let input: GrepInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.context_after.unwrap().value(), 5);
        assert_eq!(input.context_before.unwrap().value(), 3);
        assert!(input.case_insensitive.unwrap().enabled());
        assert!(input.line_numbers.unwrap().enabled());
    }

    #[test]
    fn grep_output_mode_deserializes() {
        let json = serde_json::json!({ "pattern": "x", "output_mode": "count" });
        let input: GrepInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.output_mode.unwrap(), GrepOutputMode::Count);

        let json2 = serde_json::json!({ "pattern": "x", "output_mode": "files_with_matches" });
        let input2: GrepInput = serde_json::from_value(json2).unwrap();
        assert_eq!(input2.output_mode.unwrap(), GrepOutputMode::FilesWithMatches);
    }

    // ── build_rg_command flag building ──

    #[test]
    fn rg_command_content_mode_flags() {
        let input: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test",
            "output_mode": "content",
            "-A": 3,
            "-B": 2,
            "-i": true,
            "multiline": true,
            "type": "rust"
        }))
        .unwrap();
        let mode = input.output_mode.unwrap();
        let cmd = build_rg_command(&input, mode, "/tmp");
        let args: Vec<_> = cmd.as_std().get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"--line-number".to_string()), "args: {args:?}");
        assert!(args.contains(&"-A".to_string()), "args: {args:?}");
        assert!(args.contains(&"3".to_string()), "args: {args:?}");
        assert!(args.contains(&"-B".to_string()), "args: {args:?}");
        assert!(args.contains(&"2".to_string()), "args: {args:?}");
        assert!(args.contains(&"-i".to_string()), "args: {args:?}");
        assert!(args.contains(&"-U".to_string()), "args: {args:?}");
        assert!(args.contains(&"--multiline-dotall".to_string()), "args: {args:?}");
        assert!(args.contains(&"--type".to_string()), "args: {args:?}");
        assert!(args.contains(&"rust".to_string()), "args: {args:?}");
        assert!(!args.contains(&"-l".to_string()), "args: {args:?}");
        assert!(!args.contains(&"-c".to_string()), "args: {args:?}");
    }

    #[test]
    fn rg_command_files_with_matches_mode() {
        let input: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test"
        }))
        .unwrap();
        let cmd = build_rg_command(&input, GrepOutputMode::FilesWithMatches, "/tmp");
        let args: Vec<_> = cmd.as_std().get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-l".to_string()), "args: {args:?}");
        assert!(!args.contains(&"--line-number".to_string()), "args: {args:?}");
    }

    #[test]
    fn rg_command_count_mode() {
        let input: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test",
            "output_mode": "count"
        }))
        .unwrap();
        let mode = input.output_mode.unwrap();
        let cmd = build_rg_command(&input, mode, "/tmp");
        let args: Vec<_> = cmd.as_std().get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-c".to_string()), "args: {args:?}");
    }

    #[test]
    fn rg_command_dash_pattern_uses_e_flag() {
        let input: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "-foo"
        }))
        .unwrap();
        let cmd = build_rg_command(&input, GrepOutputMode::default(), "/tmp");
        let args: Vec<_> = cmd.as_std().get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-e".to_string()), "args: {args:?}");
    }

    #[test]
    fn rg_command_vcs_exclusions_present() {
        let input: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test"
        }))
        .unwrap();
        let cmd = build_rg_command(&input, GrepOutputMode::default(), "/tmp");
        let args: Vec<_> = cmd.as_std().get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        for dir in ["!.git", "!.svn", "!.hg", "!.bzr", "!.jj"] {
            assert!(args.contains(&dir.to_string()), "missing {dir} in {args:?}");
        }
    }

    // ── format_grep_output / head_limit + offset ──

    #[test]
    fn grep_output_head_limit_and_offset() {
        let stdout = "/cwd/a.rs:1:foo\n/cwd/b.rs:2:bar\n/cwd/c.rs:3:baz\n/cwd/d.rs:4:qux\n/cwd/e.rs:5:quux";
        let parsed: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test",
            "head_limit": 2,
            "offset": 1
        }))
        .unwrap();

        let (result, status) = format_grep_output(stdout, GrepOutputMode::Content, "/cwd", &parsed);
        let text = result.as_ref();

        assert!(!status.is_error());
        assert!(!text.contains("a.rs"), "should skip first entry, got: {text}");
        assert!(text.contains("b.rs"), "got: {text}");
        assert!(text.contains("c.rs"), "got: {text}");
        assert!(!text.contains("d.rs"), "should be beyond limit, got: {text}");
        assert!(text.contains("(Showing 2 of 5 results)"), "got: {text}");
    }

    #[test]
    fn grep_output_unlimited_head_limit() {
        let stdout = "a.rs:1:foo\nb.rs:2:bar\nc.rs:3:baz";
        let parsed: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test",
            "head_limit": 0
        }))
        .unwrap();

        let (result, _) = format_grep_output(stdout, GrepOutputMode::Content, "/cwd", &parsed);
        let text = result.as_ref();
        assert!(text.contains("a.rs"), "got: {text}");
        assert!(text.contains("b.rs"), "got: {text}");
        assert!(text.contains("c.rs"), "got: {text}");
        assert!(!text.contains("Showing"), "got: {text}");
    }

    // ── relativize_grep_line ──

    #[test]
    fn grep_line_relativized_with_colon() {
        assert_eq!(
            relativize_grep_line("/home/user/project/src/main.rs:10:fn main()", "/home/user/project"),
            "src/main.rs:10:fn main()"
        );
    }

    #[test]
    fn grep_line_relativized_path_only() {
        assert_eq!(
            relativize_grep_line("/home/user/project/src/lib.rs", "/home/user/project"),
            "src/lib.rs"
        );
    }

    #[test]
    fn grep_line_not_absolute_unchanged() {
        assert_eq!(
            relativize_grep_line("relative/path.rs:5:code", "/home/user"),
            "relative/path.rs:5:code"
        );
    }

    // ── summarize_counts ──

    #[test]
    fn count_summary_sums_correctly() {
        let lines = vec!["src/a.rs:5", "src/b.rs:3", "src/c.rs:12"];
        let (occurrences, files) = summarize_counts(&lines);
        assert_eq!(occurrences, 20);
        assert_eq!(files, 3);
    }

    #[test]
    fn count_summary_skips_invalid() {
        let lines = vec!["src/a.rs:5", "invalid-line", "src/b.rs:abc"];
        let (occurrences, files) = summarize_counts(&lines);
        assert_eq!(occurrences, 5);
        assert_eq!(files, 1);
    }

    // ── grep count mode output ──

    #[test]
    fn grep_count_mode_appends_summary() {
        let stdout = "/cwd/src/a.rs:5\n/cwd/src/b.rs:3";
        let parsed: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test"
        }))
        .unwrap();

        let (result, _) = format_grep_output(stdout, GrepOutputMode::Count, "/cwd", &parsed);
        let text = result.as_ref();
        assert!(text.contains("src/a.rs:5"), "got: {text}");
        assert!(text.contains("src/b.rs:3"), "got: {text}");
        assert!(text.contains("Found 8 occurrences across 2 files"), "got: {text}");
    }

    // ── truncate_at_line_boundary ──

    #[test]
    fn grep_truncation_at_line_boundary() {
        let text = "line1\nline2\nline3\nline4";
        let result = super::truncate_at_line_boundary(text, 12);
        assert!(result.starts_with("line1\nline2"), "got: {result}");
        assert!(result.contains("exceeded 12 character limit"), "got: {result}");
        assert!(!result.contains("line3"), "got: {result}");
    }

    // ─── Read tool guard tests ─────────────────────────────────────

    // ── binary extension detection ──

    #[test]
    fn binary_extension_blocks_png() {
        let result = check_binary_extension("/tmp/image.png");
        assert!(result.is_some());
        assert!(result.unwrap().contains("png"));
    }

    #[test]
    fn binary_extension_blocks_exe() {
        let result = check_binary_extension("/tmp/app.exe");
        assert!(result.is_some());
        assert!(result.unwrap().contains("exe"));
    }

    #[test]
    fn binary_extension_allows_rs() {
        assert!(check_binary_extension("/tmp/main.rs").is_none());
    }

    #[test]
    fn binary_extension_allows_no_extension() {
        assert!(check_binary_extension("/tmp/Makefile").is_none());
    }

    #[test]
    fn binary_extension_case_insensitive() {
        let result = check_binary_extension("/tmp/photo.PNG");
        assert!(result.is_some());
    }

    // ── file size formatting ──

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_file_size(500), "500 bytes");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(format_file_size(2048), "2.0 KB");
    }

    #[test]
    fn format_size_mb() {
        assert_eq!(format_file_size(10_485_760), "10.0 MB");
    }

    #[test]
    fn format_size_gb() {
        assert_eq!(format_file_size(2_147_483_648), "2.0 GB");
    }

    // ── device path blocking ──

    #[test]
    fn blocks_dev_zero() {
        let result = check_blocked_path("/dev/zero");
        assert!(result.is_some());
        assert!(result.unwrap().contains("/dev/zero"));
    }

    #[test]
    fn blocks_dev_random() {
        assert!(check_blocked_path("/dev/random").is_some());
    }

    #[test]
    fn blocks_proc_fd() {
        let result = check_blocked_path("/proc/1234/fd/0");
        assert!(result.is_some());
        assert!(result.unwrap().contains("/proc/1234/fd/0"));
    }

    #[test]
    fn allows_regular_dev_path() {
        // /dev/shm is not in the blocklist
        assert!(check_blocked_path("/dev/shm/myfile").is_none());
    }

    #[test]
    fn allows_regular_path() {
        assert!(check_blocked_path("/tmp/test.txt").is_none());
    }

    // ── offset 1-based behavior ──

    #[test]
    fn line_offset_1based_to_0based() {
        let json = serde_json::json!({
            "file_path": "/tmp/test.rs",
            "offset": 1
        });
        let input: ReadInput = serde_json::from_value(json).unwrap();
        // offset=1 means first line, internally 0
        assert_eq!(input.offset.unwrap().value(), 0);
        assert_eq!(input.offset.unwrap().raw(), 1);
    }

    #[test]
    fn line_offset_0_treated_as_beginning() {
        let json = serde_json::json!({
            "file_path": "/tmp/test.rs",
            "offset": 0
        });
        let input: ReadInput = serde_json::from_value(json).unwrap();
        // offset=0 saturating_sub(1) = 0
        assert_eq!(input.offset.unwrap().value(), 0);
    }

    #[test]
    fn line_offset_5_becomes_4() {
        let json = serde_json::json!({
            "file_path": "/tmp/test.rs",
            "offset": 5
        });
        let input: ReadInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.offset.unwrap().value(), 4);
    }

    // ── alternate extensions ──

    #[test]
    fn alternate_extensions_ts_to_tsx() {
        let alts = find_alternate_extensions("/src/foo.ts");
        assert!(alts.contains(&"tsx"));
        assert!(alts.contains(&"js"));
    }

    #[test]
    fn alternate_extensions_rs_to_toml() {
        let alts = find_alternate_extensions("/src/lib.rs");
        assert!(alts.contains(&"toml"));
    }

    #[test]
    fn alternate_extensions_unknown() {
        let alts = find_alternate_extensions("/src/file.xyz");
        assert!(alts.is_empty());
    }

    // ── WebFetch helpers ──

    #[test]
    fn validate_url_valid_https() {
        assert!(validate_fetch_url("https://example.com/page").is_ok());
    }

    #[test]
    fn validate_url_rejects_ftp() {
        let err = validate_fetch_url("ftp://example.com").unwrap_err();
        assert!(err.contains("Unsupported URL scheme"));
    }

    #[test]
    fn validate_url_rejects_credentials() {
        let err = validate_fetch_url("https://user:pass@example.com").unwrap_err();
        assert!(err.contains("credentials"));
    }

    #[test]
    fn validate_url_rejects_no_dot_hostname() {
        let err = validate_fetch_url("https://localhost/path").unwrap_err();
        assert!(err.contains("Invalid hostname"));
    }

    #[test]
    fn validate_url_rejects_too_long() {
        let long_url = format!("https://example.com/{}", "a".repeat(2000));
        let err = validate_fetch_url(&long_url).unwrap_err();
        assert!(err.contains("too long"));
    }

    #[test]
    fn upgrade_http_to_https() {
        assert_eq!(upgrade_to_https("http://example.com"), "https://example.com");
    }

    #[test]
    fn upgrade_keeps_https() {
        assert_eq!(upgrade_to_https("https://example.com"), "https://example.com");
    }

    #[test]
    fn same_host_ignores_www() {
        assert!(is_same_host("https://example.com", "https://www.example.com"));
        assert!(is_same_host("https://www.example.com", "https://example.com"));
    }

    #[test]
    fn same_host_different_hosts() {
        assert!(!is_same_host("https://example.com", "https://other.com"));
    }

    #[test]
    fn same_host_case_insensitive() {
        assert!(is_same_host("https://Example.COM", "https://example.com"));
    }

    #[test]
    fn webfetch_input_parses() {
        let json = serde_json::json!({
            "url": "https://example.com",
            "prompt": "summarize this"
        });
        let input: WebFetchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.url.as_ref(), "https://example.com");
        assert_eq!(input.prompt.as_ref(), "summarize this");
    }
}
