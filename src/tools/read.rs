//! Read tool — mirrors `src/tools/FileReadTool/FileReadTool.ts`.

use std::fmt::Write;

use crate::types::{
    FileSizeBytes, LineLimit, LineOffset, MaxReadFileSize, ReadInput, ToolDefinition, ToolOutput,
    ToolResultStatus, WorkingDir,
};

pub(crate) fn read_definition() -> ToolDefinition {
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

pub(crate) async fn execute_read(
    input: serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: ReadInput = match serde_json::from_value(input) {
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
    if let Err(e) = check_file_size(&path).await {
        let msg = e.to_string();
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
    if content.is_empty() {
        return (
            ToolOutput::new("Warning: the file exists but the contents are empty.".into()),
            ToolResultStatus::Success,
        );
    }

    let total_lines = content.lines().count();
    let offset = parsed.offset.map_or(0, LineOffset::value);
    let limit = parsed
        .limit
        .map_or(LineLimit::DEFAULT.value(), LineLimit::value);

    if offset >= total_lines {
        let raw_offset = parsed.offset.map_or(0, LineOffset::raw);
        return (
            ToolOutput::new(format!(
                "The file has {total_lines} lines, but the offset is {raw_offset}. \
                 The file is shorter than the provided offset.",
            )),
            ToolResultStatus::Error,
        );
    }

    let mut result = String::new();
    for (i, line) in content.lines().skip(offset).take(limit).enumerate() {
        let _ = writeln!(result, "{}\t{line}", offset + i + 1);
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
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "mp3", "mp4", "avi", "mov", "mkv",
    "wav", "flac", "ogg", "pdf", "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "exe", "dll", "so",
    "dylib", "o", "a", "class", "pyc", "wasm", "bin", "dat", "db", "sqlite", "sqlite3",
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
async fn check_file_size(path: &str) -> std::result::Result<(), crate::types::AppError> {
    let metadata = tokio::fs::metadata(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            crate::types::AppError::FsValidation {
                message: String::new(),
            }
        } else {
            crate::types::AppError::FsValidation {
                message: format!("Error reading file metadata: {e}"),
            }
        }
    })?;

    let size = metadata.len();
    let max = MaxReadFileSize::DEFAULT.value();
    if size > max {
        Err(crate::types::AppError::FileTooLarge {
            message: format!(
                "File is too large to read: {} (max {}).",
                FileSizeBytes::from_u64(size),
                FileSizeBytes::from_u64(max),
            ),
        })
    } else {
        Ok(())
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
    let mut msg = format!("File not found: {path}. The current working directory is {cwd}.",);
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
fn find_case_insensitive_match(path: &std::path::Path, parent: &std::path::Path) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LineOffset;

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
        assert_eq!(FileSizeBytes::new(500).to_string(), "500 bytes");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(FileSizeBytes::new(2048).to_string(), "2.0 KB");
    }

    #[test]
    fn format_size_mb() {
        assert_eq!(FileSizeBytes::new(10_485_760).to_string(), "10.0 MB");
    }

    #[test]
    fn format_size_gb() {
        assert_eq!(FileSizeBytes::new(2_147_483_648).to_string(), "2.0 GB");
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
}
