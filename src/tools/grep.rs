//! Grep tool — mirrors `src/tools/GrepTool/GrepTool.ts`.

use std::fmt::Write;

use tokio::process::Command;

use super::helpers::to_relative_path;
use crate::types::{
    CaseSensitivity, GrepInput, GrepOutputMode, HeadLimit, LineNumberDisplay, MultilineSearch,
    ResultOffset, ToolDefinition, ToolOutput, ToolResultStatus, WorkingDir,
};

pub(crate) fn grep_definition() -> ToolDefinition {
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

/// Maximum output size for grep results (20K bytes).
const GREP_MAX_OUTPUT_BYTES: usize = 20_000;

/// Mirrors `src/tools/GrepTool/GrepTool.ts` — uses `rg` with full parameter support.
pub(crate) async fn execute_grep(
    input: serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: GrepInput = match serde_json::from_value(input) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid Grep input: {e}")),
                ToolResultStatus::Error,
            );
        }
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
                return (
                    ToolOutput::new("No matches found".into()),
                    ToolResultStatus::Success,
                );
            }
            let cwd_str = cwd.as_ref();
            format_grep_output(&stdout, mode, cwd_str, &parsed)
        }
        Err(e) => (
            ToolOutput::new(format!("Grep failed: {e}")),
            ToolResultStatus::Error,
        ),
    }
}

/// Build the `rg` command with all flags derived from `GrepInput`.
fn build_rg_command(parsed: &GrepInput, mode: GrepOutputMode, search_path: &str) -> Command {
    let mut cmd = Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--hidden")
        .arg("--max-columns")
        .arg("500");

    // VCS exclusions
    for dir in &[".git", ".svn", ".hg", ".bzr", ".jj"] {
        cmd.arg("--glob").arg(format!("!{dir}"));
    }

    // Output mode flags
    add_output_mode_flags(&mut cmd, mode, parsed);

    // Case insensitive
    if parsed
        .case_insensitive
        .is_some_and(CaseSensitivity::enabled)
    {
        cmd.arg("-i");
    }

    // Multiline
    if parsed.multiline.is_some_and(MultilineSearch::enabled) {
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
            let show_lines = parsed.line_numbers.map_or(
                LineNumberDisplay::DEFAULT.enabled(),
                LineNumberDisplay::enabled,
            );
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
    let offset = parsed
        .offset
        .map_or(ResultOffset::DEFAULT.value(), ResultOffset::value);
    let head_limit = parsed
        .head_limit
        .map_or(HeadLimit::DEFAULT.value(), HeadLimit::value);

    // Convert absolute paths to relative in each line
    let lines: Vec<String> = stdout
        .lines()
        .map(|line| relativize_grep_line(line, cwd))
        .collect();

    let total = lines.len();
    #[allow(
        clippy::cast_possible_truncation,
        reason = "u32→usize is lossless on ≥32-bit platforms"
    )]
    let offset_n = offset as usize;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "u32→usize is lossless on ≥32-bit platforms"
    )]
    let limit_n = head_limit as usize;

    let page: Vec<&str> = if head_limit == 0 {
        // 0 means unlimited
        lines.iter().skip(offset_n).map(String::as_str).collect()
    } else {
        lines
            .iter()
            .skip(offset_n)
            .take(limit_n)
            .map(String::as_str)
            .collect()
    };

    let shown = page.len();
    let mut result = page.join("\n");

    // Count summary for count mode
    if mode == GrepOutputMode::Count {
        let (occurrences, files) = summarize_counts(&page);
        let _ = write!(
            result,
            "\nFound {occurrences} occurrences across {files} files"
        );
    }

    // Truncation note
    let effective_total = total.saturating_sub(offset_n);
    if head_limit > 0 && shown < effective_total {
        let _ = write!(result, "\n(Showing {shown} of {total} results)");
    }

    // Max output size cap
    if result.len() > GREP_MAX_OUTPUT_BYTES {
        result = truncate_at_line_boundary(&result, GREP_MAX_OUTPUT_BYTES);
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
fn truncate_at_line_boundary(text: &str, max_bytes: usize) -> String {
    let safe = super::helpers::floor_char_boundary(text, max_bytes);
    let truncated = &text[..safe];
    let cut_point = truncated.rfind('\n').unwrap_or(safe);
    let mut result = text[..cut_point].to_string();
    let _ = write!(
        result,
        "\n(Output truncated — exceeded {max_bytes} byte limit)"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GrepInput, GrepOutputMode};

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
        assert_eq!(
            input2.output_mode.unwrap(),
            GrepOutputMode::FilesWithMatches
        );
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
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(
            args.contains(&"--line-number".to_string()),
            "args: {args:?}"
        );
        assert!(args.contains(&"-A".to_string()), "args: {args:?}");
        assert!(args.contains(&"3".to_string()), "args: {args:?}");
        assert!(args.contains(&"-B".to_string()), "args: {args:?}");
        assert!(args.contains(&"2".to_string()), "args: {args:?}");
        assert!(args.contains(&"-i".to_string()), "args: {args:?}");
        assert!(args.contains(&"-U".to_string()), "args: {args:?}");
        assert!(
            args.contains(&"--multiline-dotall".to_string()),
            "args: {args:?}"
        );
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
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-l".to_string()), "args: {args:?}");
        assert!(
            !args.contains(&"--line-number".to_string()),
            "args: {args:?}"
        );
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
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
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
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
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
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        for dir in ["!.git", "!.svn", "!.hg", "!.bzr", "!.jj"] {
            assert!(args.contains(&dir.to_string()), "missing {dir} in {args:?}");
        }
    }

    // ── format_grep_output / head_limit + offset ──

    #[test]
    fn grep_output_head_limit_and_offset() {
        let stdout =
            "/cwd/a.rs:1:foo\n/cwd/b.rs:2:bar\n/cwd/c.rs:3:baz\n/cwd/d.rs:4:qux\n/cwd/e.rs:5:quux";
        let parsed: GrepInput = serde_json::from_value(serde_json::json!({
            "pattern": "test",
            "head_limit": 2,
            "offset": 1
        }))
        .unwrap();

        let (result, status) = format_grep_output(stdout, GrepOutputMode::Content, "/cwd", &parsed);
        let text = result.as_ref();

        assert!(!status.is_error());
        assert!(
            !text.contains("a.rs"),
            "should skip first entry, got: {text}"
        );
        assert!(text.contains("b.rs"), "got: {text}");
        assert!(text.contains("c.rs"), "got: {text}");
        assert!(
            !text.contains("d.rs"),
            "should be beyond limit, got: {text}"
        );
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
            relativize_grep_line(
                "/home/user/project/src/main.rs:10:fn main()",
                "/home/user/project"
            ),
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
        assert!(
            text.contains("Found 8 occurrences across 2 files"),
            "got: {text}"
        );
    }

    // ── truncate_at_line_boundary ──

    #[test]
    fn grep_truncation_at_line_boundary() {
        let text = "line1\nline2\nline3\nline4";
        let result = super::truncate_at_line_boundary(text, 12);
        assert!(result.starts_with("line1\nline2"), "got: {result}");
        assert!(result.contains("exceeded 12 byte limit"), "got: {result}");
        assert!(!result.contains("line3"), "got: {result}");
    }
}
