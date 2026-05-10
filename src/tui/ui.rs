//! Rendering for the TUI chat. Pure functions: takes `&App`, paints a
//! ratatui frame. No I/O or state mutation.

use crate::tui::app::{App, BannerKind, Bubble, Popup, SLASH_COMMANDS};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
    Frame,
};

// Palette — picked to harmonise with the web dashboard's purple/cyan accents.
const ACCENT_PURPLE: Color = Color::Rgb(0x9a, 0x8c, 0xff);
const ACCENT_CYAN: Color = Color::Rgb(0x6a, 0xea, 0xff);
const ACCENT_GREEN: Color = Color::Rgb(0x6a, 0xff, 0xa6);
const ACCENT_AMBER: Color = Color::Rgb(0xff, 0xc8, 0x6a);
const ACCENT_PINK: Color = Color::Rgb(0xff, 0x8a, 0xb6);
const TEXT_PRIMARY: Color = Color::Rgb(0xe6, 0xe6, 0xff);
const TEXT_DIM: Color = Color::Rgb(0x9a, 0x9a, 0xc0);
const BG_PANEL: Color = Color::Rgb(0x14, 0x14, 0x1f);
const BG_BUBBLE_USER: Color = Color::Rgb(0x1a, 0x1f, 0x36);
const BG_BUBBLE_BOT: Color = Color::Rgb(0x1d, 0x1a, 0x36);
const BG_BUBBLE_TOOL: Color = Color::Rgb(0x12, 0x24, 0x2c);

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();

    // Outer split: optional sidebar + main column.
    let main_chunks = if app.show_sidebar {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(40)])
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(40)])
            .split(size)
    };

    if app.show_sidebar {
        draw_sidebar(f, app, main_chunks[0]);
    }
    draw_main(f, app, main_chunks[1]);
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_PURPLE))
        .title(Span::styled(
            " Sessions ",
            Style::default().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_PANEL));

    let items: Vec<ListItem> = app
        .sessions
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_active = s.id == app.sessions.active;
            let marker = if is_active { "▌" } else { " " };
            let key_hint = if i < 9 {
                format!("M-{}", i + 1)
            } else {
                "   ".to_string()
            };
            let style = if is_active {
                Style::default()
                    .fg(ACCENT_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            let line = Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT_CYAN)),
                Span::raw(" "),
                Span::styled(format!("{:<18}", truncate(&s.name, 18)), style),
                Span::styled(format!(" {}", key_hint), Style::default().fg(TEXT_DIM)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);

    // User name footer.
    if area.height >= 4 {
        let footer_area = Rect {
            x: area.x + 1,
            y: area.y + area.height.saturating_sub(2),
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let footer = Paragraph::new(Line::from(vec![
            Span::styled("user: ", Style::default().fg(TEXT_DIM)),
            Span::styled(
                truncate(app.username(), 18),
                Style::default()
                    .fg(ACCENT_PINK)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        f.render_widget(footer, footer_area);
    }
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // transcript
            Constraint::Length(input_height(app)), // input
            Constraint::Length(1), // footer hint
        ])
        .split(area);

    draw_header(f, app, chunks[0]);
    draw_transcript(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);
    draw_footer(f, app, chunks[3]);

    // Popups float over the input row.
    match &app.popup {
        Popup::Slash { matches, selected } => {
            draw_slash_popup(f, app, chunks[2], matches, *selected)
        }
        Popup::File { matches, selected, .. } => {
            draw_file_popup(f, app, chunks[2], matches, *selected)
        }
        Popup::None => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let session_name = app
        .sessions
        .active_session()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| app.sessions.active.clone());
    let agent = if app.agent_active {
        Span::styled(
            " ● Agent active ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ○ Idle ",
            Style::default().fg(TEXT_DIM),
        )
    };
    let title = Line::from(vec![
        Span::styled("  Praxis ", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("Chat", Style::default().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD)),
        Span::raw("   "),
        Span::styled(format!("» {} ", session_name), Style::default().fg(TEXT_PRIMARY)),
        Span::raw("  "),
        agent,
    ]);
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(ACCENT_PURPLE));
    let para = Paragraph::new(title).block(block);
    f.render_widget(para, area);
}

fn draw_transcript(f: &mut Frame, app: &App, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let bubble_max = inner_width.saturating_sub(8).max(20);

    // Build all lines, top-to-bottom.
    let mut lines: Vec<Line> = Vec::new();
    if app.transcript.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No messages yet — type a message and press Enter.",
            Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC),
        )));
        lines.push(Line::from(Span::styled(
            "Try `/help` for commands or attach files with @path",
            Style::default().fg(TEXT_DIM),
        )));
    } else {
        for (idx, b) in app.transcript.iter().enumerate() {
            let bubble_lines = render_bubble(b, bubble_max, app.username());
            for l in bubble_lines {
                lines.push(l);
            }
            if idx + 1 < app.transcript.len() {
                lines.push(Line::from(""));
            }
        }
    }

    // Pin to bottom: take only the last `area.height` lines (minus scroll offset).
    let total = lines.len() as u16;
    let visible_height = area.height;
    let max_offset = total.saturating_sub(visible_height);
    let offset = app.scroll_offset.min(max_offset);
    let from = max_offset.saturating_sub(offset) as usize;
    let to = (from + visible_height as usize).min(lines.len());
    let visible: Vec<Line> = lines[from..to].to_vec();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(0x33, 0x33, 0x55)))
        .style(Style::default().bg(BG_PANEL));
    let para = Paragraph::new(visible)
        .wrap(Wrap { trim: false })
        .block(block);
    f.render_widget(para, area);
}

/// Render a single bubble into 1+ ratatui lines.
fn render_bubble(b: &Bubble, max_width: usize, username: &str) -> Vec<Line<'static>> {
    match b {
        Bubble::User { content } => bubble_block(
            username,
            content,
            BubbleAlign::Right,
            ACCENT_CYAN,
            BG_BUBBLE_USER,
            max_width,
        ),
        Bubble::Assistant { content } => bubble_block(
            "praxis",
            content,
            BubbleAlign::Left,
            ACCENT_PURPLE,
            BG_BUBBLE_BOT,
            max_width,
        ),
        Bubble::Tool { name, content } => bubble_block(
            &format!("⚙ {name}"),
            content,
            BubbleAlign::Left,
            ACCENT_GREEN,
            BG_BUBBLE_TOOL,
            max_width,
        ),
        Bubble::System { content } => banner_line(content, ACCENT_GREEN, "·"),
        Bubble::Banner { kind, content } => {
            let (color, icon) = match kind {
                BannerKind::AgentStart => (ACCENT_CYAN, "▶"),
                BannerKind::AgentStop => (ACCENT_PURPLE, "■"),
                BannerKind::Info => (TEXT_DIM, "ℹ"),
                BannerKind::Error => (ACCENT_PINK, "✕"),
            };
            banner_line(content, color, icon)
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BubbleAlign {
    Left,
    Right,
}

/// Wrap `content` to `max_width - 4` columns and render as a chat bubble with
/// a label header. Bubbles are framed with rounded corners using box-drawing
/// characters for a soft, modern look.
fn bubble_block(
    label: &str,
    content: &str,
    align: BubbleAlign,
    accent: Color,
    bg: Color,
    max_width: usize,
) -> Vec<Line<'static>> {
    let inner_width = max_width.saturating_sub(4).max(10);
    let wrapped = wrap_text(content, inner_width);
    let body_width = wrapped.iter().map(|l| display_width(l)).max().unwrap_or(0);
    let body_width = body_width.max(label.chars().count() + 2);
    let body_width = body_width.min(inner_width);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Top border:  ╭─ label ────────╮
    let label_text = format!(" {label} ");
    let label_chars = label_text.chars().count();
    let dashes_after = body_width.saturating_sub(label_chars).max(1) + 1;
    let top_inner = format!("─{label_text}{}", "─".repeat(dashes_after));
    let top = format!("╭{top_inner}╮");
    lines.push(align_line(
        top,
        align,
        max_width,
        Style::default().fg(accent).bg(bg).add_modifier(Modifier::BOLD),
    ));

    // Body lines: │ content padded │
    let style_body = Style::default().fg(TEXT_PRIMARY).bg(bg);
    let style_border = Style::default().fg(accent).bg(bg);
    for content_line in wrapped {
        let pad_to = body_width + 2;
        let mut line = String::new();
        line.push('│');
        line.push(' ');
        let cw = display_width(&content_line);
        line.push_str(&content_line);
        if cw < pad_to.saturating_sub(1) {
            line.push_str(&" ".repeat(pad_to.saturating_sub(1).saturating_sub(cw)));
        }
        line.push('│');
        // Style the borders separately from the content for a coloured frame.
        let mid_text = content_line.clone();
        let pad = pad_to.saturating_sub(1).saturating_sub(cw);
        let row = Line::from(vec![
            Span::styled("│".to_string(), style_border),
            Span::styled(" ".to_string(), style_body),
            Span::styled(mid_text, style_body),
            Span::styled(" ".repeat(pad), style_body),
            Span::styled("│".to_string(), style_border),
        ]);
        lines.push(align_line_spans(row, align, max_width));
    }

    // Bottom border: ╰────────────────╯
    let bot_inner = "─".repeat(body_width + 2);
    let bot = format!("╰{bot_inner}╯");
    lines.push(align_line(
        bot,
        align,
        max_width,
        Style::default().fg(accent).bg(bg),
    ));

    lines
}

fn align_line(text: String, align: BubbleAlign, max_width: usize, style: Style) -> Line<'static> {
    let w = text.chars().count();
    let pad = max_width.saturating_sub(w);
    match align {
        BubbleAlign::Left => Line::from(vec![Span::styled(text, style), Span::raw(" ".repeat(pad))]),
        BubbleAlign::Right => {
            Line::from(vec![Span::raw(" ".repeat(pad)), Span::styled(text, style)])
        }
    }
}

fn align_line_spans(line: Line<'static>, align: BubbleAlign, max_width: usize) -> Line<'static> {
    let w: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = max_width.saturating_sub(w);
    let mut spans: Vec<Span<'static>> = line.spans.into_iter().collect();
    match align {
        BubbleAlign::Left => {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        BubbleAlign::Right => {
            spans.insert(0, Span::raw(" ".repeat(pad)));
        }
    }
    Line::from(spans)
}

fn banner_line(content: &str, color: Color, icon: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let header = format!("{icon}  {content}");
    out.push(Line::from(Span::styled(
        header,
        Style::default()
            .fg(color)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC),
    )));
    out
}

/// Soft-wrap `text` to `width` columns, preserving explicit `\n` line breaks.
/// Returns owned `String`s so the caller can produce `Span<'static>`s safely.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_w = 0;
        for word in split_words(raw_line) {
            let ww = word.chars().count();
            if current_w == 0 {
                if ww > width {
                    // Hard split very long words.
                    for chunk in chunks_of(&word, width) {
                        out.push(chunk);
                    }
                    continue;
                }
                current.push_str(&word);
                current_w = ww;
            } else if current_w + 1 + ww <= width {
                current.push(' ');
                current.push_str(&word);
                current_w += 1 + ww;
            } else {
                out.push(std::mem::take(&mut current));
                if ww > width {
                    for chunk in chunks_of(&word, width) {
                        out.push(chunk);
                    }
                    current_w = 0;
                } else {
                    current.push_str(&word);
                    current_w = ww;
                }
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

fn split_words(s: &str) -> Vec<String> {
    s.split(' ').map(|w| w.to_string()).collect()
}

fn chunks_of(s: &str, n: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(n).map(|c| c.iter().collect()).collect()
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn input_height(app: &App) -> u16 {
    let lines = app.input.lines().count().max(1) as u16;
    let attachments = if app.attachments.is_empty() { 0 } else { 1 };
    (lines + 2 + attachments).clamp(3, 12)
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let title_str = if app.attachments.is_empty() {
        " Message ".to_string()
    } else {
        format!(" Message · {} attachment(s) ", app.attachments.len())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .title(Span::styled(
            title_str,
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1));

    // Build the content with a visible cursor indicator.
    let chars: Vec<char> = app.input.chars().collect();
    let cursor_pos = app.cursor.min(chars.len());
    let before: String = chars[..cursor_pos].iter().collect();
    let after: String = chars[cursor_pos..].iter().collect();

    let mut lines: Vec<Line> = Vec::new();
    if !app.attachments.is_empty() {
        let span_text = app
            .attachments
            .iter()
            .map(|p| {
                let basename = std::path::Path::new(p)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone());
                format!("📎 {basename}")
            })
            .collect::<Vec<_>>()
            .join("   ");
        lines.push(Line::from(Span::styled(
            span_text,
            Style::default().fg(ACCENT_AMBER),
        )));
    }

    // Render input. Multi-line inputs split on '\n'. Cursor is the █ char
    // injected at position.
    let input_with_cursor = format!("{before}\u{2588}{after}");
    if input_with_cursor.is_empty() {
        lines.push(Line::from(Span::styled(
            "Type a message…  (try /help, @path, Shift+Enter for newline)",
            Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC),
        )));
    } else {
        for l in input_with_cursor.lines() {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(TEXT_PRIMARY),
            )));
        }
    }

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![
        keybind("Enter", "send"),
        Span::raw("  "),
        keybind("Sh+Enter", "newline"),
        Span::raw("  "),
        keybind("Tab", "complete"),
        Span::raw("  "),
        keybind("^N", "new session"),
        Span::raw("  "),
        keybind("M-1..9", "switch"),
        Span::raw("  "),
        keybind("^B", "sidebar"),
        Span::raw("  "),
        keybind("^C", "quit"),
    ];
    if let Some((msg, _)) = &app.status_msg {
        spans.push(Span::raw("  ·  "));
        spans.push(Span::styled(
            msg.clone(),
            Style::default().fg(ACCENT_AMBER).add_modifier(Modifier::BOLD),
        ));
    }
    let para = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Left)
        .style(Style::default().fg(TEXT_DIM));
    f.render_widget(para, area);
}

fn keybind<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::raw(format!("{key} {label}"))
}

fn draw_slash_popup(
    f: &mut Frame,
    _app: &App,
    input_area: Rect,
    matches: &[usize],
    selected: usize,
) {
    let height = (matches.len() as u16 + 2).min(10);
    let width = 50.min(input_area.width.saturating_sub(2));
    let popup_area = Rect {
        x: input_area.x + 1,
        y: input_area.y.saturating_sub(height),
        width,
        height,
    };
    f.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(i, &cmd_idx)| {
            let cmd = &SLASH_COMMANDS[cmd_idx];
            let is_sel = i == selected;
            let style = if is_sel {
                Style::default()
                    .bg(ACCENT_PURPLE)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<12}", cmd.name), style.add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {}", cmd.desc),
                    if is_sel {
                        Style::default().bg(ACCENT_PURPLE).fg(Color::Black)
                    } else {
                        Style::default().fg(TEXT_DIM)
                    },
                ),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_PURPLE))
        .title(Span::styled(
            " / commands ",
            Style::default().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_PANEL));
    let list = List::new(items).block(block);
    f.render_widget(list, popup_area);
}

fn draw_file_popup(
    f: &mut Frame,
    _app: &App,
    input_area: Rect,
    matches: &[String],
    selected: usize,
) {
    let height = (matches.len() as u16 + 2).min(12);
    let width = 70.min(input_area.width.saturating_sub(2));
    let popup_area = Rect {
        x: input_area.x + 1,
        y: input_area.y.saturating_sub(height),
        width,
        height,
    };
    f.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_sel = i == selected;
            let display = truncate(p, width.saturating_sub(4) as usize);
            let icon = if p.ends_with('/') { "📁" } else { "📄" };
            let style = if is_sel {
                Style::default()
                    .bg(ACCENT_CYAN)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {icon} {display}"), style),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .title(Span::styled(
            " @ files ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_PANEL));
    let list = List::new(items).block(block);
    f.render_widget(list, popup_area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        let head: String = s.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_basic() {
        let lines = wrap_text("hello world this is a test", 10);
        assert_eq!(lines, vec!["hello", "world this", "is a test"]);
    }

    #[test]
    fn wrap_preserves_paragraphs() {
        let lines = wrap_text("a\n\nb", 80);
        assert_eq!(lines, vec!["a", "", "b"]);
    }

    #[test]
    fn truncate_works() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
    }
}
