//! Write tool — mirrors `src/tools/FileWriteTool/FileWriteTool.ts`.

use super::helpers::{detect_file_encoding, encode_for_write, expand_path};
use crate::types::{
    FileEncoding, FileSizeBytes, LineEndings, MaxWriteFileSize, ToolDefinition, ToolOutput,
    ToolResultStatus, WorkingDir, WriteInput,
};

pub(crate) fn write_definition() -> ToolDefinition {
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

pub(crate) async fn execute_write(
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: WriteInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid Write input: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    // Size guard
    if parsed.content.as_ref().len() > MaxWriteFileSize::DEFAULT.as_bytes() {
        let actual = FileSizeBytes::new(parsed.content.as_ref().len());
        let limit = FileSizeBytes::new(MaxWriteFileSize::DEFAULT.as_bytes());
        return (
            ToolOutput::new(format!(
                "Content too large ({actual}); max allowed is {limit}"
            )),
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
        tokio::fs::read(file_path)
            .await
            .map_or((FileEncoding::Utf8, LineEndings::Lf), |bytes| {
                detect_file_encoding(&bytes)
            })
    } else {
        (FileEncoding::Utf8, LineEndings::Lf)
    };

    if let Some(parent) = file_path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return (
            ToolOutput::new(format!("Error creating directories: {e}")),
            ToolResultStatus::Error,
        );
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
        Err(e) => (
            ToolOutput::new(format!("Error writing {path}: {e}")),
            ToolResultStatus::Error,
        ),
    }
}
