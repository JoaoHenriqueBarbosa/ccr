//! Edit tool — mirrors `src/tools/FileEditTool/FileEditTool.ts`.

use super::helpers::{detect_file_encoding, encode_for_write};
use crate::types::{
    EditInput, FileEncoding, LineEndings, ToolDefinition, ToolOutput, ToolResultStatus, WorkingDir,
};

pub(crate) fn edit_definition() -> ToolDefinition {
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

pub(crate) async fn execute_edit(
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: EditInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid Edit input: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    let path = match cwd.validate_path(parsed.file_path.as_ref()) {
        Ok(p) => p,
        Err(e) => return (ToolOutput::new(format!("{e}")), ToolResultStatus::Error),
    };

    // Reject no-op edits early
    if parsed.old_string.as_ref() == parsed.new_string.as_ref() {
        return (
            ToolOutput::new(
                "No changes to make: old_string and new_string are exactly the same.".into(),
            ),
            ToolResultStatus::Error,
        );
    }

    let (file_content, encoding, endings) = match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let (enc, end) = detect_file_encoding(&bytes);
            (Some(decode_file_bytes(&bytes)), enc, end)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (None, FileEncoding::Utf8, LineEndings::Lf)
        }
        Err(e) => {
            return (
                ToolOutput::new(format!("Error reading {path}: {e}")),
                ToolResultStatus::Error,
            );
        }
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
        return (
            ToolOutput::new(format!("Error creating directories: {e}")),
            ToolResultStatus::Error,
        );
    }

    let encoded = encode_for_write(&updated, encoding, endings);
    match tokio::fs::write(&path, &encoded).await {
        Ok(()) => (
            ToolOutput::new(format!("The file {path} has been updated successfully.")),
            ToolResultStatus::Success,
        ),
        Err(e) => (
            ToolOutput::new(format!("Error writing {path}: {e}")),
            ToolResultStatus::Error,
        ),
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
                return (
                    ToolOutput::new(format!("Error creating directories: {e}")),
                    ToolResultStatus::Error,
                );
            }
            match tokio::fs::write(path, parsed.new_string.as_ref()).await {
                Ok(()) => (
                    ToolOutput::new(format!("Created new file {path}")),
                    ToolResultStatus::Success,
                ),
                Err(e) => (
                    ToolOutput::new(format!("Error writing {path}: {e}")),
                    ToolResultStatus::Error,
                ),
            }
        }
        // File exists but is empty → replace with new content
        Some(c) if c.trim().is_empty() => {
            match tokio::fs::write(path, parsed.new_string.as_ref()).await {
                Ok(()) => (
                    ToolOutput::new(format!("The file {path} has been updated successfully.")),
                    ToolResultStatus::Success,
                ),
                Err(e) => (
                    ToolOutput::new(format!("Error writing {path}: {e}")),
                    ToolResultStatus::Error,
                ),
            }
        }
        // File exists with content → error
        Some(_) => (
            ToolOutput::new("Cannot create new file - file already exists.".into()),
            ToolResultStatus::Error,
        ),
    }
}

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
    // Normalization changes byte lengths (curly quotes 3 bytes → straight 1 byte),
    // so map from normalized char offset back to original byte offset.
    let char_offset = normalized_file[..idx].chars().count();
    let char_len = normalized_search.chars().count();

    let start: usize = file_content
        .chars()
        .take(char_offset)
        .map(char::len_utf8)
        .sum();
    let len: usize = file_content[start..]
        .chars()
        .take(char_len)
        .map(char::len_utf8)
        .sum();
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
            result.push(if is_opening_context(&chars, i) {
                '\u{201C}'
            } else {
                '\u{201D}'
            });
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
                result.push(if is_opening_context(&chars, i) {
                    '\u{2018}'
                } else {
                    '\u{2019}'
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EditInput;

    // ── normalize_quotes ──

    #[test]
    fn normalize_quotes_straight_unchanged() {
        assert_eq!(normalize_quotes("hello 'world'"), "hello 'world'");
    }

    #[test]
    fn normalize_quotes_curly_to_straight() {
        assert_eq!(
            normalize_quotes("he said \u{201C}hello\u{201D}"),
            "he said \"hello\""
        );
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
        let result = preserve_quote_style("said \"hi\"", "said \u{201C}hi\u{201D}", "said \"bye\"");
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
}
