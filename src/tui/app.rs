//! TUI chat application — state, event loop, and dispatch.

use crate::db::messages::Message;
use crate::db::Database;
use crate::tui::sessions::{Session, SessionStore};
use crate::tui::ui;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
        EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
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
    SlashCmd { name: "/delegations", desc: "Show delegated subtasks (status + result)" },
    SlashCmd { name: "/context", desc: "Get/set context variables (e.g. set foo=bar)" },
    SlashCmd { name: "/thinking", desc: "Reasoning level: off|low|medium|high|xhigh|auto" },
    SlashCmd { name: "/show_thinking", desc: "Show reasoning: on|off" },
    SlashCmd { name: "/rename", desc: "Rename the current session" },
    SlashCmd { name: "/delete", desc: "Delete the current session" },
    SlashCmd { name: "/plugins", desc: "List plugin tools (use: /plugins [enable|disable] <tool_name>)" },
    SlashCmd { name: "/login", desc: "Provider login on the gateway: /login | /login codex | /login <provider> <api_key> [model] [api_base]" },
    SlashCmd { name: "/logout", desc: "Remove a provider credential: /logout <provider>" },
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
    Thinking { content: String },
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
    pub live: super::streaming::LiveOutput,
    pub remote: Option<super::remote::Remote>,
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
    /// Track if user was at bottom before new messages (for auto-scroll).
    pub at_bottom: bool,
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
            live: Default::default(),
            remote: None,
            seen_keys: std::collections::HashSet::new(),
            input: String::new(),
            cursor: 0,
            attachments: Vec::new(),
            popup: Popup::None,
            show_sidebar: true,
            scroll_offset: 0,
            at_bottom: true,
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
        self.live = Default::default();
        self.seen_keys.clear();
        self.scroll_offset = 0;
        self.at_bottom = true;
        self.input.clear();
        self.cursor = 0;
        self.attachments.clear();
        self.popup = Popup::None;
    }

    /// Refresh the transcript from the DB. Idempotent — uses `seen_keys` to
    /// avoid duplicates so it's safe to call on every poll tick.
    pub fn refresh_transcript(&mut self) {
        if self.remote.is_some() { return; }
        let user_id = self.active_user_id().to_string();
        let Ok(msgs) = self.db.get_messages(&user_id, 1000) else {
            return;
        };
        self.append_messages(msgs);
    }

    fn append_messages(&mut self, msgs: Vec<Message>) {
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
                // Discord-mirror rows (persisted for the dashboard chat):
                // shown with an origin tag so TUI and dashboard stay in sync.
                "discord_user" | "discord_bot" => {
                    let author = m
                        .discord_meta
                        .as_ref()
                        .and_then(|v| v.get("author"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(if m.role == "discord_user" { "discord" } else { "bot" });
                    Bubble::System {
                        content: format!("🎮 Discord {}: {}", author, m.content),
                    }
                }
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
                let result = if let Some(remote) = &self.remote {
                    remote.request(reqwest::Method::DELETE, &format!("/v1/messages/{}",urlencoding::encode(self.active_user_id())),None).await.map(|_|())
                } else { self.db.clear_messages(self.active_user_id()) };
                if let Err(e) = result {
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
            "/delegations" => {
                let user_id = self.active_user_id().to_string();
                if self.remote.is_some() {
                    self.flash("Use the backend dashboard for delegated subtasks");
                    return;
                }
                match crate::gateway::delegation::list_delegations(&self.db, &user_id) {
                    Ok(list) if list.is_empty() => {
                        self.transcript.push(Bubble::Banner {
                            kind: BannerKind::Info,
                            content: "No delegations yet.".to_string(),
                        });
                    }
                    Ok(list) => {
                        let body = list
                            .iter()
                            .map(|d| {
                                let status_icon = match d.status.as_str() {
                                    "done" => "✅",
                                    "failed" => "❌",
                                    _ => "⏳",
                                };
                                let task_short: String = d.task.chars().take(80).collect();
                                let result_short = d
                                    .result
                                    .as_deref()
                                    .map(|r| {
                                        let r = r.replace('\n', " ");
                                        r.chars().take(120).collect::<String>()
                                    })
                                    .unwrap_or_else(|| "-".to_string());
                                format!(
                                    "{} {} [{}] {}\n    result: {}",
                                    status_icon, d.id, d.status, task_short, result_short
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.transcript.push(Bubble::Banner {
                            kind: BannerKind::Info,
                            content: format!("🤝 Delegations:\n{}", body),
                        });
                    }
                    Err(e) => {
                        self.transcript.push(Bubble::Banner {
                            kind: BannerKind::Error,
                            content: format!("/delegations: {}", e),
                        });
                    }
                }
            }
            "/thinking" | "/show_thinking" => {
                let value = line.split_whitespace().nth(1).unwrap_or("").to_ascii_lowercase();
                let setting = if name == "/show_thinking" {
                    match value.as_str() {
                        "on" => Some(("show_thinking", serde_json::json!(true))),
                        "off" => Some(("show_thinking", serde_json::json!(false))),
                        _ => None,
                    }
                } else if ["off", "on", "low", "medium", "high", "xhigh", "auto"].contains(&value.as_str()) {
                    Some(("thinking_mode", serde_json::json!(value)))
                } else { None };
                if let Some((key, value)) = setting {
                    let result = if let Some(remote) = &self.remote {
                        remote.command(self.active_user_id(), &format!("/context set settings.{key}={value}")).await.map(|_|())
                    } else { self.db.merge_context(self.active_user_id(), serde_json::json!({format!("settings.{key}"):value})).map(|_|()) };
                    match result {
                        Ok(_) => self.flash("Thinking setting saved"),
                        Err(e) => self.flash(format!("{e}")),
                    }
                } else { self.flash("Invalid thinking setting; see /help"); }
            }
            "/context" | "/ctx" => {
                if let Some(remote) = &self.remote {
                    let response = remote.command(self.active_user_id(),line).await;
                    self.transcript.push(Bubble::System {content:response.unwrap_or_else(|e|format!("{e}"))});
                } else { self.run_context_command(line); }
            }
            "/rename" => {
                let title = _rest.trim();
                if title.is_empty() || title.chars().count() > 128 {
                    self.flash("Usage: /rename <name> (1–128 characters)"); return;
                }
                let id = self.active_user_id().to_string();
                let result = if let Some(remote) = &self.remote {
                    remote.command(&id,&format!("/context set custom_data.session_title={}",serde_json::json!(title))).await.map(|_|())
                } else { self.db.merge_context(&id,serde_json::json!({"custom_data.session_title":title})).map(|_|()) };
                match result {
                    Ok(_) => {
                        if let Some(session) = self.sessions.sessions.iter_mut().find(|s|s.id==id) { session.name=title.into(); }
                        self.sessions.save(&self.data_dir);
                        self.flash(format!("Renamed to {title}; context: {id}"));
                    }
                    Err(e) => self.flash(format!("Rename failed: {e}")),
                }
            }
            "/delete" => {
                self.delete_active_session().await;
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
            "/login" | "/logout" => {
                self.run_login(&name, &_rest).await;
            }
            "/quit" | "/exit" => {
                self.should_quit = true;
            }
            _ => {
                self.flash(format!("unknown command: {name}"));
            }
        }
    }

    /// Provider login. Always executed by the gateway (local or remote), because
    /// that is the process that talks to the model; a remote TUI therefore logs
    /// the remote machine in. Credentials travel once, over the authenticated
    /// gateway connection, and are not stored by the TUI.
    async fn run_login(&mut self, verb: &str, rest: &str) {
        let args = shell_words(rest);
        let request = match parse_login_args(verb, &args) {
            Ok(None) => {
                match self.gateway_get("/v1/providers").await {
                    Ok(v) => {
                        let text = v["text"].as_str().unwrap_or("No provider information").to_string();
                        self.transcript.push(Bubble::Banner { kind: BannerKind::Info, content: text });
                    }
                    Err(e) => self.transcript.push(Bubble::Banner { kind: BannerKind::Error, content: format!("/login: {e}") }),
                }
                return;
            }
            Ok(Some(request)) => request,
            Err(usage) => { self.flash(usage); return; }
        };
        self.flash(format!("Contacting gateway for {} login…", request["provider"].as_str().unwrap_or("provider")));
        match self.gateway_post("/v1/providers/login", request).await {
            Ok(v) => {
                let mut content = v["message"].as_str().unwrap_or("Done").to_string();
                let setup: Vec<&str> = v["setup"].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).collect()).unwrap_or_default();
                if !setup.is_empty() {
                    content.push_str("\n\nNext steps:\n  ");
                    content.push_str(&setup.join("\n  "));
                }
                let kind = if v["status"] == "pending" { BannerKind::Info } else { BannerKind::AgentStop };
                self.transcript.push(Bubble::Banner { kind, content });
            }
            Err(e) => self.transcript.push(Bubble::Banner { kind: BannerKind::Error, content: format!("{verb}: {e}") }),
        }
    }

    async fn gateway_get(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        if let Some(remote) = &self.remote {
            return remote.request(reqwest::Method::GET, path, None).await;
        }
        let response = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?
            .get(format!("{}{path}", self.gateway_url.trim_end_matches('/'))).bearer_auth(&self.gateway_api_key).send().await
            .map_err(|_| anyhow::anyhow!("Gateway not reachable at {} (is `praxis run` running?)", self.gateway_url))?;
        anyhow::ensure!(response.status().is_success(), "Gateway returned HTTP {}", response.status().as_u16());
        Ok(response.json().await?)
    }

    async fn gateway_post(&self, path: &str, body: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        // Login can take a while (endpoint probes, spawning the Codex CLI).
        let client = reqwest::Client::builder().timeout(Duration::from_secs(60)).build()?;
        let response = client.post(format!("{}{path}", self.gateway_url.trim_end_matches('/'))).bearer_auth(&self.gateway_api_key).json(&body).send().await
            .map_err(|_| anyhow::anyhow!("Gateway not reachable at {} (is `praxis run` running?)", self.gateway_url))?;
        let status = response.status();
        let value: serde_json::Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            let detail = value["error"].as_str().unwrap_or("request failed");
            anyhow::bail!("{detail} (HTTP {})", status.as_u16());
        }
        Ok(value)
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
                .bearer_auth(&api_key)
                .json(&body)
                .send()
                .await;
        });
        self.agent_active = true;
        self.flash("sent");
    }

    async fn stop_agent(&mut self) {
        let user_id = self.active_user_id().to_string();
        let url = format!("{}/v1/stop/{}", self.gateway_url.trim_end_matches('/'), urlencoding::encode(&user_id));
        if reqwest::Client::new().post(url).bearer_auth(&self.gateway_api_key)
            .timeout(Duration::from_secs(5)).send().await.and_then(|r| r.error_for_status()).is_err() {
            self.flash("Failed to send stop signal to gateway");
            return;
        }
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
        let result = if let Some(remote) = &self.remote {
            remote.request(reqwest::Method::POST,&format!("/v1/context/{}/fork",urlencoding::encode(&parent)),
                Some(serde_json::json!({"new_user_id":new_id,"username":username}))).await.map(|_|())
        } else { self.db.fork_context(&parent,&new_id,Some(&username)).map(|_|()) };
        match result {
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

    async fn delete_active_session(&mut self) {
        if self.sessions.sessions.len() <= 1 {
            self.flash("cannot delete the last session");
            return;
        }
        let id = self.active_user_id().to_string();
        let result = if let Some(remote) = &self.remote {
            remote.request(reqwest::Method::DELETE,&format!("/v1/context/{}",urlencoding::encode(&id)),None).await.map(|_|())
        } else { self.db.delete_context(&id) };
        if let Err(e) = result { self.flash(format!("{e}")); return; }
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

pub async fn run(gateway_url: Option<String>, gateway_key: Option<String>) -> anyhow::Result<()> {
    let config = crate::config::Config::from_env();
    // Remote mode never opens the local backend DB or loads local backend secrets.
    // A temporary empty DB satisfies legacy UI state only; all remote reads and
    // writes below use the authenticated API, and are not cached as local truth.
    let remote_dir = if gateway_url.is_some() { Some(tempfile::tempdir()?) } else { None };
    let data_dir = remote_dir.as_ref().map(|d|d.path().to_string_lossy().to_string()).unwrap_or_else(||config.data_dir.clone());
    let db = Database::new(std::path::Path::new(&data_dir))?;
    let key = gateway_key.or_else(||std::env::var("PRAXIS_GATEWAY_KEY").ok());
    let api_key = if gateway_url.is_some() {
        key.ok_or_else(||anyhow::anyhow!("Remote chat requires --gateway-key or PRAXIS_GATEWAY_KEY"))?
    } else {
        key.or_else(||crate::db::secrets::get_secrets().gateway_api_key.filter(|k|!k.is_empty()))
            .unwrap_or_else(||config.gateway_api_key.clone())
    };
    let is_remote = gateway_url.is_some();
    let gateway_url = gateway_url.unwrap_or_else(||format!("http://127.0.0.1:{}",config.gateway_port));
    let mut app = App::new(db, gateway_url.clone(), api_key.clone(), data_dir);
    if is_remote {
        let remote = super::remote::Remote::new(gateway_url,api_key)?;
        let data = remote.request(reqwest::Method::GET,"/v1/sessions",None).await?;
        let sessions: Vec<Session> = serde_json::from_value(data["sessions"].clone())?;
        if !sessions.is_empty() {
            app.sessions.active = sessions[0].id.clone();
            app.sessions.sessions = sessions;
        } else {
            let id = SessionStore::generate_id();
            app.sessions.sessions[0].id = id.clone(); app.sessions.active = id;
        }
        app.append_messages(remote.messages(app.active_user_id()).await?);
        app.remote = Some(remote);
    } else { app.refresh_transcript(); }

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
    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(1024);
    let mut stream_user = String::new();
    let mut stream_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut history_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        if stream_user != app.active_user_id() {
            if let Some(task) = stream_task.take() { task.abort(); }
            if let Some(task) = history_task.take() { task.abort(); }
            stream_user = app.active_user_id().to_string();
            if let Some(remote) = app.remote.clone() {
                let user = stream_user.clone(); let tx = stream_tx.clone();
                history_task = Some(tokio::spawn(async move {
                    loop {
                        if let Ok(messages) = remote.messages(&user).await {
                            if let Ok(data) = serde_json::to_string(&messages) {
                                if tx.send((user.clone(), "history".into(), data)).await.is_err() { break; }
                            }
                        }
                        if tx.is_closed() { break; }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }));
            }
            stream_task = Some(super::streaming::subscribe(app.gateway_url.clone(), app.gateway_api_key.clone(), stream_user.clone(), stream_tx.clone()));
        }
        terminal.draw(|f| ui::draw(f, app))?;
        if app.should_quit {
            if let Some(task) = stream_task.take() { task.abort(); }
            if let Some(task) = history_task.take() { task.abort(); }
            break;
        }

        tokio::select! {
            Some((user, event, data)) = stream_rx.recv() => {
                if user == app.active_user_id() {
                    if event == "history" {
                        if let Ok(messages) = serde_json::from_str(&data) { app.append_messages(messages); }
                    } else if event == "reasoning" {
                        app.transcript.push(Bubble::Thinking { content: data.clone() });
                        app.live.reasoning.clear();
                    } else {
                        app.live.apply(&event, &data);
                    }
                    if event == "stream_end" { app.live.tools.clear(); }
                    if event == "assistant_saved" { app.refresh_transcript(); }
                    if event == "agent_start" { app.agent_active = true; }
                    if event == "agent_stop" { app.agent_active = false; }
                }
            }
            // Keyboard / paste events.
            maybe_event = events.next() => {
                let Some(Ok(ev)) = maybe_event else { continue };
                handle_event(ev, app).await;
            }
            // Periodic poll of the DB for new messages produced by the agent.
            _ = tick.tick() => {
                app.refresh_transcript();
                // Activity is received from the gateway, not this client's
                // process-local registry (which never owns the remote task).
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
        Event::Mouse(me) => {
            match me.kind {
                MouseEventKind::ScrollUp => {
                    app.scroll_offset = app.scroll_offset.saturating_add(3);
                    app.at_bottom = false;
                }
                MouseEventKind::ScrollDown => {
                    app.scroll_offset = app.scroll_offset.saturating_sub(3);
                    if app.scroll_offset == 0 {
                        app.at_bottom = true;
                    }
                }
                _ => {}
            }
        }
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
            app.at_bottom = false;
            return;
        }
        (KeyCode::PageDown, _) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(5);
            if app.scroll_offset == 0 {
                app.at_bottom = true;
            }
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


/// Minimal shell-like splitting: whitespace separated, single/double quotes group.
fn shell_words(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_token = false;
    for ch in input.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"') | (None, '\'') => { quote = Some(ch); has_token = true; }
            (None, c) if c.is_whitespace() => { if has_token || !current.is_empty() { out.push(std::mem::take(&mut current)); has_token = false; } }
            (None, c) => { current.push(c); has_token = true; }
        }
    }
    if has_token || !current.is_empty() { out.push(current); }
    out
}

/// Turn `/login …` / `/logout …` arguments into a gateway request.
/// `Ok(None)` means "show status".
fn parse_login_args(verb: &str, args: &[String]) -> Result<Option<serde_json::Value>, String> {
    let usage = "Usage: /login | /login codex [--auth-json '<json>'] | /login <openai|anthropic|openrouter|minimax|mimo> <api_key> [model] [api_base] | /login <ollama|llamacpp> [api_base] [model] | /logout <provider>";
    let Some(provider) = args.first() else {
        return if verb == "/logout" { Err(usage.into()) } else { Ok(None) };
    };
    let provider = provider.to_ascii_lowercase();
    if verb == "/logout" {
        return Ok(Some(serde_json::json!({"provider": provider, "logout": true})));
    }
    let mut request = serde_json::json!({"provider": provider});
    let rest = &args[1..];
    match provider.as_str() {
        "codex" => {
            if let Some(i) = rest.iter().position(|a| a == "--auth-json") {
                let json = rest.get(i + 1).ok_or(usage)?;
                request["auth_json"] = serde_json::json!(json);
            }
            if let Some(model) = rest.iter().find(|a| !a.starts_with("--") && !a.starts_with('{')) {
                request["model"] = serde_json::json!(model);
            }
        }
        "ollama" | "llamacpp" => {
            for arg in rest {
                if arg.starts_with("http://") || arg.starts_with("https://") { request["api_base"] = serde_json::json!(arg); }
                else if request.get("model").is_none() { request["model"] = serde_json::json!(arg); }
                else { request["api_key"] = serde_json::json!(arg); }
            }
        }
        _ => {
            let key = rest.first().ok_or(usage)?;
            request["api_key"] = serde_json::json!(key);
            for arg in &rest[1..] {
                if arg.starts_with("http://") || arg.starts_with("https://") { request["api_base"] = serde_json::json!(arg); }
                else { request["model"] = serde_json::json!(arg); }
            }
        }
    }
    Ok(Some(request))
}

#[cfg(test)]
mod login_parse_tests {
    use super::*;

    #[test]
    fn login_arguments_map_to_gateway_requests() {
        assert_eq!(parse_login_args("/login", &[]).unwrap(), None);
        assert!(parse_login_args("/logout", &[]).is_err());
        let r = parse_login_args("/login", &shell_words("openai sk-test-1234 gpt-4.1 https://proxy.example/v1")).unwrap().unwrap();
        assert_eq!(r["api_key"], "sk-test-1234");
        assert_eq!(r["model"], "gpt-4.1");
        assert_eq!(r["api_base"], "https://proxy.example/v1");
        assert!(parse_login_args("/login", &shell_words("anthropic")).is_err(), "key required");
        let r = parse_login_args("/login", &shell_words("ollama http://gpu:11434 qwen3:8b")).unwrap().unwrap();
        assert_eq!(r["api_base"], "http://gpu:11434");
        assert_eq!(r["model"], "qwen3:8b");
        let r = parse_login_args("/login", &shell_words("codex --auth-json '{\"tokens\":{}}' gpt-5")).unwrap().unwrap();
        assert_eq!(r["auth_json"], "{\"tokens\":{}}");
        assert_eq!(r["model"], "gpt-5");
        let r = parse_login_args("/logout", &shell_words("Codex")).unwrap().unwrap();
        assert_eq!(r["logout"], true);
        assert_eq!(r["provider"], "codex");
    }
}
