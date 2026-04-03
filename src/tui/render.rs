//! Rendering — typestate pipeline and draw functions.
//!
//! Pipeline: `PreparedBlocks → MeasuredBlocks → ViewportSlice → render()`

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::types::{
    ContentBlock, ConversationMessage, Role, ScrollOffset, TermRows,
};

use super::state::App;

// ─── Render pipeline (typestate) ─────────────────────────────────

/// Whether a message block has a blank line above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
enum Spacing {
    Flush,
    MarginTop,
}

/// A renderable message segment with optional background fill.
struct MessageBlock {
    lines: Vec<Line<'static>>,
    bg: Option<Color>,
    spacing: Spacing,
}

/// Phase 1: blocks built from app state, not yet measured.
struct PreparedBlocks {
    blocks: Vec<MessageBlock>,
}

/// Phase 2: blocks with computed heights, total known.
struct MeasuredBlocks {
    blocks: Vec<MessageBlock>,
    heights: Vec<TermRows>,
    total_height: TermRows,
}

/// Phase 3: viewport-clipped slice ready for rendering.
struct ViewportSlice {
    blocks: Vec<MessageBlock>,
    heights: Vec<TermRows>,
    total_height: TermRows,
    visible: TermRows,
    scroll: TermRows,
    max_scroll: TermRows,
}

impl PreparedBlocks {
    #[allow(clippy::cast_possible_truncation)]
    fn measure(self, width: u16) -> MeasuredBlocks {
        let heights: Vec<TermRows> = self.blocks.iter().map(|b| {
            let margin = u16::from(b.spacing == Spacing::MarginTop);
            let text = Text::from(b.lines.clone());
            let content = Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .line_count(width) as u16;
            TermRows::from(margin + content)
        }).collect();
        let total_height = heights.iter().copied().sum();
        MeasuredBlocks { blocks: self.blocks, heights, total_height }
    }
}

impl MeasuredBlocks {
    fn clip_to_viewport(self, visible: u16, scroll_offset: ScrollOffset) -> ViewportSlice {
        let visible = TermRows::from(visible);
        let max_scroll = self.total_height.saturating_sub(visible);
        let scroll = max_scroll.saturating_sub(TermRows::from(scroll_offset.value()));
        ViewportSlice {
            blocks: self.blocks,
            heights: self.heights,
            total_height: self.total_height,
            visible,
            scroll,
            max_scroll,
        }
    }
}

impl ViewportSlice {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn render(self, f: &mut Frame, area: Rect) {
        let mut y: i32 = -(self.scroll.as_i32());

        for (block, &h_total) in self.blocks.iter().zip(&self.heights) {
            if block.spacing == Spacing::MarginTop {
                y += 1;
            }
            let margin = TermRows::from(u16::from(block.spacing == Spacing::MarginTop));
            let h = h_total.saturating_sub(margin);

            if y + h.as_i32() <= 0 {
                y += h.as_i32();
                continue;
            }
            if y >= self.visible.as_i32() {
                break;
            }

            let render_y = TermRows::from(y.max(0) as u16);
            let clip_top = TermRows::from((-y).max(0) as u16);
            let render_h = h.saturating_sub(clip_top).min(self.visible.saturating_sub(render_y));

            if render_h == TermRows::default() {
                y += h.as_i32();
                continue;
            }

            let rect = Rect {
                x: area.x,
                y: area.y + render_y.as_u16(),
                width: area.width,
                height: render_h.as_u16(),
            };

            let text = Text::from(block.lines.clone());
            let mut paragraph = Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((clip_top.as_u16(), 0));

            if let Some(bg) = block.bg {
                paragraph = paragraph.style(Style::default().bg(bg));
            }

            f.render_widget(paragraph, rect);
            y += h.as_i32();
        }

        if self.total_height > self.visible {
            let mut scrollbar_state = ScrollbarState::new(self.max_scroll.as_usize())
                .position(self.scroll.as_usize());
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█");
            f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
        }
    }
}

// ─── Block builders ─────────────────────────────────────────────

/// Render markdown text with 2-space indent, returning owned lines.
fn markdown_to_lines(text: &str) -> Vec<Line<'static>> {
    super::markdown::markdown_to_lines(text)
        .into_iter()
        .map(|line| {
            let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

/// Build a `MessageBlock` for a single user message.
fn build_user_block(msg: &ConversationMessage) -> MessageBlock {
    let text = msg.text_content();
    if text.is_empty() {
        let mut lines = Vec::new();
        for block in &msg.content {
            if let ContentBlock::ToolResult { content, is_error, .. } = block {
                build_tool_result_lines(&mut lines, content, *is_error);
            }
        }
        MessageBlock { lines, bg: None, spacing: Spacing::Flush }
    } else {
        MessageBlock {
            lines: vec![Line::from(vec![
                Span::styled("❯ ", Style::default().fg(Color::DarkGray)),
                Span::raw(text),
            ])],
            bg: Some(Color::Rgb(55, 55, 55)),
            spacing: Spacing::MarginTop,
        }
    }
}

/// Build display lines for a tool result block.
fn build_tool_result_lines(
    lines: &mut Vec<Line<'static>>,
    content: &serde_json::Value,
    is_error: crate::types::ToolResultStatus,
) {
    let result_text = match content {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let color = if is_error.is_error() { Color::Red } else { Color::DarkGray };
    let result_lines: Vec<&str> = result_text.lines().collect();
    let max_lines = if is_error.is_error() { 10 } else { 3 };
    for (i, line) in result_lines.iter().take(max_lines).enumerate() {
        let prefix = if i == 0 { "  ⎿  " } else { "     " };
        lines.push(Line::from(Span::styled(
            format!("{prefix}{line}"),
            Style::default().fg(color),
        )));
    }
    if result_lines.len() > max_lines {
        lines.push(Line::from(Span::styled(
            format!("     … +{} lines", result_lines.len() - max_lines),
            Style::default().fg(Color::DarkGray),
        )));
    }
}

/// Build a `MessageBlock` for a single assistant message.
fn build_assistant_block(msg: &ConversationMessage) -> MessageBlock {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for block in &msg.content {
        match block {
            ContentBlock::Text { text, .. } => {
                lines.extend(markdown_to_lines(text.as_ref()));
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let input_summary = input.to_string();
                let truncated = match input_summary.char_indices().nth(80) {
                    Some((byte_idx, _)) => format!("{}…", &input_summary[..byte_idx]),
                    None => input_summary,
                };
                lines.push(Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{name}("), Style::default().fg(Color::Yellow)),
                    Span::styled(truncated, Style::default().fg(Color::DarkGray)),
                    Span::styled(")", Style::default().fg(Color::Yellow)),
                ]));
            }
            _ => {}
        }
    }
    MessageBlock { lines, bg: None, spacing: Spacing::MarginTop }
}

/// Convert all messages + streaming state into a list of renderable blocks.
fn build_message_blocks(app: &App) -> Vec<MessageBlock> {
    let mut blocks: Vec<MessageBlock> = Vec::with_capacity(app.messages.len() + 1);

    for msg in &app.messages {
        blocks.push(match msg.role {
            Role::User => build_user_block(msg),
            Role::Assistant => build_assistant_block(msg),
        });
    }

    if app.is_streaming() && !app.streaming.is_empty() {
        let mut lines = markdown_to_lines(app.streaming.as_ref());
        lines.push(Line::from(Span::styled("  ▊", Style::default().fg(Color::White))));
        blocks.push(MessageBlock { lines, bg: None, spacing: Spacing::MarginTop });
    } else if app.is_streaming() {
        blocks.push(MessageBlock {
            lines: vec![Line::from(Span::styled(
                "  ◐ thinking...",
                Style::default().fg(Color::DarkGray),
            ))],
            bg: None,
            spacing: Spacing::MarginTop,
        });
    }

    blocks
}

// ─── Draw functions ─────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),     // messages
            Constraint::Length(1),   // spinner line
            Constraint::Length(1),   // separator top
            Constraint::Length(1),   // input
            Constraint::Length(1),   // separator bottom
            Constraint::Length(1),   // status bar
            Constraint::Length(1),   // padding
        ])
        .split(f.area());

    draw_messages(f, app, chunks[0]);
    draw_activity_line(f, app, chunks[1]);
    draw_separator_plain(f, chunks[2]);
    draw_input(f, app, chunks[3]);
    draw_separator_plain(f, chunks[4]);
    draw_status_bar(f, app, chunks[5]);
}

fn draw_messages(f: &mut Frame, app: &mut App, area: Rect) {
    let prepared = PreparedBlocks { blocks: build_message_blocks(app) };
    let measured = prepared.measure(area.width);
    app.total_content_height = measured.total_height;
    let viewport = measured.clip_to_viewport(area.height, app.scroll);
    viewport.render(f, area);
}

#[allow(clippy::cast_possible_truncation)] // elapsed values are small after division
fn draw_activity_line(f: &mut Frame, app: &App, area: Rect) {
    let line = if app.is_streaming() {
        const WORDS: &[&str] = &[
            "Thinking", "Pondering", "Reasoning", "Computing", "Analyzing",
            "Processing", "Evaluating", "Crafting", "Generating", "Working",
            "Musing", "Brewing", "Cooking", "Noodling", "Crunching",
        ];
        let elapsed = app.turn_timer.elapsed_ms().unwrap_or(0);
        let idx = (elapsed / 2000) as usize % WORDS.len();
        let word = WORDS[idx];
        let dots = ".".repeat(((elapsed / 500) % 4) as usize);
        let secs = elapsed / 1000;

        Line::from(vec![
            Span::styled(
                format!("  · {word}{dots}"),
                Style::default().fg(Color::Rgb(153, 153, 153)),
            ),
            Span::styled(
                if secs > 0 { format!(" ({secs}s)") } else { String::new() },
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else if let Some(secs) = app.turn_timer.completed_secs() {
        Line::from(Span::styled(
            format!("  ✻ Cooked for {secs:.0}s"),
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from("")
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_separator_plain(f: &mut Frame, area: Rect) {
    let sep = "─".repeat(usize::from(area.width));
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(sep, Style::default().fg(Color::DarkGray)))),
        area,
    );
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![
        Span::styled("  ❯ ", Style::default().fg(Color::DarkGray)),
        Span::raw(app.input.text()),
    ]);

    let paragraph = Paragraph::new(line).block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);

    f.set_cursor_position((area.x + 4 + app.input.display_cursor_offset().as_u16(), area.y));
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let tokens = app.tokens;

    let mut parts: Vec<Span> = vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{}@{}", app.username, app.hostname),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(app.display_path.as_ref().to_owned(), Style::default().fg(Color::DarkGray)),
    ];

    if let Some(b) = &app.git_branch {
        parts.push(Span::styled("  ", Style::default()));
        parts.push(Span::styled(b.as_ref().to_owned(), Style::default().fg(Color::DarkGray)));
    }

    parts.push(Span::styled("  ", Style::default()));
    parts.push(Span::styled(app.display_model.as_ref().to_owned(), Style::default().fg(Color::DarkGray)));

    if !tokens.is_empty() {
        parts.push(Span::styled(
            format!(" · {tokens}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(parts)), area);
}
