//! Custom Markdown → ratatui `Text` renderer.
//!
//! Covers: headings (bold, no `#`), code blocks (syntax highlighting + background),
//! inline code, bold, italic, strikethrough, lists, blockquotes, tables,
//! horizontal rules, links, task lists.
//!
//! Uses a `MarkdownRenderer` struct to keep state, avoiding a single 300-line function.
//! Syntax highlighting powered by `syntect` + `ansi_to_tui`.

use std::sync::LazyLock;

use ansi_to_tui::IntoText;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
    util::{LinesWithEndings, as_24_bit_terminal_escaped},
};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Heading style by depth (1-indexed). All bold; H1 gets underline.
fn heading_style(level: u8) -> Style {
    match level {
        1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        _ => Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::ITALIC),
    }
}

const CODE_BG: Color = Color::Rgb(40, 40, 40);
const CODE_STYLE: Style = Style::new().fg(Color::White).bg(CODE_BG);
const INLINE_CODE_STYLE: Style = Style::new().fg(Color::White).bg(CODE_BG);
const BLOCKQUOTE_STYLE: Style = Style::new().fg(Color::Green);
const LINK_STYLE: Style = Style::new()
    .fg(Color::Blue)
    .add_modifier(Modifier::UNDERLINED);
const RULE_STYLE: Style = Style::new().fg(Color::DarkGray);

// ─── Bool replacements — two-variant enums ──────────────────────

/// Whether the renderer is currently inside a fenced code block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CodeBlockState {
    #[default]
    Outside,
    Inside,
}

/// Whether the renderer is currently inside a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TableState {
    #[default]
    Outside,
    Inside,
}

/// Whether the current table row is the header row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TableRowKind {
    #[default]
    Body,
    Head,
}

/// Whether a blank line separator should be inserted before the next block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PendingSeparator {
    #[default]
    None,
    BlankLine,
}

/// Stateful markdown renderer that builds ratatui `Line`s from pulldown-cmark events.
struct MarkdownRenderer<'hl> {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    code_block: CodeBlockState,
    code_highlighter: Option<HighlightLines<'hl>>,
    list_stack: Vec<Option<u64>>,
    blockquote_depth: usize,
    link_url: Option<String>,
    pending_separator: PendingSeparator,
    table: TableState,
    table_rows: Vec<Vec<Vec<Span<'static>>>>,
    current_row: Vec<Vec<Span<'static>>>,
    current_cell: Vec<Span<'static>>,
    table_row_kind: TableRowKind,
}

impl MarkdownRenderer<'_> {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            style_stack: Vec::new(),
            code_block: CodeBlockState::Outside,
            code_highlighter: None,
            list_stack: Vec::new(),
            blockquote_depth: 0,
            link_url: None,
            pending_separator: PendingSeparator::None,
            table: TableState::Outside,
            table_rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: Vec::new(),
            table_row_kind: TableRowKind::Body,
        }
    }

    /// Flush current spans into a line with blockquote prefixes.
    fn flush_line(&mut self) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for _ in 0..self.blockquote_depth {
            spans.push(Span::styled("│ ", BLOCKQUOTE_STYLE));
        }
        spans.append(&mut self.current_spans);
        self.lines.push(Line::from(spans));
    }

    /// Insert a blank line separator if needed.
    fn maybe_blank_line(&mut self) {
        if self.pending_separator == PendingSeparator::BlankLine && !self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        self.pending_separator = PendingSeparator::None;
    }

    /// Get the current style from the stack (or default).
    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn handle_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.maybe_blank_line();
                let lvl = heading_level_to_u8(level);
                self.style_stack.push(heading_style(lvl));
            }
            Tag::Paragraph => {
                self.maybe_blank_line();
            }
            Tag::CodeBlock(kind) => {
                self.maybe_blank_line();
                self.code_block = CodeBlockState::Inside;
                if let CodeBlockKind::Fenced(ref lang) = kind {
                    if let Some(syntax) = SYNTAX_SET.find_syntax_by_token(lang.as_ref()) {
                        let theme = &THEME_SET.themes["base16-ocean.dark"];
                        self.code_highlighter = Some(HighlightLines::new(syntax, theme));
                    }
                }
            }
            Tag::BlockQuote(_) => {
                self.maybe_blank_line();
                self.blockquote_depth += 1;
            }
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.maybe_blank_line();
                }
                self.list_stack.push(start);
            }
            Tag::Item => {
                let indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
                if let Some(last) = self.list_stack.last_mut() {
                    match last {
                        None => {
                            self.current_spans.push(Span::raw(format!("{indent}• ")));
                        }
                        Some(n) => {
                            self.current_spans.push(Span::styled(
                                format!("{indent}{n}. "),
                                Style::default().fg(Color::LightBlue),
                            ));
                            *n += 1;
                        }
                    }
                }
            }
            Tag::Emphasis => {
                let base = self.current_style();
                self.style_stack.push(base.add_modifier(Modifier::ITALIC));
            }
            Tag::Strong => {
                let base = self.current_style();
                self.style_stack.push(base.add_modifier(Modifier::BOLD));
            }
            Tag::Strikethrough => {
                let base = self.current_style();
                self.style_stack
                    .push(base.add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
                let base = self.current_style();
                self.style_stack.push(base.patch(LINK_STYLE));
            }
            Tag::Table(_) => {
                self.maybe_blank_line();
                self.table = TableState::Inside;
                self.table_rows.clear();
            }
            Tag::TableHead => {
                self.table_row_kind = TableRowKind::Head;
                self.current_row.clear();
            }
            Tag::TableRow => {
                self.current_row.clear();
            }
            Tag::TableCell => {
                self.current_cell.clear();
            }
            _ => {}
        }
    }

    fn handle_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.flush_line();
                self.style_stack.pop();
                self.pending_separator = PendingSeparator::BlankLine;
            }
            TagEnd::Paragraph => {
                self.flush_line();
                self.pending_separator = PendingSeparator::BlankLine;
            }
            TagEnd::CodeBlock => {
                self.code_block = CodeBlockState::Outside;
                self.code_highlighter = None;
                self.pending_separator = PendingSeparator::BlankLine;
            }
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pending_separator = PendingSeparator::BlankLine;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.pending_separator = PendingSeparator::BlankLine;
                }
            }
            TagEnd::Item => {
                self.flush_line();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::Link => {
                self.style_stack.pop();
                if let Some(url) = self.link_url.take() {
                    self.current_spans.push(Span::raw(" ("));
                    self.current_spans.push(Span::styled(url, LINK_STYLE));
                    self.current_spans.push(Span::raw(")"));
                }
            }
            TagEnd::Table => {
                render_table(&self.table_rows, &mut self.lines);
                self.table_rows.clear();
                self.table = TableState::Outside;
                self.pending_separator = PendingSeparator::BlankLine;
            }
            TagEnd::TableHead => {
                // Cells already pushed by End(TableCell) — just save the row.
                self.table_rows.push(self.current_row.clone());
                self.current_row.clear();
                self.table_row_kind = TableRowKind::Body;
            }
            TagEnd::TableRow => {
                self.table_rows.push(self.current_row.clone());
                self.current_row.clear();
            }
            TagEnd::TableCell => {
                self.current_row.push(self.current_cell.clone());
                self.current_cell.clear();
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str) {
        if self.table == TableState::Inside {
            let style = if self.table_row_kind == TableRowKind::Head {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                self.current_style()
            };
            self.current_cell
                .push(Span::styled(text.to_string(), style));
        } else if self.code_block == CodeBlockState::Inside {
            self.handle_code_block_text(text);
        } else {
            let style = self.current_style();
            self.current_spans
                .push(Span::styled(text.to_string(), style));
        }
    }

    fn handle_code_block_text(&mut self, text: &str) {
        if self.code_highlighter.is_some() {
            self.handle_highlighted_code(text);
        } else {
            for line_text in text.lines() {
                self.push_plain_code_line(line_text);
            }
        }
    }

    /// Render syntax-highlighted code using `syntect` + `ansi_to_tui`.
    fn handle_highlighted_code(&mut self, text: &str) {
        let highlighter = self.code_highlighter.as_mut().unwrap();
        let depth = self.blockquote_depth;

        for line_text in LinesWithEndings::from(text) {
            let Ok(regions) = highlighter.highlight_line(line_text, &SYNTAX_SET) else {
                push_plain_code_line_to(&mut self.lines, depth, line_text.trim_end_matches('\n'));
                continue;
            };
            let escaped = as_24_bit_terminal_escaped(&regions, false);
            let Ok(ansi_text) = escaped.into_text() else {
                push_plain_code_line_to(&mut self.lines, depth, line_text.trim_end_matches('\n'));
                continue;
            };
            for line in ansi_text.lines {
                let mut spans: Vec<Span<'static>> = Vec::new();
                for _ in 0..depth {
                    spans.push(Span::styled("│ ", BLOCKQUOTE_STYLE));
                }
                spans.push(Span::styled(" ", Style::default().bg(CODE_BG)));
                for s in line.spans {
                    spans.push(Span::styled(
                        s.content.to_string(),
                        s.style.bg(CODE_BG),
                    ));
                }
                spans.push(Span::styled(" ", Style::default().bg(CODE_BG)));
                self.lines.push(Line::from(spans));
            }
        }
    }

    fn push_plain_code_line(&mut self, line_text: &str) {
        push_plain_code_line_to(&mut self.lines, self.blockquote_depth, line_text);
    }

    fn handle_code_inline(&mut self, code: &str) {
        if self.table == TableState::Inside {
            self.current_cell
                .push(Span::styled(code.to_string(), INLINE_CODE_STYLE));
        } else {
            self.current_spans
                .push(Span::styled(code.to_string(), INLINE_CODE_STYLE));
        }
    }

    fn handle_soft_break(&mut self) {
        if self.table == TableState::Inside {
            self.current_cell.push(Span::raw(" "));
        } else {
            self.current_spans.push(Span::raw(" "));
        }
    }

    fn handle_hard_break(&mut self) {
        if self.table != TableState::Inside {
            self.flush_line();
        }
    }

    fn handle_rule(&mut self) {
        if !self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        self.lines.push(Line::from(Span::styled(
            "────────────────────────────────────────",
            RULE_STYLE,
        )));
        self.pending_separator = PendingSeparator::BlankLine;
    }

    fn handle_task_list_marker(&mut self, checked: bool) {
        let marker = if checked { "☑ " } else { "☐ " };
        let color = if checked {
            Color::Green
        } else {
            Color::DarkGray
        };
        self.current_spans
            .push(Span::styled(marker, Style::default().fg(color)));
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current_spans.is_empty() {
            self.flush_line();
        }
        self.lines
    }
}

/// Push a single plain (unhighlighted) code line with background and blockquote prefixes.
fn push_plain_code_line_to(lines: &mut Vec<Line<'static>>, depth: usize, line_text: &str) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for _ in 0..depth {
        spans.push(Span::styled("│ ", BLOCKQUOTE_STYLE));
    }
    spans.push(Span::styled(format!(" {line_text} "), CODE_STYLE));
    lines.push(Line::from(spans));
}

fn heading_level_to_u8(level: pulldown_cmark::HeadingLevel) -> u8 {
    match level {
        pulldown_cmark::HeadingLevel::H1 => 1,
        pulldown_cmark::HeadingLevel::H2 => 2,
        pulldown_cmark::HeadingLevel::H3 => 3,
        pulldown_cmark::HeadingLevel::H4 => 4,
        pulldown_cmark::HeadingLevel::H5 => 5,
        pulldown_cmark::HeadingLevel::H6 => 6,
    }
}

/// Convert markdown text to styled ratatui lines.
pub fn markdown_to_lines(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, opts);
    let events: Vec<Event<'_>> = parser.collect();

    let mut renderer = MarkdownRenderer::new();

    for event in events {
        match event {
            Event::Start(tag) => renderer.handle_start(tag),
            Event::End(tag) => renderer.handle_end(tag),
            Event::Text(text) => renderer.handle_text(&text),
            Event::Code(code) => renderer.handle_code_inline(&code),
            Event::SoftBreak => renderer.handle_soft_break(),
            Event::HardBreak => renderer.handle_hard_break(),
            Event::Rule => renderer.handle_rule(),
            Event::TaskListMarker(checked) => renderer.handle_task_list_marker(checked),
            _ => {}
        }
    }

    renderer.finish()
}

/// Render a table with full box-drawing borders and row separators.
fn render_table(rows: &[Vec<Vec<Span<'static>>>], lines: &mut Vec<Line<'static>>) {
    if rows.is_empty() {
        return;
    }

    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return;
    }

    let widths = compute_column_widths(rows, col_count);
    let bs = Style::default().fg(Color::DarkGray);

    // Top border: ┌─┬─┐
    push_horizontal_border(lines, &widths, col_count, bs, "┌", "┬", "┐");

    for (row_idx, row) in rows.iter().enumerate() {
        // Data row: │ cell │ cell │
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled("│", bs));

        for (col_idx, cell) in row.iter().enumerate() {
            let w = widths.get(col_idx).copied().unwrap_or(0);
            let text_len: usize = cell.iter().map(|s| s.content.width()).sum();
            let padding = w.saturating_sub(text_len);

            spans.push(Span::raw(" "));
            for s in cell {
                spans.push(s.clone());
            }
            spans.push(Span::raw(" ".repeat(padding + 1)));
            spans.push(Span::styled("│", bs));
        }

        // Fill missing columns.
        for col_idx in row.len()..col_count {
            let w = widths.get(col_idx).copied().unwrap_or(0);
            spans.push(Span::raw(" ".repeat(w + 2)));
            spans.push(Span::styled("│", bs));
        }

        lines.push(Line::from(spans));

        // Row separator: ├─┼─┤ (after every row except the last)
        if row_idx + 1 < rows.len() {
            push_horizontal_border(lines, &widths, col_count, bs, "├", "┼", "┤");
        }
    }

    // Bottom border: └─┴─┘
    push_horizontal_border(lines, &widths, col_count, bs, "└", "┴", "┘");
}

/// Compute column widths from cell text character counts.
fn compute_column_widths(rows: &[Vec<Vec<Span<'static>>>], col_count: usize) -> Vec<usize> {
    let mut widths = vec![0_usize; col_count];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let len: usize = cell.iter().map(|s| s.content.width()).sum();
            if len > widths[i] {
                widths[i] = len;
            }
        }
    }
    widths
}

/// Push a horizontal border line: `left ─── mid ─── right`
fn push_horizontal_border(
    lines: &mut Vec<Line<'static>>,
    widths: &[usize],
    col_count: usize,
    style: Style,
    left: &str,
    mid: &str,
    right: &str,
) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(left.to_owned(), style));
    for (i, &w) in widths.iter().enumerate() {
        spans.push(Span::styled("─".repeat(w + 2), style));
        if i + 1 < col_count {
            spans.push(Span::styled(mid.to_owned(), style));
        }
    }
    spans.push(Span::styled(right.to_owned(), style));
    lines.push(Line::from(spans));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_full_borders() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let lines = markdown_to_lines(md);
        let all: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all.contains('┌'), "should have top-left corner");
        assert!(all.contains('┐'), "should have top-right corner");
        assert!(all.contains('└'), "should have bottom-left corner");
        assert!(all.contains('┘'), "should have bottom-right corner");
        assert!(all.contains('┬'), "should have top T-junction");
        assert!(all.contains('┴'), "should have bottom T-junction");
        assert!(all.contains('├'), "should have left T-junction");
        assert!(all.contains('┤'), "should have right T-junction");
        assert!(all.contains('┼'), "should have cross junction");
    }

    #[test]
    fn table_no_phantom_column() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let lines = markdown_to_lines(md);
        // The top border should have exactly 2 columns: ┌─┬─┐ (one ┬)
        let top_line: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        let t_count = top_line.matches('┬').count();
        assert_eq!(t_count, 1, "2-column table should have 1 ┬, got {t_count}");
    }

    #[test]
    fn heading_no_hashes() {
        let lines = markdown_to_lines("# Hello World");
        assert!(!lines.is_empty());
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.contains('#'), "heading should not contain '#' chars");
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn code_block_no_backticks() {
        let lines = markdown_to_lines("```rust\nlet x = 1;\n```");
        for line in &lines {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                !text.contains("```"),
                "code block should not contain backticks: {text}"
            );
        }
    }

    #[test]
    fn code_block_has_bg() {
        let lines = markdown_to_lines("```\nhello\n```");
        let code_line = &lines[0];
        let has_bg = code_line.spans.iter().any(|s| s.style.bg == Some(CODE_BG));
        assert!(has_bg, "code block should have background color");
    }

    #[test]
    fn table_renders() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let lines = markdown_to_lines(md);
        assert!(
            lines.len() >= 3,
            "table should produce at least 3 lines (header + sep + row)"
        );
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            all_text.contains('│'),
            "table should have box-drawing borders"
        );
    }

    #[test]
    fn bold_has_modifier() {
        let lines = markdown_to_lines("hello **world**");
        let bold_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "world");
        assert!(bold_span.is_some());
        assert!(
            bold_span
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn horizontal_rule() {
        let lines = markdown_to_lines("above\n\n---\n\nbelow");
        let has_rule = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains('─')));
        assert!(
            has_rule,
            "should render horizontal rule with box-drawing chars"
        );
    }

    #[test]
    fn blockquote_prefix() {
        let lines = markdown_to_lines("> quoted text");
        let all_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(all_text.contains('│'), "blockquote should have │ prefix");
    }

    #[test]
    fn unordered_list() {
        let lines = markdown_to_lines("- item one\n- item two");
        assert!(lines.len() >= 2);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('•'), "unordered list should use bullet char");
    }

    #[test]
    fn inline_code_styled() {
        let lines = markdown_to_lines("use `foo` here");
        let code_span = lines[0].spans.iter().find(|s| s.content.as_ref() == "foo");
        assert!(code_span.is_some());
        assert_eq!(code_span.unwrap().style.bg, Some(CODE_BG));
    }

    #[test]
    fn task_list() {
        let lines = markdown_to_lines("- [ ] todo\n- [x] done");
        let all: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all.contains('☐'), "unchecked task should show ☐");
        assert!(all.contains('☑'), "checked task should show ☑");
    }

    #[test]
    fn link_shows_url() {
        let lines = markdown_to_lines("[click](https://example.com)");
        let all: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(all.contains("https://example.com"));
    }

    #[test]
    fn syntax_highlighted_code_has_colored_spans() {
        let lines = markdown_to_lines("```rust\nlet x = 42;\n```");
        assert!(!lines.is_empty());
        // Highlighted code should have more than one span per line (keywords get distinct colors).
        let code_line = &lines[0];
        let colored_spans = code_line
            .spans
            .iter()
            .filter(|s| s.style.fg.is_some() || s.style.bg.is_some())
            .count();
        assert!(
            colored_spans >= 2,
            "highlighted code should have multiple colored spans, got {colored_spans}"
        );
    }

    #[test]
    fn unknown_lang_falls_back_to_plain() {
        let lines = markdown_to_lines("```xyznotreal\nhello world\n```");
        assert!(!lines.is_empty());
        let has_bg = lines[0].spans.iter().any(|s| s.style.bg == Some(CODE_BG));
        assert!(has_bg, "unknown lang should fall back to plain code style");
    }
}
