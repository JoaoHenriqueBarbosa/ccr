//! Shared utility functions used across multiple tools.

use crate::types::{
    ExitCode, FileEncoding, LargeOutputThreshold, LineEndings, MaxOutputLen, PreviewLen,
};

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
    let endings = if has_crlf {
        LineEndings::CrLf
    } else {
        LineEndings::Lf
    };
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
pub(crate) fn strip_blank_lines(s: &str) -> &str {
    // Find first non-blank line start — scan forward through line boundaries.
    let mut start = 0;
    for line in s.lines() {
        if !line.trim().is_empty() {
            // `line` is a subslice of `s` — compute its offset.
            start = line.as_ptr() as usize - s.as_ptr() as usize;
            break;
        }
    }

    // Find last non-blank line end — scan backward.
    let mut end = start;
    for line in s.lines() {
        if !line.trim().is_empty() {
            let line_start = line.as_ptr() as usize - s.as_ptr() as usize;
            end = line_start + line.len();
        }
    }

    &s[start..end]
}

/// Extract the base command name from a shell command string.
///
/// Handles pipelines by using the last command. Strips `sudo`, env vars,
/// and path prefixes.
#[must_use]
pub(crate) fn extract_base_command(command: &str) -> &str {
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
pub(crate) fn is_benign_exit(command: &str, code: ExitCode) -> bool {
    if code.is_success() {
        return true;
    }
    let base = extract_base_command(command);
    match base {
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "diff" | "colordiff" | "find" | "fd" => {
            code.value() == 1
        }
        _ => false,
    }
}

/// Truncate output to `max_len` chars at a line boundary.
///
/// If truncated, appends a note with the number of lines removed.
#[must_use]
pub(crate) fn truncate_output(output: &str, max_len: MaxOutputLen) -> String {
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
pub(crate) fn persist_large_output(output: &str, threshold: LargeOutputThreshold) -> String {
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
pub(crate) fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut pos = i;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

#[allow(clippy::cast_precision_loss, reason = "display-only formatting")]
pub(crate) fn to_relative_path(abs_path: &str, cwd: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ExitCode;

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

    // ── Property tests ──

    use proptest::prelude::*;

    proptest! {
        /// `floor_char_boundary` always returns a valid char boundary.
        #[test]
        fn floor_char_boundary_always_valid(s in "\\PC{0,200}", i in 0_usize..300) {
            let result = floor_char_boundary(&s, i);
            prop_assert!(s.is_char_boundary(result), "not a char boundary: {result}");
            prop_assert!(result <= i.min(s.len()));
        }

        /// `truncate_output` with default max never panics on any input.
        #[test]
        fn truncate_output_never_panics(s in "\\PC{0,500}") {
            let result = truncate_output(&s, MaxOutputLen::DEFAULT);
            // Should be valid UTF-8 (it's a String, so it is by construction).
            prop_assert!(!result.is_empty() || s.is_empty());
        }

        /// `find_double_newline` result, if Some, is always at a `\n\n` position.
        #[test]
        fn find_double_newline_correct(s in "\\PC{0,300}") {
            let buf = s.as_bytes();
            if let Some(pos) = crate::api::tests::call_find_double_newline(buf) {
                prop_assert_eq!(buf[pos], b'\n');
                prop_assert_eq!(buf[pos + 1], b'\n');
            }
        }
    }
}
