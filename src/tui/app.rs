//! TUI chat application — state, event loop, and dispatch.

use crate::db::messages::Message;
use crate::db::Database;
use crate::tui::sessions::{Session, SessionStore};
use crate::tui::ui;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
        EventStream, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use futures_util::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::time::Instant;

/// One slash command the TUI knows about.
pub struct SlashCmd {
    pub name: &'static str,
    pub desc: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCmd] = &[
    SlashCmd { name: "/clear", desc: "Clear messages in this session" },
    SlashCmd { name: "/new", desc: "Create a new chat session (forks current)" },
    SlashCmd { name: "/sessions", desc: "Toggle the sessions sidebar" },
    SlashCmd { name: "/stop", desc: "Stop the running agent loop" },
    SlashCmd { name: "/status", desc: "Print agent status" },
    SlashCmd { name: "/context", desc: "Get/set context variables (e.g. set foo=bar)" },
    SlashCmd { name: "/rename", desc: "Rename the current session" },
    SlashCmd { name: "/delete", desc: "Delete the current session" },
    SlashCmd { name: "/help", desc: "Show available commands" },
    SlashCmd { name: "/quit", desc: "Exit the TUI" },
];

/// What the input-line popup is currently showing.
#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    None,
    /// Slash command picker. `selected` is an index into `matches`.
    Slash { matches: Vec<usize>, selected: usize },
    /// File picker triggered by an unfinished `@partial` token at the cursor.
    File {
        /// The portion of the path the user has typed after `@`.
        prefix: String,
        /// Resolved candidate paths (absolute).
        matches: Vec<String>,
        selected: usize,
    },
}

/// One chat row in the visible transcript. Built from DB messages plus
/// transient banners (system, agent loop start/stop).
#[derive(Debug, Clone)]
pub enum Bubble {
    User { content: String },
    Assistant { content: String },
    Tool { name: String, content: String },
    System { content: String },
    Banner { kind: BannerKind, content: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    AgentStart,
    AgentStop,
    Info,
    Error,
}

/// Top-level TUI state.
pub struct App {
    pub db: Database,
    pub gateway_url: String,
    pub gateway_api_key: String,
    pub data_dir: String,
    pub sessions: SessionStore,

    /// Currently displayed transcript (rebuilt from DB on session switch
    /// and on poll ticks). The Vec is in chronological order.
    pub transcript: Vec<Bubble>,
    /// Set of `(role, prefix)` keys we've already rendered, to dedupe new rows
    /// arriving via polling.
    seen_keys: std::collections::HashSet<String>,

    /// Multi-line input buffer.
    pub input: String,
    /// Caret position in chars (NOT bytes).
    pub cursor: usize,
    /// Pending file attachments for the next message (absolute paths).
    pub attachments: Vec<String>,

    pub popup: Popup,

    pub show_sidebar: bool,
    /// Vertical scroll offset (0 = pinned to bottom).
    pub scroll_offset: u16,
    /// Track whether the agent loop is currently running for the active session.
    pub agent_active: bool,

    pub status_msg: Option<(String, Instant)>,

    pub should_quit: bool,
}

impl App {
    pub fn new(
        db: Database,
        gateway_url: String,
        gateway_api_key: String,
        data_dir: String,
    ) -> Self {
        let sessions = SessionStore::load(&data_dir);
        Self {
            db,
            gateway_url,
            gateway_api_key,
            data_dir,
            sessions,
            transcript: Vec::new(),
            seen_keys: std::collections::HashSet::new(),
            input: String::new(),
            cursor: 0,
            attachments: Vec::new(),
            popup: Popup::None,
            show_sidebar: true,
            scroll_offset: 0,
            agent_active: false,
            status_msg: None,
            should_quit: false,
        }
    }

    pub fn active_user_id(&self) -> &str {
        &self.sessions.active
    }

    pub fn username(&self) -> &str {
        &self.sessions.username
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    /// Drop transient state when switching session.
    pub fn reset_view(&mut self) {
        self.transcript.clear();
        self.seen_keys.clear();
        self.scroll_offset = 0;
        self.input.clear();
        self.cursor = 0;
        self.attachments.clear();
        self.popup = Popup::None;
    }

    /// Refresh the transcript from the DB. Idempotent — uses `seen_keys` to
    /// avoid duplicates so it's safe to call on every poll tick.
    pub fn refresh_transcript(&mut self) {
        let user_id = self.active_user_id().to_string();
        let Ok(msgs) = self.db.get_messages(&user_id, 1000) else {
            return;
        };
        for m in msgs {
            let key = bubble_key(&m);
            if self.seen_keys.contains(&key) {
                continue;
            }
            // Skip purely intermediate assistant messages (just tool_calls).
            if m.role == "assistant"
                && m.tool_calls.as_ref().map(|t| !t.is_empty()).unwrap_or(false)
                && m.content.is_empty()
            {
                self.seen_keys.insert(key);
                continue;
            }
            let bubble = match m.role.as_str() {
                "user" => Bubble::User {
                    content: m.content.clone(),
                },
                "assistant" => Bubble::Assistant {
                    content: m.content.clone(),
                },
                "tool" => Bubble::Tool {
                    name: m.tool_name.clone().unwrap_or_else(|| "tool".to_string()),
                    content: m.content.clone(),
                },
                "system" => Bubble::System {
                    content: m.content.clone(),
                },
                _ => continue,
            };
            self.transcript.push(bubble);
            self.seen_keys.insert(key);
        }
    }

    /// Recompute the slash-command popup from the current input + cursor.
    fn update_popup(&mut self) {
        // Pull the token immediately before the cursor.
        let before: String = self.input.chars().take(self.cursor).collect();
        // Slash command: must be at the very start of the input line.
        if before.starts_with('/') && !before.contains(char::is_whitespace) {
            let q = before[1..].to_ascii_lowercase();
            let matches: Vec<usize> = SLASH_COMMANDS
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name[1..].to_ascii_lowercase().starts_with(&q))
                .map(|(i, _)| i)
                .collect();
            if !matches.is_empty() {
                self.popup = Popup::Slash {
                    matches,
                    selected: 0,
                };
                return;
            }
        }
        // File @ token (anywhere in the input).
        if let Some(at_idx) = before.rfind('@') {
            let after_at = &before[at_idx + 1..];
            if !after_at.contains(char::is_whitespace) {
                let prefix = after_at.to_string();
                let matches = file_matches(&prefix);
                if !matches.is_empty() {
                    self.popup = Popup::File {
                        prefix,
                        matches,
                        selected: 0,
                    };
                    return;
                }
            }
        }
        self.popup = Popup::None;
    }

    /// Apply a slash command. The full command line (including any args
    /// after the verb) is passed in as `line`.
    async fn run_slash(&mut self, line: &str) {
        // The first whitespace-separated token is the command name; the rest
        // is forwarded as-is so commands like `/context set foo=bar` work.
        let mut parts = line.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let _rest = parts.next().unwrap_or("").to_string();
        match name.as_str() {
            "/clear" => {
                if let Err(e) = self.db.clear_messages(self.active_user_id()) {
                    self.flash(format!("clear failed: {e}"));
                } else {
                    self.transcript.clear();
                    self.seen_keys.clear();
                    self.flash("cleared");
                }
            }
            "/new" => {
                self.new_session().await;
            }
            "/sessions" => {
                self.show_sidebar = !self.show_sidebar;
            }
            "/stop" => {
                self.stop_agent().await;
            }
            "/status" => {
                let active = if self.agent_active { "active" } else { "idle" };
                self.transcript.push(Bubble::Banner {
                    kind: BannerKind::Info,
                    content: format!("Agent is {active} for session {}", self.active_user_id()),
                });
            }
            "/context" | "/ctx" => {
                self.run_context_command(line);
            }
            "/rename" => {
                self.flash("edit `tui_sessions.json` to rename for now");
            }
            "/delete" => {
                self.delete_active_session();
            }
            "/help" => {
                let body = SLASH_COMMANDS
                    .iter()
                    .map(|c| format!("  {} — {}", c.name, c.desc))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.transcript.push(Bubble::Banner {
                    kind: BannerKind::Info,
                    content: format!(
                        "Commands:\n{body}\n\nAttach files with @path. Use Tab/Enter to autocomplete.\n\
                         /context examples:\n  \
                            /context set custom_data.device=main settings.max_llm_turns=20\n  \
                            /context set settings.voice_tts_enabled=true\n  \
                            /context get settings.max_llm_turns\n  \
                            /context show settings"
                    ),
                });
            }
            "/quit" | "/exit" => {
                self.should_quit = true;
            }
            _ => {
                self.flash(format!("unknown command: {name}"));
            }
        }
    }

    fn run_context_command(&mut self, line: &str) {
        match crate::context_cmd::parse(line) {
            Ok(op) => {
                let user_id = self.active_user_id().to_string();
                let response = crate::context_cmd::apply(&self.db, &user_id, &op);
                self.transcript.push(Bubble::Banner {
                    kind: BannerKind::Info,
                    content: response,
                });
            }
            Err(e) => {
                self.transcript.push(Bubble::Banner {
                    kind: BannerKind::Error,
                    content: format!("/context: {e}"),
                });
            }
        }
    }

    /// Send the current input + attachments to the gateway.
    async fn send_message(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        // Slash commands run locally — they should never leave the TUI.
        // We accept both single-token (`/help`) and multi-token (`/context set foo=bar`).
        if text.starts_with('/') {
            self.input.clear();
            self.cursor = 0;
            self.popup = Popup::None;
            self.run_slash(&text).await;
            return;
        }

        // Strip @file tokens out of the visible text and add their resolved
        // paths to the attachment list. The gateway sees a clean message
        // and a separate `attachments` array.
        let (clean_text, mut extra_attachments) = split_at_attachments(&text);
        let mut atts = std::mem::take(&mut self.attachments);
        atts.append(&mut extra_attachments);

        // Optimistic local echo.
        let display = if clean_text.is_empty() {
            "(attachments)".to_string()
        } else {
            clean_text.clone()
        };
        let local_key = format!("user::{}", display.chars().take(80).collect::<String>());
        self.transcript.push(Bubble::User {
            content: display.clone(),
        });
        self.seen_keys.insert(local_key);

        self.input.clear();
        self.cursor = 0;
        self.popup = Popup::None;

        // Fire-and-forget HTTP POST to the local gateway. The gateway runs
        // the agent loop and persists results to the DB; our poll task picks
        // them up.
        let url = format!("{}/v1/chat", self.gateway_url.trim_end_matches('/'));
        let api_key = self.gateway_api_key.clone();
        let user_id = self.active_user_id().to_string();
        let body = serde_json::json!({
            "user_id": user_id,
            "message": clean_text,
            "attachments": atts,
        });
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(60 * 30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let _ = client
                .post(&url)
                .header("x-api-key", &api_key)
                .json(&body)
                .send()
                .await;
        });
        self.agent_active = true;
        self.flash("sent");
    }

    async fn stop_agent(&mut self) {
        let user_id = self.active_user_id().to_string();
        crate::gateway::agent_loop::stop_agent_loop(&user_id).await;
        self.agent_active = false;
        self.transcript.push(Bubble::Banner {
            kind: BannerKind::AgentStop,
            content: "Stop signal sent".to_string(),
        });
    }

    async fn new_session(&mut self) {
        let parent = self.active_user_id().to_string();
        let new_id = SessionStore::generate_id();
        let username = self.sessions.username.clone();
        match self.db.fork_context(&parent, &new_id, Some(&username)) {
            Ok(_) => {
                let name = format!("Session {}", self.sessions.sessions.len() + 1);
                self.sessions.sessions.push(Session {
                    id: new_id.clone(),
                    name,
                    username,
                });
                self.sessions.set_active(&new_id);
                self.sessions.save(&self.data_dir);
                self.reset_view();
                self.refresh_transcript();
                self.flash("new session");
            }
            Err(e) => self.flash(format!("fork failed: {e}")),
        }
    }

    fn delete_active_session(&mut self) {
        if self.sessions.sessions.len() <= 1 {
            self.flash("cannot delete the last session");
            return;
        }
        let id = self.active_user_id().to_string();
        let _ = self.db.delete_context(&id);
        self.sessions.sessions.retain(|s| s.id != id);
        let new_active = self.sessions.sessions[0].id.clone();
        self.sessions.set_active(&new_active);
        self.sessions.save(&self.data_dir);
        self.reset_view();
        self.refresh_transcript();
        self.flash("session deleted");
    }

    fn switch_session(&mut self, idx: usize) {
        if let Some(s) = self.sessions.sessions.get(idx) {
            let id = s.id.clone();
            self.sessions.set_active(&id);
            self.sessions.save(&self.data_dir);
            self.reset_view();
            self.refresh_transcript();
        }
    }
}

/// Compute a stable dedup key for a DB message.
fn bubble_key(m: &Message) -> String {
    let prefix: String = m.content.chars().take(80).collect();
    format!("{}::{}", m.role, prefix)
}

/// Find file completion candidates for `@<prefix>`.
///
/// Rules:
/// - If `prefix` is empty → list cwd entries.
/// - If `prefix` ends with `/` → list entries inside that directory.
/// - Otherwise → list entries inside the parent directory whose basename
///   starts with `prefix`'s basename.
///
/// Hidden files (leading `.`) are included only when the prefix's basename
/// itself starts with `.`. Up to 50 results are returned.
pub fn file_matches(prefix: &str) -> Vec<String> {
    use std::path::PathBuf;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let raw = if prefix.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            prefix.replacen('~', &home, 1)
        } else {
            prefix.to_string()
        }
    } else {
        prefix.to_string()
    };
    let pb = PathBuf::from(&raw);
    let (dir, partial) = if raw.is_empty() {
        (cwd.clone(), String::new())
    } else if raw.ends_with('/') {
        (
            if pb.is_absolute() {
                pb.clone()
            } else {
                cwd.join(&pb)
            },
            String::new(),
        )
    } else {
        let parent = pb.parent().map(PathBuf::from).unwrap_or_default();
        let base = pb
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let abs_parent = if parent.as_os_str().is_empty() {
            cwd.clone()
        } else if parent.is_absolute() {
            parent
        } else {
            cwd.join(&parent)
        };
        (abs_parent, base)
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let want_hidden = partial.starts_with('.');
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !want_hidden && name.starts_with('.') {
                return None;
            }
            if !name.to_lowercase().starts_with(&partial.to_lowercase()) {
                return None;
            }
            let mut path = e.path();
            if path.is_dir() {
                path.push("");
            }
            Some(path.to_string_lossy().into_owned())
        })
        .collect();
    out.sort();
    out.truncate(50);
    out
}

/// Walk through the input and split out `@/path/to/file` tokens. Returns
/// `(clean_text_without_attachments, resolved_paths)`. Tokens that don't
/// resolve to an existing file are passed through into `clean_text`.
fn split_at_attachments(input: &str) -> (String, Vec<String>) {
    let mut clean = String::new();
    let mut paths = Vec::new();
    for word in input.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end();
        if let Some(rest) = trimmed.strip_prefix('@') {
            // Try to resolve to an existing path. Tilde expansion + cwd join.
            let p = if let Some(stripped) = rest.strip_prefix('~') {
                if let Ok(home) = std::env::var("HOME") {
                    std::path::PathBuf::from(format!("{home}{stripped}"))
                } else {
                    std::path::PathBuf::from(rest)
                }
            } else {
                std::path::PathBuf::from(rest)
            };
            let abs = if p.is_absolute() {
                p
            } else {
                std::env::current_dir().unwrap_or_default().join(&p)
            };
            if abs.exists() {
                paths.push(abs.to_string_lossy().into_owned());
                // Drop the entire word including its trailing whitespace so
                // we don't leave a double-space in the cleaned text.
                continue;
            }
        }
        clean.push_str(word);
    }
    // Collapse any runs of internal whitespace introduced by the removal.
    let collapsed = clean
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (collapsed, paths)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Event loop
// ─────────────────────────────────────────────────────────────────────────────

pub async fn run() -> anyhow::Result<()> {
    let config = crate::config::Config::from_env();
    let db = Database::new(std::path::Path::new(&config.data_dir))?;

    // Best-effort: pull api key from secrets first, fall back to env.
    let secrets = crate::db::secrets::get_secrets();
    let api_key = secrets
        .gateway_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| config.gateway_api_key.clone());

    let gateway_url = format!("http://127.0.0.1:{}", config.gateway_port);

    let mut app = App::new(db, gateway_url, api_key, config.data_dir.clone());
    app.refresh_transcript();

    // Terminal setup. Bracketed paste so multi-line paste arrives as one event.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app).await;

    // Tear down terminal.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    app.sessions.save(&app.data_dir);
    result
}

async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut status_clear = tokio::time::interval(Duration::from_millis(500));

    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        if app.should_quit {
            break;
        }

        tokio::select! {
            // Keyboard / paste events.
            maybe_event = events.next() => {
                let Some(Ok(ev)) = maybe_event else { continue };
                handle_event(ev, app).await;
            }
            // Periodic poll of the DB for new messages produced by the agent.
            _ = tick.tick() => {
                app.refresh_transcript();
                // Detect agent activity via the registry from agent_loop.
                let user_id = app.active_user_id().to_string();
                let active = crate::gateway::agent_loop::get_user_input_sender(&user_id).await.is_some();
                app.agent_active = active;
            }
            // Auto-clear the flash status message after a couple of seconds.
            _ = status_clear.tick() => {
                if let Some((_, when)) = &app.status_msg {
                    if when.elapsed() > Duration::from_secs(2) {
                        app.status_msg = None;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn handle_event(ev: Event, app: &mut App) {
    match ev {
        Event::Paste(text) => {
            insert_str(app, &text);
            app.update_popup();
        }
        Event::Key(k) if k.kind == KeyEventKind::Press => handle_key(app, k.code, k.modifiers).await,
        _ => {}
    }
}

async fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    // Global keybinds first.
    match (code, mods) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
            app.should_quit = true;
            return;
        }
        (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
            app.show_sidebar = !app.show_sidebar;
            return;
        }
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            app.new_session().await;
            return;
        }
        (KeyCode::PageUp, _) => {
            app.scroll_offset = app.scroll_offset.saturating_add(5);
            return;
        }
        (KeyCode::PageDown, _) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(5);
            return;
        }
        (KeyCode::Char(c), KeyModifiers::ALT) if c.is_ascii_digit() => {
            let idx = (c as u8 - b'1') as usize;
            app.switch_session(idx);
            return;
        }
        _ => {}
    }

    // Popup-specific handling.
    let popup_active = !matches!(app.popup, Popup::None);
    if popup_active {
        match code {
            KeyCode::Esc => {
                app.popup = Popup::None;
                return;
            }
            KeyCode::Up => {
                bump_popup_selection(app, -1);
                return;
            }
            KeyCode::Down => {
                bump_popup_selection(app, 1);
                return;
            }
            KeyCode::Tab | KeyCode::Enter => {
                if accept_popup(app) {
                    return;
                }
            }
            _ => {}
        }
    }

    // Default editing behaviour.
    match code {
        KeyCode::Enter => {
            if mods.contains(KeyModifiers::SHIFT) {
                insert_char(app, '\n');
            } else {
                app.send_message().await;
            }
        }
        KeyCode::Char(c) => {
            insert_char(app, c);
            app.update_popup();
        }
        KeyCode::Backspace => {
            backspace(app);
            app.update_popup();
        }
        KeyCode::Delete => {
            delete_forward(app);
            app.update_popup();
        }
        KeyCode::Left => {
            if app.cursor > 0 {
                app.cursor -= 1;
            }
        }
        KeyCode::Right => {
            if app.cursor < app.input.chars().count() {
                app.cursor += 1;
            }
        }
        KeyCode::Home => app.cursor = 0,
        KeyCode::End => app.cursor = app.input.chars().count(),
        KeyCode::Esc => app.popup = Popup::None,
        _ => {}
    }
}

fn bump_popup_selection(app: &mut App, delta: i32) {
    match &mut app.popup {
        Popup::Slash { matches, selected } => {
            let len = matches.len();
            if len == 0 {
                return;
            }
            *selected = ((*selected as i32 + delta).rem_euclid(len as i32)) as usize;
        }
        Popup::File { matches, selected, .. } => {
            let len = matches.len();
            if len == 0 {
                return;
            }
            *selected = ((*selected as i32 + delta).rem_euclid(len as i32)) as usize;
        }
        Popup::None => {}
    }
}

fn accept_popup(app: &mut App) -> bool {
    let popup = std::mem::replace(&mut app.popup, Popup::None);
    match popup {
        Popup::Slash { matches, selected } => {
            let Some(&idx) = matches.get(selected) else {
                return false;
            };
            let cmd = SLASH_COMMANDS[idx].name.to_string();
            // Replace the slash-token at the start of input with the command + space.
            app.input.clear();
            app.input.push_str(&cmd);
            app.cursor = app.input.chars().count();
            true
        }
        Popup::File {
            prefix, matches, selected, ..
        } => {
            let Some(path) = matches.get(selected).cloned() else {
                return false;
            };
            // Replace the @<prefix> token before the cursor with @<path>.
            replace_at_token(app, &prefix, &path);
            // Refresh popup in case the user wants to keep navigating into a dir.
            app.update_popup();
            true
        }
        Popup::None => false,
    }
}

fn insert_char(app: &mut App, c: char) {
    let mut chars: Vec<char> = app.input.chars().collect();
    let pos = app.cursor.min(chars.len());
    chars.insert(pos, c);
    app.input = chars.into_iter().collect();
    app.cursor += 1;
}

fn insert_str(app: &mut App, s: &str) {
    let mut chars: Vec<char> = app.input.chars().collect();
    let pos = app.cursor.min(chars.len());
    for (i, c) in s.chars().enumerate() {
        chars.insert(pos + i, c);
    }
    let added = s.chars().count();
    app.input = chars.into_iter().collect();
    app.cursor += added;
}

fn backspace(app: &mut App) {
    if app.cursor == 0 {
        return;
    }
    let mut chars: Vec<char> = app.input.chars().collect();
    chars.remove(app.cursor - 1);
    app.input = chars.into_iter().collect();
    app.cursor -= 1;
}

fn delete_forward(app: &mut App) {
    let mut chars: Vec<char> = app.input.chars().collect();
    if app.cursor < chars.len() {
        chars.remove(app.cursor);
        app.input = chars.into_iter().collect();
    }
}

/// Replace the `@<prefix>` token immediately before the cursor with `@<replacement>`.
fn replace_at_token(app: &mut App, prefix: &str, replacement: &str) {
    let mut chars: Vec<char> = app.input.chars().collect();
    // Find `@` immediately preceding cursor (skipping the prefix).
    let token_len = prefix.chars().count() + 1; // +1 for '@'
    let start = app.cursor.saturating_sub(token_len);
    // Verify start is actually an '@'; if not, just append.
    if chars.get(start) != Some(&'@') {
        let appended = format!("@{replacement}");
        for c in appended.chars() {
            chars.insert(app.cursor, c);
            app.cursor += 1;
        }
        app.input = chars.into_iter().collect();
        return;
    }
    chars.drain(start..app.cursor);
    let new_token = format!("@{replacement}");
    let new_token_chars = new_token.chars().count();
    for (i, c) in new_token.chars().enumerate() {
        chars.insert(start + i, c);
    }
    app.cursor = start + new_token_chars;
    app.input = chars.into_iter().collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_at_attachments_basic() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let input = format!("hello @{} world", path);
        let (clean, atts) = split_at_attachments(&input);
        assert_eq!(clean, "hello world");
        assert_eq!(atts, vec![path]);
    }

    #[test]
    fn split_at_attachments_no_match_passes_through() {
        let (clean, atts) = split_at_attachments("hello @/totally/nope/file.txt world");
        assert_eq!(clean, "hello @/totally/nope/file.txt world");
        assert!(atts.is_empty());
    }

    #[test]
    fn file_matches_lists_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _f1 = std::fs::File::create(tmp.path().join("alpha.txt")).unwrap();
        let _f2 = std::fs::File::create(tmp.path().join("alphabet.txt")).unwrap();
        let prev = std::env::current_dir().ok();
        std::env::set_current_dir(tmp.path()).unwrap();
        let out = file_matches("al");
        if let Some(p) = prev {
            let _ = std::env::set_current_dir(p);
        }
        assert!(out.iter().any(|s| s.ends_with("alpha.txt")));
        assert!(out.iter().any(|s| s.ends_with("alphabet.txt")));
    }

    #[test]
    fn replace_at_token_works() {
        let mut app = dummy_app();
        app.input = "see @al".to_string();
        app.cursor = app.input.chars().count();
        replace_at_token(&mut app, "al", "/etc/hosts");
        assert_eq!(app.input, "see @/etc/hosts");
        assert_eq!(app.cursor, app.input.chars().count());
    }

    fn dummy_app() -> App {
        // We don't need a real DB for these helpers; build a thin stand-in.
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        // Keep the tempdir alive for the test by leaking it (cheap).
        Box::leak(Box::new(dir));
        App::new(db, "http://127.0.0.1:0".into(), "x".into(), ".".into())
    }
}
