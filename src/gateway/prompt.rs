//! Shared POML contract for both message paths and dashboard previews.
//! Routing is performed before rendering; builders never consult secrets.
use crate::db::{contexts::Context, Database};
use crate::plugins::PluginRegistry;
use serde_json::{json, Value};
use std::path::Path;

pub fn workflow_name(ctx: &Context) -> &str {
    ctx.settings
        .sm_file
        .as_deref()
        .or(ctx.sm_file.as_deref())
        .unwrap_or("standard")
}

/// Prepare a context without persistence (also used by preview). All persona
/// decisions belong in the selected SM file, never in the message handler.
pub fn route_context(
    root: &Path,
    ctx: &mut Context,
    input: &str,
    plugins: &PluginRegistry,
    channel_id: Option<&str>,
) -> anyhow::Result<()> {
    if ctx.custom_data.is_null() {
        ctx.custom_data = json!({});
    }
    let data = ctx
        .custom_data
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("custom_data must be an object"))?;
    for (key, value) in plugins.context_defaults() {
        data.entry(key).or_insert(value);
    }
    data.insert("user_prompt".into(), json!(input));
    data.entry("user_template".to_string())
        .or_insert(json!("user"));
    if let Some(channel) = channel_id
        .filter(|s| !s.is_empty())
        .or(ctx.settings.feedback_channel_id.as_deref())
    {
        data.entry("channel_id".to_string())
            .or_insert(json!(channel));
    }
    if ctx.active_state.is_none() {
        ctx.active_state = ctx.settings.active_state.clone();
    }
    let workflow = crate::sm::load_file_in(&root.join("contexts"), workflow_name(ctx))
        .map_err(|e| anyhow::anyhow!("Workflow routing failed: {e}"))?;
    let mut value = serde_json::to_value(&*ctx)?;
    // Secret overrides are deliberately not applied to global credentials.
    let _ = crate::sm::apply_to_context(&workflow, &mut value);
    crate::db::contexts::normalize_legacy_keys(&mut value);
    let mut routed: Context = serde_json::from_value(value)?;
    anyhow::ensure!(
        routed.user_id == ctx.user_id && routed.session_id == ctx.session_id,
        "Workflow must not change user/session identity"
    );
    if let Some(active) = routed.active_state.as_deref().filter(|s| !s.is_empty()) {
        anyhow::ensure!(
            workflow.states.contains_key(active),
            "Workflow selected an undefined state: {active}"
        );
    }
    anyhow::ensure!(routed.settings.active_skill == ctx.settings.active_skill,
        "Persistent skill selection is user-only; workflows must use task-local use_skill instead");
    routed.settings.active_state = routed.active_state.clone();
    if routed.custom_data.is_null() {
        routed.custom_data = json!({});
    }
    anyhow::ensure!(
        routed.custom_data.is_object(),
        "Workflow custom_data must remain an object"
    );
    routed.custom_data["user_prompt"] = json!(input);
    crate::gateway::templates::resolve_template(
        &root.join("templates"),
        routed
            .settings
            .system_template
            .as_deref()
            .unwrap_or("standard"),
    )?;
    *ctx = routed;
    Ok(())
}

pub fn prepare_runtime(
    state: &super::GatewayState,
    user_id: &str,
    input: &str,
    turn: Option<i32>,
    channel_id: Option<&str>,
) -> anyhow::Result<Context> {
    let mut ctx = state.db.load_context(user_id)?;
    if let Some(turn) = turn {
        ctx.turn = turn;
    }
    route_context(Path::new("."), &mut ctx, input, &state.plugins, channel_id)?;
    state.db.save_context(&ctx)?;
    Ok(ctx)
}

/// Prefer explicit preview input, then the last raw runtime input. On legacy
/// contexts fall back to the most recent USER record, never an assistant/tool.
pub fn preview_input(
    db: &Database,
    ctx: &Context,
    explicit: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(input) = explicit {
        return Ok(input.to_string());
    }
    if let Some(input) = ctx.custom_data.get("user_prompt").and_then(Value::as_str) {
        return Ok(input.to_string());
    }
    let (messages, _) = db.get_messages_with_token_budget(&ctx.user_id, usize::MAX)?;
    Ok(messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "Preview message".into()))
}

/// Complete non-sensitive defaults, shared by runtime/preview enrichment and
/// synthetic template validation. No database, provider or secret lookup.
pub fn base_context(ctx: &Context, input: &str) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(ctx)?;
    value["username"] = json!(ctx.username.as_deref().unwrap_or("User"));
    value["settings"]["system_template"] = json!(ctx
        .settings
        .system_template
        .as_deref()
        .unwrap_or("standard"));
    value["system_template"] = value["settings"]["system_template"].clone();
    value["sm_file"] = json!(workflow_name(ctx));
    value["active_state"] = json!(ctx
        .active_state
        .as_deref()
        .or(ctx.settings.active_state.as_deref())
        .unwrap_or(""));
    value["active_templates"] = json!(if ctx.active_templates.is_empty() {
        &ctx.settings.active_templates
    } else {
        &ctx.active_templates
    });
    value["user_prompt"] = json!(input);
    value["user_message"] = json!(input);
    value["system_info"] = json!(format!("Praxis v{}", env!("CARGO_PKG_VERSION")));
    value["path"] = json!(if ctx.settings.path.is_empty() {
        std::env::current_dir()?.to_string_lossy().to_string()
    } else {
        ctx.settings.path.clone()
    });
    value["time"] = json!(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    if !value["custom_data"].is_object() {
        value["custom_data"] = json!({});
    }
    value["custom_data"]["user_prompt"] = json!(input);
    if value["sm_data"].is_null() {
        value["sm_data"] = json!({});
    }
    value["used_tools_history_size"] = json!(ctx.settings.tool_history_limit);
    value["tag_instructions"] = json!(if ctx.settings.tags_enabled {
        crate::tags::get_tag_instructions()
    } else {
        ""
    });
    value["user_template"] = json!(ctx
        .custom_data
        .get("user_template")
        .and_then(Value::as_str)
        .unwrap_or("user"));
    value["active_skill"] = json!(ctx.settings.active_skill.as_deref().unwrap_or(""));
    value["active_skill_instructions"] = json!("");
    value["skills"] = json!([]);
    value["skill_discovery_instructions"] = json!("");
    value["tools"] = json!([]);
    value["memory"] = json!({"facts": [], "topics": [], "preferences": {}, "variables": {}});
    value["paired_users"] = json!([]);
    value["paired_users_count"] = json!(0);
    value["uptime_secs"] = json!(0);
    value["uptime"] = json!("0m 0s");
    value["conversation_text"] = json!(format!("user: {input}"));
    value["tokens_used"] = json!(0);
    value["tokens_limit"] = json!(ctx.settings.history_token_limit.unwrap_or(500000));
    value["tokens_percentage"] = json!("0.0");
    value["compaction_token_limit"] = json!(ctx.settings.compaction_token_limit.unwrap_or(500000));
    value["compaction_percentage"] = json!("0.0");
    value["message_count"] = json!(0);
    Ok(value)
}

pub async fn build_context(
    db: &Database,
    ctx: &Context,
    input: &str,
    plugins: &PluginRegistry,
    uptime_secs: u64,
    root: &Path,
) -> anyhow::Result<Value> {
    let memory = crate::db::memory::load_memory(db, &ctx.user_id)?;
    let mut tools = crate::db::tools::to_tool_definitions(db)?;
    tools.extend(plugins.tool_definitions());
    let tools: Vec<_> = tools.iter().map(|t| json!({"name": t.function.name, "description": t.function.description, "parameters": t.function.parameters})).collect();
    let (messages, tokens_used) = db.get_messages_with_token_budget(&ctx.user_id, usize::MAX)?;
    let token_limit = ctx.settings.history_token_limit.unwrap_or(500000);
    let compaction_limit = ctx.settings.compaction_token_limit.unwrap_or(500000);
    let percentage = |limit: usize| {
        if limit == 0 {
            "0.0".to_string()
        } else {
            format!(
                "{:.1}",
                (tokens_used as f64 / limit as f64 * 100.0).min(100.0)
            )
        }
    };
    // Keep compatibility metadata scoped to this user, not other users' IDs.
    let paired: Vec<_> = db.list_all_pairings()?.into_iter().filter(|p| p.user_id == ctx.user_id).map(|p| json!({"user_id": p.user_id, "discord_user_id": p.discord_user_id, "paired_at": p.paired_at})).collect();
    let mut value = base_context(ctx, input)?;
    value["uptime_secs"] = json!(uptime_secs);
    value["uptime"] = json!(format!("{}m {}s", uptime_secs / 60, uptime_secs % 60));
    value["paired_users_count"] = json!(paired.len());
    value["paired_users"] = json!(paired);
    value["tools"] = json!(tools);
    value["memory"] = json!({"facts": memory.learned_facts, "topics": memory.last_topics, "preferences": memory.user_preferences, "variables": memory.custom_variables});
    value["conversation_text"] = json!(messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n"));
    value["tokens_used"] = json!(tokens_used);
    value["tokens_limit"] = json!(token_limit);
    value["tokens_percentage"] = json!(percentage(token_limit));
    value["compaction_token_limit"] = json!(compaction_limit);
    value["compaction_percentage"] = json!(percentage(compaction_limit));
    value["message_count"] = json!(messages.len());
    crate::skills::discovery::enrich(db, &mut value, root).await?;
    if let Some(name) = ctx.settings.active_skill.as_deref() {
        anyhow::ensure!(
            crate::db::tools::get(db, "use_skill")?.is_enabled,
            "Active skill cannot load: use_skill is disabled"
        );
        let skill = crate::skills::lookup_skill(db, &root.join("skills"), name)?;
        let parameters = skill.parameters_for_task(&value, input)?;
        value["active_skill_instructions"] =
            json!(crate::skills::execute_skill(&skill, &parameters).await?);
    }
    Ok(value)
}

pub async fn render_system(
    state: &super::GatewayState,
    ctx: &Context,
    input: &str,
) -> anyhow::Result<String> {
    let value = build_context(
        &state.db,
        ctx,
        input,
        &state.plugins,
        state.start_time.elapsed().as_secs(),
        Path::new("."),
    )
    .await?;
    let path = super::templates::resolve_template(
        Path::new("templates"),
        ctx.settings
            .system_template
            .as_deref()
            .unwrap_or("standard"),
    )?;
    let mut rendered = super::poml::render_strict(&path.to_string_lossy(), &value).await?;
    if let Some(instructions) = value["active_skill_instructions"]
        .as_str()
        .filter(|s| !s.is_empty())
    {
        rendered.push_str(&format!("\n\nActive skill '{}': instructions only, not completed actions. Tool permissions remain unchanged.\n{}", ctx.settings.active_skill.as_deref().unwrap_or(""), instructions));
    }
    Ok(rendered)
}

pub async fn append_injected_message(
    state: &super::GatewayState,
    user_id: &str,
    input: &str,
) -> anyhow::Result<()> {
    let ctx = prepare_runtime(state, user_id, input, None, None)?;
    let rendered = render_user(state, &ctx, input).await?;
    state
        .db
        .add_message(user_id, &crate::db::messages::Message::user(rendered))?;
    Ok(())
}

pub async fn render_user(
    state: &super::GatewayState,
    ctx: &Context,
    input: &str,
) -> anyhow::Result<String> {
    let name = ctx
        .custom_data
        .get("user_template")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let path = super::templates::resolve_template(Path::new("templates"), name)?;
    let value = build_context(
        &state.db,
        ctx,
        input,
        &state.plugins,
        state.start_time.elapsed().as_secs(),
        Path::new("."),
    )
    .await?;
    super::poml::render_strict(&path.to_string_lossy(), &value).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercise the shipped workflows/templates without changing the working
    // directory, editing fixtures in the repository, or invoking POML/LLMs.
    fn route_shipped(ctx: &mut Context, input: &str) {
        route_context(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            ctx,
            input,
            &PluginRegistry::new(),
            None,
        )
        .unwrap();
    }

    #[test]
    fn backend_repo_language_instructor_request() {
        for input in [
            "Be my language instructor.",
            "Teach me Japanese.",
            "Sei bitte mein Sprachtrainer.",
        ] {
            let mut ctx = Context { user_id: "alice".into(), ..Default::default() };
            route_shipped(&mut ctx, input);
            assert_eq!(ctx.settings.system_template.as_deref(), Some("language_instructor"), "{input}");
            assert_eq!(ctx.active_state.as_deref(), Some("routing"));
            assert_eq!(ctx.custom_data["user_prompt"], input);
        }
    }

    #[test]
    fn backend_repo_language_instructor_sticks_on_next_ordinary_task() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut ctx = Context { user_id: "alice".into(), ..Default::default() };
        route_shipped(&mut ctx, "Be my language instructor.");
        db.save_context(&ctx).unwrap();
        let mut next = db.load_context("alice").unwrap();
        let input = "Explain why this sentence uses the past tense.";
        route_shipped(&mut next, input);
        assert_eq!(next.settings.system_template.as_deref(), Some("language_instructor"));
        assert_eq!(next.custom_data["user_prompt"], input);
    }

    #[test]
    fn backend_repo_manual_template_is_preserved() {
        let mut ctx = Context { user_id: "alice".into(), ..Default::default() };
        ctx.settings.system_template = Some("researcher".into());
        route_shipped(&mut ctx, "Compare the evidence for these two claims.");
        assert_eq!(ctx.settings.system_template.as_deref(), Some("researcher"));
    }

    #[test]
    fn backend_repo_quoted_role_request_does_not_switch_template() {
        for input in [
            r#""Be my language instructor" is a quoted example; explain its wording."#,
            r#"Translate "Be my language instructor" into French."#,
            "> Be my language instructor",
        ] {
            let mut ctx = Context { user_id: "alice".into(), ..Default::default() };
            ctx.settings.system_template = Some("researcher".into());
            route_shipped(&mut ctx, input);
            assert_eq!(ctx.settings.system_template.as_deref(), Some("researcher"), "{input}");
        }
    }

    #[test]
    fn backend_repo_role_reset_selects_standard() {
        for reset in ["reset role.", "return to standard"] {
            let mut ctx = Context { user_id: "alice".into(), ..Default::default() };
            route_shipped(&mut ctx, "Be my language instructor.");
            assert_eq!(ctx.settings.system_template.as_deref(), Some("language_instructor"));
            route_shipped(&mut ctx, reset);
            assert_eq!(ctx.settings.system_template.as_deref(), Some("standard"), "{reset}");
            assert_eq!(base_context(&ctx, reset).unwrap()["system_template"], "standard");
        }
    }

    #[test]
    fn backend_repo_all_shipped_sm_states_resolve_templates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let contexts = root.join("contexts");
        let mut names: Vec<_> = std::fs::read_dir(&contexts)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sm"))
            .map(|path| path.file_stem().unwrap().to_str().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["chat", "coding", "default", "self_learning", "standard"]);
        let mut explicit_selections = 0;
        for name in names {
            let sm = crate::sm::load_file_in(&contexts, &name).unwrap();
            assert!(!sm.states.is_empty(), "{name}");
            for (state_name, state) in &sm.states {
                let mut ctx = Context {
                    user_id: "alice".into(),
                    sm_file: Some(name.clone()),
                    active_state: Some(state_name.clone()),
                    ..Default::default()
                };
                route_shipped(&mut ctx, "Ordinary task with no role change.");
                if let Some(selected) = state.variables.get("settings.system_template") {
                    explicit_selections += 1;
                    assert_eq!(ctx.settings.system_template.as_deref(), Some(selected.as_str()), "{name}:{state_name}");
                }
                let selected = ctx.settings.system_template.as_deref().unwrap_or("standard");
                crate::gateway::templates::resolve_template(&root.join("templates"), selected)
                    .unwrap_or_else(|error| panic!("{name}:{state_name} -> {selected}: {error}"));
            }
        }
        assert!(explicit_selections > 0);
    }

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("contexts")).unwrap();
        std::fs::create_dir(root.path().join("templates")).unwrap();
        for name in ["standard", "selected"] {
            std::fs::write(
                root.path().join(format!("templates/{name}.poml")),
                "<poml><p>test</p></poml>",
            )
            .unwrap();
        }
        std::fs::write(root.path().join("contexts/standard.sm"), "@steps [base]\n[state base]\n[overrides]\nif custom_data.user_prompt =~ \"route-me\" -> settings.system_template = \"selected\"\n").unwrap();
        root
    }

    #[test]
    fn backend_routing_sees_current_input_before_template_selection() {
        let root = fixture();
        let mut ctx = Context {
            user_id: "alice".into(),
            custom_data: json!({"user_prompt":"stale"}),
            ..Default::default()
        };
        route_context(
            root.path(),
            &mut ctx,
            "route-me now",
            &PluginRegistry::new(),
            None,
        )
        .unwrap();
        assert_eq!(ctx.settings.system_template.as_deref(), Some("selected"));
        assert_eq!(ctx.custom_data["user_prompt"], "route-me now");
        assert_eq!(ctx.active_state, ctx.settings.active_state);
        ctx.settings.system_template = None;
        route_context(
            root.path(),
            &mut ctx,
            "no match",
            &PluginRegistry::new(),
            None,
        )
        .unwrap();
        assert!(ctx.settings.system_template.is_none());
    }

    #[test]
    fn backend_changed_workflow_does_not_keep_an_unknown_old_state() {
        let root = fixture();
        std::fs::write(
            root.path().join("contexts/alternate.sm"),
            "@steps [new]\n[state new]\nsettings.system_template = \"selected\"\n",
        )
        .unwrap();
        let db = Database::new(&root.path().join("db")).unwrap();
        let mut ctx = Context {
            user_id: "alice".into(),
            ..Default::default()
        };
        let plugins = PluginRegistry::new();
        route_context(root.path(), &mut ctx, "input", &plugins, None).unwrap();
        db.save_context(&ctx).unwrap();
        db.merge_context("alice", json!({"settings.sm_file":"alternate"}))
            .unwrap();
        let mut fresh = db.load_context("alice").unwrap();
        route_context(root.path(), &mut fresh, "current input", &plugins, None).unwrap();
        assert_eq!(fresh.active_state.as_deref(), Some("new"));
        assert_eq!(fresh.settings.system_template.as_deref(), Some("selected"));
        assert_eq!(fresh.custom_data["user_prompt"], "current input");
    }

    #[test]
    fn backend_preview_never_uses_assistant_or_tool_as_current_input() {
        let root = fixture();
        let db = Database::new(&root.path().join("db")).unwrap();
        let mut ctx = Context {
            user_id: "alice".into(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();
        db.add_message(
            "alice",
            &crate::db::messages::Message::user("user input".into()),
        )
        .unwrap();
        db.add_message(
            "alice",
            &crate::db::messages::Message::assistant("assistant output".into()),
        )
        .unwrap();
        assert_eq!(preview_input(&db, &ctx, None).unwrap(), "user input");
        ctx.custom_data = json!({"user_prompt":"raw runtime input"});
        assert_eq!(preview_input(&db, &ctx, None).unwrap(), "raw runtime input");
        assert_eq!(
            preview_input(&db, &ctx, Some("explicit")).unwrap(),
            "explicit"
        );
    }

    #[tokio::test]
    async fn backend_active_skill_rejects_unknown_disabled_or_unmapped_inputs() {
        let root = fixture();
        let db = Database::new(&root.path().join("db")).unwrap();
        crate::db::tools::init_default_tools(&db).unwrap();
        let mut ctx = Context {
            user_id: "alice".into(),
            ..Default::default()
        };
        ctx.settings.active_skill = Some("unregistered".into());
        let plugins = PluginRegistry::new();
        assert!(build_context(&db, &ctx, "task", &plugins, 0, root.path())
            .await
            .unwrap_err()
            .to_string()
            .contains("not registered"));
        let skill = root.path().join("skills/test");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("skill.json"),
            r#"{"name":"test","description":"Test", "required_parameters":["unmapped"]}"#,
        )
        .unwrap();
        std::fs::write(skill.join("skill.poml"), "<poml><p>{{unmapped}}</p></poml>").unwrap();
        ctx.settings.active_skill = Some("test".into());
        assert!(build_context(&db, &ctx, "task", &plugins, 0, root.path())
            .await
            .unwrap_err()
            .to_string()
            .contains("unmapped"));
        crate::db::tools::disable(&db, "use_skill").unwrap();
        assert!(build_context(&db, &ctx, "task", &plugins, 0, root.path())
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled"));
    }

    #[tokio::test]
    async fn backend_shared_context_contract_preserves_memory_and_input() {
        let root = fixture();
        let db = Database::new(&root.path().join("db")).unwrap();
        db.add_memory("alice", "only alice", Some("fact")).unwrap();
        db.add_memory("bob", "only bob", Some("fact")).unwrap();
        crate::db::memory::update_memory(&db, "alice", |m| {
            m.user_preferences.insert("theme".into(), json!("current"));
        })
        .unwrap();
        let mut ctx = Context {
            user_id: "alice".into(),
            custom_data: json!({"pref_theme":"stale"}),
            ..Default::default()
        };
        let plugins = PluginRegistry::new();
        route_context(root.path(), &mut ctx, "raw input", &plugins, None).unwrap();
        let value = build_context(&db, &ctx, "raw input", &plugins, 0, root.path())
            .await
            .unwrap();
        for key in ["user_prompt", "user_message"] {
            assert_eq!(value[key], "raw input");
        }
        assert_eq!(value["custom_data"]["user_prompt"], "raw input");
        assert_eq!(value["memory"]["facts"], json!(["only alice"]));
        assert_eq!(value["memory"]["preferences"]["theme"], "current");
        assert_eq!(value["settings"]["system_template"], "standard");
        for key in [
            "skills",
            "tools",
            "active_skill",
            "active_skill_instructions",
            "settings",
            "sm_data",
            "sm_file",
            "active_state",
            "tokens_used",
            "user_template",
            "conversation_text",
            "tag_instructions",
        ] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert!(value.get("cl_file").is_none());
    }
}
