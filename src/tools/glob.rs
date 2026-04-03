//! Glob tool — mirrors `src/tools/GlobTool/GlobTool.ts`.

use std::fmt::Write;

use tokio::process::Command;

use super::helpers::to_relative_path;
use crate::types::{
    GlobInput, GlobResultLimit, GlobResultOffset, ToolDefinition, ToolOutput, ToolResultStatus,
    WorkingDir,
};

pub(crate) fn glob_definition() -> ToolDefinition {
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

pub(crate) async fn execute_glob(
    input: serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: GlobInput = match serde_json::from_value(input) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid Glob input: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    let (search_path, pattern) = resolve_glob_path_and_pattern(&parsed, cwd);

    let search_path = match search_path {
        Ok(p) => p,
        Err(e) => return (ToolOutput::new(e.to_string()), ToolResultStatus::Error),
    };

    if let Err(e) = validate_search_dir(&search_path).await {
        return (ToolOutput::new(e.to_string()), ToolResultStatus::Error);
    }

    let started = std::time::Instant::now();

    let output = build_rg_glob_command(&search_path, &pattern).output().await;

    let elapsed_ms = started.elapsed().as_millis();

    match output {
        Ok(o) => format_glob_results(&o, &search_path, elapsed_ms, &parsed),
        Err(e) => (
            ToolOutput::new(format!("Glob failed: {e}")),
            ToolResultStatus::Error,
        ),
    }
}

/// Resolve the search directory and glob pattern, handling absolute patterns.
fn resolve_glob_path_and_pattern(
    parsed: &GlobInput,
    cwd: &WorkingDir,
) -> (std::result::Result<String, crate::types::AppError>, String) {
    let pattern = parsed.pattern.as_ref();

    // Absolute pattern: extract base directory from the pattern itself
    if pattern.starts_with('/') {
        return extract_absolute_pattern(pattern);
    }

    let search_path = if let Some(ref p) = parsed.path {
        cwd.validate_path(p.as_ref())
            .map_err(|e| crate::types::AppError::FsValidation {
                message: format!("{e}"),
            })
    } else {
        Ok(cwd.as_ref().to_string())
    };

    (search_path, pattern.to_string())
}

/// Extract base directory and relative pattern from an absolute glob pattern.
///
/// E.g. `/home/user/src/**/*.rs` -> base dir `/home/user/src`, pattern `**/*.rs`.
fn extract_absolute_pattern(
    pattern: &str,
) -> (std::result::Result<String, crate::types::AppError>, String) {
    let parts: Vec<&str> = pattern.split('/').collect();
    let mut base_parts = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if part.contains('*') || part.contains('?') || part.contains('[') || part.contains('{') {
            let joined = base_parts.join("/");
            let base = if joined.is_empty() {
                "/".to_string()
            } else {
                joined
            };
            let rest = parts[i..].join("/");
            return (Ok(base), rest);
        }
        base_parts.push(*part);
    }

    // No glob metacharacters — treat whole thing as literal path, pattern `*`
    (Ok(pattern.to_string()), "*".to_string())
}

/// Validate that `search_path` exists and is a directory.
async fn validate_search_dir(path: &str) -> std::result::Result<(), crate::types::AppError> {
    match tokio::fs::metadata(path).await {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(crate::types::AppError::FsValidation {
            message: format!("Path is not a directory: {path}"),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(crate::types::AppError::FsValidation {
                message: format!("Directory does not exist: {path}"),
            })
        }
        Err(e) => Err(crate::types::AppError::FsValidation {
            message: format!("Cannot access path {path}: {e}"),
        }),
    }
}

/// Build the `rg --files` command with glob pattern and VCS exclusions.
fn build_rg_glob_command(search_path: &str, pattern: &str) -> Command {
    let mut cmd = Command::new("rg");
    cmd.arg("--files")
        .arg("--hidden")
        .arg("--sort=modified")
        .arg("--glob")
        .arg(pattern)
        .arg("--glob")
        .arg("!.git")
        .arg("--glob")
        .arg("!.svn")
        .arg("--glob")
        .arg("!.hg")
        .arg("--glob")
        .arg("!.bzr")
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
    let offset = parsed
        .offset
        .map_or(GlobResultOffset::DEFAULT.value(), GlobResultOffset::value);
    let limit = parsed
        .limit
        .map_or(GlobResultLimit::DEFAULT.value(), GlobResultLimit::value);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GlobInput;

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
}
