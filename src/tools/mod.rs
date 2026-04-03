//! Built-in tools — mirrors the TS `src/tools/` directory.
//!
//! Each tool follows the same pattern: validate input → execute → return result.
//! Tool dispatch is exhaustive via `BuiltinTool` enum — no string matching.

pub(crate) mod bash;
pub(crate) mod edit;
pub(crate) mod glob;
pub(crate) mod grep;
pub mod helpers;
pub(crate) mod read;
pub(crate) mod webfetch;
pub(crate) mod write;

use crate::types::{
    BuiltinTool, ToolDefinition, ToolName, ToolOutput, ToolResultStatus, WorkingDir,
};

// Re-export shared utilities that other modules need.
#[allow(unused_imports)]
pub use helpers::{detect_file_encoding, encode_for_write, expand_path};

/// Execute a tool by name with given input. Returns `(result_text, status)`.
/// Mirrors the TS `runTools()` in `src/services/tools/toolOrchestration.ts`.
pub async fn execute_tool(
    name: &ToolName,
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let Some(tool) = BuiltinTool::from_name(name) else {
        return (
            ToolOutput::new(format!("Unknown tool: {name}")),
            ToolResultStatus::Error,
        );
    };

    match tool {
        BuiltinTool::Bash => bash::execute_bash(input, cwd).await,
        BuiltinTool::Read => read::execute_read(input, cwd).await,
        BuiltinTool::Write => write::execute_write(input, cwd).await,
        BuiltinTool::Edit => edit::execute_edit(input, cwd).await,
        BuiltinTool::Glob => glob::execute_glob(input, cwd).await,
        BuiltinTool::Grep => grep::execute_grep(input, cwd).await,
        BuiltinTool::WebFetch => webfetch::execute_webfetch(input).await,
    }
}

/// Get tool definitions for the API. Mirrors `src/tools/*/prompt.ts`.
#[must_use]
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        bash::bash_definition(),
        read::read_definition(),
        write::write_definition(),
        edit::edit_definition(),
        glob::glob_definition(),
        grep::grep_definition(),
        webfetch::webfetch_definition(),
    ]
}
