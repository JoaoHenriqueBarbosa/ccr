use super::*;
use proptest::prelude::*;

// ─── InputBuffer property tests ─────────────────────────────

proptest! {
    /// Any sequence of insert + backspace never panics on UTF-8 boundaries.
    #[test]
    fn input_buffer_insert_backspace_never_panics(s in "\\PC{0,100}") {
        let mut buf = InputBuffer::default();
        for c in s.chars() {
            buf.insert(c);
        }
        // Backspace everything
        for _ in 0..s.len() + 10 {
            buf.backspace();
        }
        prop_assert!(buf.is_empty());
    }

    /// Insert then take always returns the original string.
    #[test]
    fn input_buffer_insert_take_roundtrip(s in "\\PC{0,100}") {
        let mut buf = InputBuffer::default();
        for c in s.chars() {
            buf.insert(c);
        }
        prop_assert_eq!(buf.take(), s);
    }

    /// Move left then right from any position is stable.
    #[test]
    fn input_buffer_cursor_navigation_stable(
        s in "\\PC{1,50}",
        moves_left in 0usize..100,
        moves_right in 0usize..100,
    ) {
        let mut buf = InputBuffer::default();
        for c in s.chars() {
            buf.insert(c);
        }
        for _ in 0..moves_left {
            buf.move_left();
        }
        for _ in 0..moves_right {
            buf.move_right();
        }
        // Should never panic — cursor always on valid boundary
        let _ = buf.text();
        let _ = buf.display_cursor_offset();
    }

    /// Insert in the middle produces correct text.
    #[test]
    fn input_buffer_mid_insert(
        prefix in "[a-z]{1,10}",
        suffix in "[a-z]{1,10}",
        middle in "[A-Z]",
    ) {
        let mut buf = InputBuffer::default();
        for c in prefix.chars() {
            buf.insert(c);
        }
        for c in suffix.chars() {
            buf.insert(c);
        }
        // Move left by suffix length
        for _ in 0..suffix.len() {
            buf.move_left();
        }
        // Insert middle char
        let mid_char = middle.chars().next().unwrap();
        buf.insert(mid_char);
        let result = buf.take();
        let expected = format!("{prefix}{mid_char}{suffix}");
        prop_assert_eq!(result, expected);
    }
}

// ─── InputBuffer unit tests ─────────────────────────────────

#[test]
fn input_buffer_empty() {
    let buf = InputBuffer::default();
    assert!(buf.is_empty());
    assert_eq!(buf.text(), "");
    assert_eq!(buf.display_cursor_offset(), TermCols::default());
}

#[test]
fn input_buffer_multibyte_chars() {
    let mut buf = InputBuffer::default();
    buf.insert('é');
    buf.insert('ñ');
    buf.insert('中');
    assert_eq!(buf.text(), "éñ中");

    buf.backspace();
    assert_eq!(buf.text(), "éñ");

    buf.move_left();
    buf.backspace();
    assert_eq!(buf.text(), "ñ");
}

#[test]
fn input_buffer_clear_resets_cursor() {
    let mut buf = InputBuffer::default();
    buf.insert('a');
    buf.insert('b');
    buf.clear();
    assert!(buf.is_empty());
    buf.insert('c');
    assert_eq!(buf.text(), "c");
}

// ─── Newtype tests ──────────────────────────────────────────

#[test]
fn tool_result_status_serde_roundtrip() {
    let success = ToolResultStatus::Success;
    let error = ToolResultStatus::Error;

    let s_json = serde_json::to_value(success).unwrap();
    let e_json = serde_json::to_value(error).unwrap();

    assert_eq!(s_json, serde_json::Value::Bool(false));
    assert_eq!(e_json, serde_json::Value::Bool(true));

    let s_back: ToolResultStatus = serde_json::from_value(s_json).unwrap();
    let e_back: ToolResultStatus = serde_json::from_value(e_json).unwrap();

    assert_eq!(s_back, ToolResultStatus::Success);
    assert_eq!(e_back, ToolResultStatus::Error);
}

#[test]
fn token_count_display() {
    let mut tc = TokenCount::default();
    assert!(tc.is_empty());

    let usage = Usage {
        input_tokens: ApiTokens::default(),
        output_tokens: ApiTokens::default(),
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
        server_tool_use: None,
    };
    tc.add(&usage);
    assert!(tc.is_empty());

    let usage_json = serde_json::json!({
        "input_tokens": 5000,
        "output_tokens": 3000
    });
    let usage: Usage = serde_json::from_value(usage_json).unwrap();
    tc.add(&usage);
    assert!(!tc.is_empty());
    assert_eq!(format!("{tc}"), "8k tokens");
}

#[test]
fn scroll_offset_functional() {
    let s = ScrollOffset::default();
    assert_eq!(s.value(), 0);

    let s = s.scroll_up(5);
    assert_eq!(s.value(), 5);

    let s = s.scroll_down(3);
    assert_eq!(s.value(), 2);

    let s = s.scroll_down(100); // saturating
    assert_eq!(s.value(), 0);
}

#[test]
fn turn_timer_typestate() {
    let t = TurnTimer::default();
    assert!(t.elapsed_ms().is_none());
    assert!(t.completed_secs().is_none());

    let t = TurnTimer::start();
    assert!(t.elapsed_ms().is_some());
    assert!(t.completed_secs().is_none());

    let t = t.finish();
    assert!(t.elapsed_ms().is_none());
    assert!(t.completed_secs().is_some());
}

#[test]
fn turn_timer_timing_struct() {
    let timing = TimingTimer::new();
    let _elapsed = timing.elapsed_ms(); // just verify it does not panic
    let completed = timing.finish();
    assert!(completed.as_secs_f64() >= 0.0);
}

#[test]
fn term_rows_arithmetic() {
    let a = TermRows::from(10);
    let b = TermRows::from(3);
    assert_eq!(a + b, TermRows::from(13));
    assert_eq!(a.saturating_sub(b), TermRows::from(7));
    assert_eq!(a.saturating_sub(TermRows::from(20)), TermRows::default());
    assert_eq!(a.min(b), b);
}

#[test]
fn term_rows_add_saturates() {
    let a = TermRows::from(u16::MAX);
    let b = TermRows::from(1);
    assert_eq!(a + b, TermRows::from(u16::MAX)); // saturating, not panic
}

#[test]
fn term_rows_sum_saturates() {
    let items = vec![TermRows::from(u16::MAX), TermRows::from(100)];
    let total: TermRows = items.into_iter().sum();
    assert_eq!(total, TermRows::from(u16::MAX));
}

#[test]
fn device_identity_default() {
    let identity = DeviceIdentity::default();
    assert!(identity.device_id.is_empty());
}

#[test]
fn tool_output_display() {
    let output = ToolOutput::new("hello world".into());
    assert_eq!(format!("{output}"), "hello world");
    assert_eq!(output.as_ref(), "hello world");
}

#[test]
fn builtin_tool_from_name() {
    assert_eq!(
        BuiltinTool::from_name(&ToolName::from("Bash")),
        Some(BuiltinTool::Bash)
    );
    assert_eq!(
        BuiltinTool::from_name(&ToolName::from("Read")),
        Some(BuiltinTool::Read)
    );
    assert_eq!(
        BuiltinTool::from_name(&ToolName::from("Write")),
        Some(BuiltinTool::Write)
    );
    assert_eq!(
        BuiltinTool::from_name(&ToolName::from("Glob")),
        Some(BuiltinTool::Glob)
    );
    assert_eq!(
        BuiltinTool::from_name(&ToolName::from("Grep")),
        Some(BuiltinTool::Grep)
    );
    assert_eq!(BuiltinTool::from_name(&ToolName::from("Unknown")), None);
}

#[test]
fn max_tokens_value() {
    assert_eq!(MaxTokens::DEFAULT.value(), 64000);
}

// ─── TermCols tests ────────────────────────────────────────

#[test]
fn term_cols_basic() {
    let c = TermCols::from(42);
    assert_eq!(c.as_u16(), 42);
    assert_eq!(c, TermCols::from(42));
    assert_ne!(c, TermCols::default());
}

#[test]
fn input_buffer_cursor_offset_is_cols() {
    let mut buf = InputBuffer::default();
    buf.insert('a');
    buf.insert('b');
    buf.insert('c');
    let offset = buf.display_cursor_offset();
    assert_eq!(offset, TermCols::from(3));
}

// ─── Path traversal tests ──────────────────────────────────

#[test]
fn validate_path_inside_cwd() {
    let cwd = WorkingDir::current();
    let result = cwd.validate_path("Cargo.toml");
    assert!(result.is_ok());
}

#[test]
fn validate_path_traversal_blocked() {
    let cwd = WorkingDir::current();
    let result = cwd.validate_path("/etc/passwd");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, AppError::PathTraversal { .. }));
}

#[test]
fn validate_path_tmp_allowed() {
    let cwd = WorkingDir::current();
    let result = cwd.validate_path("/tmp/test_file.txt");
    assert!(result.is_ok());
}

#[test]
fn validate_path_dev_allowed() {
    let cwd = WorkingDir::current();
    let result = cwd.validate_path("/dev/null");
    assert!(result.is_ok());
}

#[test]
fn validate_path_dotdot_blocked() {
    let cwd = WorkingDir::current();
    // This goes to parent, which may be outside cwd
    let result = cwd.validate_path("../../../../../../etc/passwd");
    assert!(result.is_err());
}

// ─── Tool input newtype tests ──────────────────────────────

#[test]
fn bash_input_parses_typed() {
    let json = serde_json::json!({
        "command": "echo hello",
        "timeout": 5000
    });
    let input: BashInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.command.as_ref(), "echo hello");
    assert_eq!(input.timeout.unwrap().as_millis(), 5000);
}

#[test]
fn bash_input_timeout_clamped() {
    let json = serde_json::json!({ "command": "x", "timeout": 999_999 });
    let input: BashInput = serde_json::from_value(json).unwrap();
    let clamped = input.timeout.unwrap().clamped();
    assert_eq!(clamped.as_millis(), TimeoutMs::MAX.as_millis());
}

#[test]
fn read_input_parses_typed() {
    let json = serde_json::json!({
        "file_path": "/tmp/test.txt",
        "offset": 10,
        "limit": 50
    });
    let input: ReadInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.file_path.as_ref(), "/tmp/test.txt");
    // 1-based input: offset=10 → internal 0-based value=9
    assert_eq!(input.offset.unwrap().value(), 9);
    assert_eq!(input.offset.unwrap().raw(), 10);
    assert_eq!(input.limit.unwrap().value(), 50);
}

#[test]
fn write_input_parses_typed() {
    let json = serde_json::json!({
        "file_path": "/tmp/out.txt",
        "content": "hello\nworld"
    });
    let input: WriteInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.file_path.as_ref(), "/tmp/out.txt");
    assert_eq!(input.content.as_ref(), "hello\nworld");
}

#[test]
fn glob_input_parses_typed() {
    let json = serde_json::json!({
        "pattern": "*.rs",
        "path": "/tmp"
    });
    let input: GlobInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.pattern.as_ref(), "*.rs");
    assert_eq!(input.path.unwrap().as_ref(), "/tmp");
}

#[test]
fn grep_input_parses_typed() {
    let json = serde_json::json!({
        "pattern": "fn main",
        "path": "/tmp",
        "glob": "*.rs"
    });
    let input: GrepInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.pattern.as_ref(), "fn main");
    assert_eq!(input.path.unwrap().as_ref(), "/tmp");
    assert_eq!(input.glob.unwrap().as_ref(), "*.rs");
}

// ─── Role equality tests ───────────────────────────────────

#[test]
fn role_eq() {
    assert_eq!(Role::User, Role::User);
    assert_eq!(Role::Assistant, Role::Assistant);
    assert_ne!(Role::User, Role::Assistant);
}

// ─── TimeoutMs constants ───────────────────────────────────

#[test]
fn timeout_ms_defaults() {
    assert_eq!(TimeoutMs::DEFAULT.as_millis(), 120_000);
    assert_eq!(TimeoutMs::MAX.as_millis(), 600_000);
}

#[test]
fn line_limit_default() {
    assert_eq!(LineLimit::DEFAULT.value(), 2000);
}
