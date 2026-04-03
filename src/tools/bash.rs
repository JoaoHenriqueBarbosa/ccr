//! Bash tool — mirrors `src/tools/BashTool/BashTool.ts`.

use std::fmt::Write;
use tokio::process::Command;

use super::helpers::{is_benign_exit, persist_large_output, strip_blank_lines, truncate_output};
use crate::types::{
    BashInput, ExecutionMode, ExitCode, LargeOutputThreshold, MaxOutputLen, TimeoutMs,
    ToolDefinition, ToolOutput, ToolResultStatus, WorkingDir,
};

pub(crate) fn bash_definition() -> ToolDefinition {
    ToolDefinition {
        name: "Bash".into(),
        description: "Executes a bash command and returns its output.\n\n\
            The working directory persists between commands, but shell state does not.\n\
            Avoid using this for tasks that have dedicated tools (Read, Edit, Grep, Glob).\n\
            Default timeout: 120000ms. Max: 600000ms."
            .into(),
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

/// Mirrors `src/tools/BashTool/BashTool.ts`.
pub(crate) async fn execute_bash(
    input: &serde_json::Value,
    cwd: &WorkingDir,
) -> (ToolOutput, ToolResultStatus) {
    let parsed: BashInput = match serde_json::from_value(input.clone()) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid Bash input: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    // Stub: background execution not yet implemented
    if parsed
        .run_in_background
        .is_some_and(ExecutionMode::is_enabled)
    {
        return (
            ToolOutput::new("Background execution not yet implemented".into()),
            ToolResultStatus::Error,
        );
    }

    let timeout = parsed
        .timeout
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
        Ok(Err(e)) => (
            ToolOutput::new(format!("Command execution failed: {e}")),
            ToolResultStatus::Error,
        ),
        Err(_) => (
            ToolOutput::new(format!("Command timed out after {}ms", timeout.as_millis())),
            ToolResultStatus::Error,
        ),
    }
}

/// Format stdout + stderr from a completed command.
///
/// Applies: empty-line stripping, exit code annotation, command-semantic
/// exit code interpretation, output truncation, and large output persistence.
pub(crate) fn format_command_output(
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
