//! Rendering for the TUI chat. Pure functions: takes `&App`, paints a
//! ratatui frame. No I/O or state mutation.

use crate::tui::app::{App, BannerKind, Bubble, Popup, SLASH_COMMANDS};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
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
    if size.width == 0 || size.height == 0 {
        return;
    }

    // Outer split: optional sidebar + main column. On narrow terminals the
    // sidebar used to consume the whole frame, leaving the transcript/input at
    // width 0. Collapse it automatically below a practical breakpoint.
    let sidebar_visible = app.show_sidebar && size.width >= 72;
    let main_chunks = if sidebar_visible {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(1)])
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(size)
    };

    if sidebar_visible {
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
            Style::default()
                .fg(ACCENT_PURPLE)
                .add_modifier(Modifier::BOLD),
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
                Span::styled(pad_to_width(&truncate(&s.name, 18), 18), style),
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
    // Reserve the composer before allocating transcript space. An unsatisfiable
    // Min(5) transcript constraint used to squeeze the input down to its borders.
    let header_height = if area.height >= 12 { 3 } else { 0 };
    let footer_height = if area.height >= 6 { 1 } else { 0 };
    let available = area.height.saturating_sub(header_height + footer_height);
    let transcript_min = u16::from(available >= 4);
    let composer_height =
        input_height(app, area.width).min(available.saturating_sub(transcript_min));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(available.saturating_sub(composer_height)),
            Constraint::Length(composer_height),
            Constraint::Length(footer_height),
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
        Popup::File {
            matches, selected, ..
        } => draw_file_popup(f, app, chunks[2], matches, *selected),
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
        Span::styled(" ○ Idle ", Style::default().fg(TEXT_DIM))
    };
    let title = Line::from(vec![
        Span::styled(
            "  Praxis ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Chat",
            Style::default()
                .fg(ACCENT_PURPLE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("» {} ", session_name),
            Style::default().fg(TEXT_PRIMARY),
        ),
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
    if area.width == 0 || area.height == 0 {
        return;
    }
    let inner_width = area.width.saturating_sub(2) as usize;
    let bubble_max = inner_width.saturating_sub(2).max(1);

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
    if max_width < 12 {
        return compact_bubble(label, content, accent, max_width.max(1));
    }

    // `max_width` is a terminal column budget. Keep every rendered line within
    // that budget, including borders, padding and wide glyphs.
    let max_inner = max_width.saturating_sub(2).max(1);
    let max_content = max_inner.saturating_sub(2).max(1);
    let wrapped = wrap_text(content, max_content);
    let widest_content = wrapped.iter().map(|l| display_width(l)).max().unwrap_or(0);
    let raw_label = format!(" {label} ");
    let label_budget = max_inner.saturating_sub(1).max(1);
    let label_text = truncate(&raw_label, label_budget);
    let label_width = display_width(&label_text);
    let inner_width = (widest_content + 2)
        .max(label_width + 1)
        .clamp(1, max_inner);
    let content_width = inner_width.saturating_sub(2).max(1);
    let wrapped = if content_width == max_content {
        wrapped
    } else {
        wrap_text(content, content_width)
    };

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Top border width: 1 + inner_width + 1.
    let top_fill = inner_width.saturating_sub(1 + label_width);
    let top = format!("╭─{label_text}{}╮", "─".repeat(top_fill));
    lines.push(align_line(
        top,
        align,
        max_width,
        Style::default()
            .fg(accent)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    ));

    // Body width: border + (space + content + pad + space) + border.
    let style_body = Style::default().fg(TEXT_PRIMARY).bg(bg);
    let style_border = Style::default().fg(accent).bg(bg);
    for content_line in wrapped {
        let cw = display_width(&content_line);
        let pad = content_width.saturating_sub(cw);
        let row = Line::from(vec![
            Span::styled("│".to_string(), style_border),
            Span::styled(" ".to_string(), style_body),
            Span::styled(content_line, style_body),
            Span::styled(" ".repeat(pad), style_body),
            Span::styled(" ".to_string(), style_body),
            Span::styled("│".to_string(), style_border),
        ]);
        lines.push(align_line_spans(row, align, max_width));
    }

    let bot = format!("╰{}╯", "─".repeat(inner_width));
    lines.push(align_line(
        bot,
        align,
        max_width,
        Style::default().fg(accent).bg(bg),
    ));

    lines
}

fn align_line(text: String, align: BubbleAlign, max_width: usize, style: Style) -> Line<'static> {
    align_line_spans(Line::from(Span::styled(text, style)), align, max_width)
}

fn align_line_spans(line: Line<'static>, align: BubbleAlign, max_width: usize) -> Line<'static> {
    let pad = max_width.saturating_sub(line.width());
    let mut spans: Vec<Span<'static>> = line.spans.into_iter().collect();
    match align {
        BubbleAlign::Left => spans.push(Span::raw(" ".repeat(pad))),
        BubbleAlign::Right => spans.insert(0, Span::raw(" ".repeat(pad))),
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

fn compact_bubble(
    label: &str,
    content: &str,
    accent: Color,
    max_width: usize,
) -> Vec<Line<'static>> {
    let width = max_width.max(1);
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        truncate(label, width),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )));
    for line in wrap_text(content, width) {
        out.push(Line::from(Span::styled(
            truncate(&line, width),
            Style::default().fg(TEXT_PRIMARY),
        )));
    }
    out
}

/// Soft-wrap `text` to `width` terminal columns, preserving explicit `\n` line
/// breaks. Wide CJK/emoji glyphs use Ratatui's width implementation via
/// `Line::width`; combining marks (width 0) stay attached to their base.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in split_words(raw_line) {
            let ww = display_width(&word);
            if current.is_empty() {
                if ww > width {
                    out.extend(chunks_of(&word, width));
                } else {
                    current.push_str(&word);
                }
            } else if display_width(&current) + 1 + ww <= width {
                current.push(' ');
                current.push_str(&word);
            } else {
                out.push(std::mem::take(&mut current));
                if ww > width {
                    out.extend(chunks_of(&word, width));
                } else {
                    current.push_str(&word);
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
    let width = n.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        let ch_s = ch.to_string();
        let ch_width = display_width(&ch_s);
        if current.is_empty() && ch_width > width {
            chunks.push(truncate(&ch_s, width));
            continue;
        }
        if !current.is_empty() && ch_width > 0 && display_width(&current) + ch_width > width {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
        if display_width(&current) >= width {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn display_width(s: &str) -> usize {
    Line::from(s.to_string()).width()
}

fn input_height(app: &App, frame_width: u16) -> u16 {
    let inner_width = frame_width.saturating_sub(4).max(1) as usize;
    let mut lines = input_visual_lines(app, inner_width).0.len().min(10) as u16;
    if !app.attachments.is_empty() {
        lines += 1;
    }
    (lines + 2).clamp(3, 12)
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.height < 3 || area.width < 5 {
        let (visual, cursor_row) = input_visual_lines(app, area.width as usize);
        let rows = area.height as usize;
        let start = cursor_row.saturating_sub(rows.saturating_sub(1));
        let lines: Vec<Line> = visual
            .into_iter()
            .skip(start)
            .take(rows)
            .map(|text| Line::from(Span::styled(text, Style::default().fg(TEXT_PRIMARY))))
            .collect();
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
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
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    let inner_width = inner.width.max(1) as usize;
    let visible_rows = inner.height.max(1) as usize;
    let show_attachments = !app.attachments.is_empty() && visible_rows > 1;
    let mut lines: Vec<Line> = Vec::new();
    if show_attachments {
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
            truncate(&span_text, inner_width),
            Style::default().fg(ACCENT_AMBER),
        )));
    }

    let input_style = Style::default().fg(TEXT_PRIMARY);
    if app.input.is_empty() {
        let mut spans = vec![Span::styled("█", Style::default().fg(ACCENT_CYAN))];
        if inner_width > 1 {
            spans.push(Span::styled(
                truncate(
                    " Type a message…  (try /help, @path, Shift+Enter for newline)",
                    inner_width - 1,
                ),
                Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC),
            ));
        }
        lines.push(Line::from(spans));
    } else {
        let input_rows = visible_rows
            .saturating_sub(usize::from(show_attachments))
            .max(1);
        let (visual, cursor_row) = input_visual_lines(app, inner_width);
        let start = cursor_row.saturating_sub(input_rows - 1);
        for l in visual.into_iter().skip(start).take(input_rows) {
            lines.push(Line::from(Span::styled(l, input_style)));
        }
    }

    // Input is already hard-wrapped without discarding spaces. Re-wrapping it
    // would invalidate the cursor row and can hide it again.
    f.render_widget(Paragraph::new(lines).block(block), area);
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
            Style::default()
                .fg(ACCENT_AMBER)
                .add_modifier(Modifier::BOLD),
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
                Span::styled(
                    pad_to_width(&format!(" {}", cmd.name), 13),
                    style.add_modifier(Modifier::BOLD),
                ),
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
            Style::default()
                .fg(ACCENT_PURPLE)
                .add_modifier(Modifier::BOLD),
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
            ListItem::new(Line::from(vec![Span::styled(
                format!(" {icon} {display}"),
                style,
            )]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .title(Span::styled(
            " @ files ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_PANEL));
    let list = List::new(items).block(block);
    f.render_widget(list, popup_area);
}

fn pad_to_width(s: &str, width: usize) -> String {
    let mut out = truncate(s, width);
    let pad = width.saturating_sub(display_width(&out));
    out.push_str(&" ".repeat(pad));
    out
}

fn input_visual_lines(app: &App, width: usize) -> (Vec<String>, usize) {
    let width = width.max(1);
    let chars: Vec<char> = app.input.chars().collect();
    let cursor_pos = app.cursor.min(chars.len());
    let before: String = chars[..cursor_pos].iter().collect();
    let after: String = chars[cursor_pos..].iter().collect();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut cursor_row = 0;
    for (index, ch) in format!("{before}█{after}").chars().enumerate() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
            continue;
        }
        let piece = truncate(&ch.to_string(), width);
        if !current.is_empty() && display_width(&format!("{current}{piece}")) > width {
            lines.push(std::mem::take(&mut current));
        }
        if index == cursor_pos {
            cursor_row = lines.len();
        }
        current.push_str(&piece);
    }
    lines.push(current);
    (lines, cursor_row)
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if display_width(s) <= max {
        return s.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let budget = max - 1;
    for ch in s.chars() {
        let next = format!("{out}{ch}");
        if display_width(&next) > budget {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_width(line: &Line<'_>) -> usize {
        line.width()
    }

    fn assert_lines_fit(lines: &[Line<'_>], width: usize) {
        for line in lines {
            assert!(
                line_width(line) <= width,
                "line width {} > {width}: {:?}",
                line_width(line),
                line
            );
        }
    }

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
    fn column_width_helpers_handle_wide_and_combining_text() {
        assert_eq!(Line::from("日本").width(), 4);
        assert_eq!(display_width("é"), Line::from("é").width());
        assert_eq!(display_width("e\u{301}"), Line::from("e\u{301}").width());
        assert_eq!(display_width("🌵"), Line::from("🌵").width());
        for width in [1, 8, 12, 24, 40] {
            for chunk in chunks_of("日本語🌵e\u{301}abcdef", width) {
                assert!(display_width(&chunk) <= width, "{chunk:?} exceeds {width}");
            }
            for line in wrap_text("日本語 🌵🌵🌵 e\u{301}e\u{301} supercalifragilistic", width)
            {
                assert!(display_width(&line) <= width, "{line:?} exceeds {width}");
            }
        }
    }

    #[test]
    fn truncate_works_by_columns() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert!(Line::from(truncate("日本語🌵", 5)).width() <= 5);
        assert!(Line::from(truncate("e\u{301}e\u{301}e\u{301}", 3)).width() <= 3);
    }

    #[test]
    fn bubbles_fit_all_small_widths_with_wide_text_and_long_labels() {
        for width in [1, 8, 12, 24, 40] {
            for align in [BubbleAlign::Left, BubbleAlign::Right] {
                let lines = bubble_block(
                    "praxis-日本語🌵-very-long-label",
                    "日本語🌵 e\u{301} accents supercalifragilisticexpialidocious",
                    align,
                    ACCENT_PURPLE,
                    BG_BUBBLE_BOT,
                    width,
                );
                assert_lines_fit(&lines, width);
            }
        }
    }

    fn dummy_app() -> (App, tempfile::TempDir) {
        use crate::db::Database;
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let app = App::new(
            db,
            "http://127.0.0.1:0".into(),
            "x".into(),
            dir.path().to_string_lossy().into_owned(),
        );
        (app, dir)
    }

    fn buffer_contains_cursor(terminal: &Terminal<TestBackend>) -> bool {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "█")
    }

    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn input_height_accounts_for_soft_wraps_and_cursor_stays_visible() {
        for (width, height, input) in [
            (1, 1, "abc"),
            (8, 4, "日本語🌵"),
            (24, 10, ""),
            (24, 10, "日本語🌵"),
            (24, 10, "long-input-"),
            (40, 12, "first line\nsecond 日本語 line\nthird line"),
        ] {
            let (mut app, _dir) = dummy_app();
            app.show_sidebar = false;
            app.input = if input == "long-input-" {
                format!("{}", "日本語🌵abcdef".repeat(10))
            } else {
                input.to_string()
            };
            app.attachments
                .push("/synthetic/very-long-attachment-name.txt".into());
            assert!(input_height(&app, width) >= 3);
            for cursor in [0, app.input.chars().count() / 2, app.input.chars().count()] {
                app.cursor = cursor;
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal.draw(|f| draw(f, &app)).unwrap();
                assert!(
                    buffer_contains_cursor(&terminal),
                    "cursor missing for width={width} input={input:?} cursor={cursor}"
                );
            }
        }
    }

    #[test]
    fn draw_regression_narrow_and_wide_terminals() {
        let (mut app, _dir) = dummy_app();
        app.transcript.push(Bubble::Assistant {
            content: "日本語🌵 A very long line_that_must_wrap_without_panicking_or_painting_past_the_terminal_width".into(),
        });
        app.input = "hello from a narrow terminal 日本語🌵".into();
        app.cursor = app.input.chars().count();

        for (w, h) in [(24, 10), (40, 8), (100, 30)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            assert!(buffer_contains_cursor(&terminal));
        }
    }
}
