use crate::cl;
use crate::gateway::llm::provider::{ChatMessage, ChatRequest, ToolCall};
use crate::gateway::GatewayState;
use crate::tags::{self, TagExecution};

#[derive(Debug)]
pub struct AgentLoopResult {
    pub response: String,
    pub turns_used: i32,
    pub tag_execution: Option<TagExecution>,
    pub completed: bool,
    pub advanced: bool,
}

pub struct AgentLoopConfig {
    pub max_turns: i32,
    pub max_tool_calls: i32,
    pub tags_enabled: bool,
    pub cl_file: Option<String>,
    pub feedback_enabled: bool,
    pub message_on_toolcalling: bool,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            max_tool_calls: 5,
            tags_enabled: true,
            cl_file: None,
            feedback_enabled: false,
            message_on_toolcalling: false,
        }
    }
}

pub async fn run_agent_loop(
    state: &GatewayState,
    user_id: &str,
    user_message: &str,
    config: AgentLoopConfig,
    feedback_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> anyhow::Result<AgentLoopResult> {
    let mut turn = 0;
    let mut completed = false;
    let mut advanced = false;
    let mut last_tag_execution = None;

    // Load context
    let mut ctx = state.db.load_context(user_id)?;

    // Merge plugin context defaults into custom_data
    let plugin_defaults = state.plugins.context_defaults();
    if !plugin_defaults.is_empty() {
        if ctx.custom_data.is_null() {
            ctx.custom_data = serde_json::json!({});
        }
        if let Some(obj) = ctx.custom_data.as_object_mut() {
            for (key, value) in &plugin_defaults {
                obj.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }

    // Inject computed variables into custom_data
    {
        if ctx.custom_data.is_null() {
            ctx.custom_data = serde_json::json!({});
        }
        if let Some(obj) = ctx.custom_data.as_object_mut() {
            obj.insert("user_prompt".to_string(), serde_json::json!(user_message));

            let effective_path = if ctx.settings.path.is_empty() {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "/".to_string())
            } else {
                ctx.settings.path.clone()
            };
            obj.insert("path".to_string(), serde_json::json!(effective_path));
            obj.insert("time".to_string(), serde_json::json!(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()));

            let user_template = obj.get("user_template").cloned()
                .unwrap_or_else(|| serde_json::json!("user"));
            obj.insert("user_template".to_string(), user_template);

            let memory = crate::db::memory::load_memory(&state.db, user_id);
            obj.insert("memory".to_string(), serde_json::json!({
                "facts": memory.learned_facts,
                "topics": memory.last_topics,
                "preferences": memory.user_preferences,
                "variables": memory.custom_variables,
            }));
        }
    }

    // Apply CL workflow if configured
    if let Some(ref cl_path) = config.cl_file {
        if let Ok(cl) = cl::load_file(cl_path) {
            let mut ctx_val = serde_json::to_value(&ctx)?;
            let _secret_changes = cl::apply_to_context(&cl, &mut ctx_val);
            // Save updated context
            if let Ok(updated_ctx) = serde_json::from_value::<crate::db::contexts::Context>(ctx_val)
            {
                ctx = updated_ctx;
                let _ = state.db.save_context(&ctx);
            }
        }
    }

    // Apply user template
    let user_template_name = ctx.custom_data
        .get("user_template")
        .and_then(|v| v.as_str())
        .unwrap_or("user");
    let user_template_path = format!("templates/{}.poml", user_template_name);
    let rendered_user_message = if std::path::Path::new(&user_template_path).exists() {
        let tmpl_ctx = serde_json::json!({ "user_prompt": user_message });
        crate::gateway::poml::render(&user_template_path, &tmpl_ctx).await
            .unwrap_or_else(|_| user_message.to_string())
    } else {
        user_message.to_string()
    };

    // Store user message
    state.db.add_message(
        user_id,
        &crate::db::messages::Message::user(rendered_user_message),
    )?;

    loop {
        if turn >= config.max_turns {
            tracing::warn!(user_id = %user_id, turn = turn, "Max turns reached");
            break;
        }

        turn += 1;
        ctx.turn = turn;
        let _ = state.db.save_context(&ctx);

        // Build messages
        let system_prompt = build_system_prompt(state, &ctx, user_message).await;
        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
            tool_call_id: None,
        }];

        // Add compaction summary if available
        if ctx.settings.compaction_enabled && !ctx.settings.compaction_summary.is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Some(format!(
                    "Previous conversation summary: {}",
                    ctx.settings.compaction_summary
                )),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let history = state.db.get_messages(user_id, 50)?;
        for msg in &history {
            // Skip tool-call messages if setting is disabled
            if !ctx.settings.history_with_toolcalls
                && (msg.role == "tool" || msg.tool_calls.is_some())
            {
                continue;
            }
            let tool_calls = msg.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| ToolCall {
                        id: tc.id.clone(),
                        function: crate::gateway::llm::provider::FunctionCall {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        },
                    })
                    .collect()
            });
            messages.push(ChatMessage {
                role: msg.role.clone(),
                content: if msg.content.is_empty() {
                    None
                } else {
                    Some(msg.content.clone())
                },
                tool_calls,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }

        // Get tool definitions from database + plugins
        let mut tools = crate::db::tools::to_tool_definitions(&state.db).unwrap_or_default();
        tools.extend(state.plugins.tool_definitions());

        let request = ChatRequest {
            messages,
            tools: if tools.is_empty() { None } else { Some(tools) },
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };

        // Call LLM
        let response = match state.llm.chat(request, None).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "LLM call failed");
                if let Some(ref tx) = feedback_tx {
                    let _ = tx.send(format!("LLM error: {}", e));
                }
                return Err(e);
            }
        };

        // Handle tool calls
        if let Some(tool_calls) = &response.tool_calls {
            // Persist the assistant message with tool_calls
            let db_tool_calls: Vec<crate::db::messages::ToolCallData> = tool_calls
                .iter()
                .map(|tc| crate::db::messages::ToolCallData {
                    id: tc.id.clone(),
                    function: crate::db::messages::FunctionCallData {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    },
                })
                .collect();
            state.db.add_message(
                user_id,
                &crate::db::messages::Message::assistant_with_tool_calls(
                    response.content.clone().unwrap_or_default(),
                    db_tool_calls,
                ),
            )?;

            if let Some(ref content) = response.content {
                if !content.trim().is_empty() {
                    let send_first = ctx.custom_data
                        .get("send_first_response")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    if send_first {
                        if let Some(ref tx) = feedback_tx {
                            let _ = tx.send(content.clone());
                        }
                    }
                }
            }

            let mut tool_call_count = 0;
            for tc in tool_calls {
                if tool_call_count >= config.max_tool_calls {
                    tracing::warn!(user_id = %user_id, "Max tool calls reached");
                    break;
                }

                if let Some(ref tx) = feedback_tx {
                    if config.feedback_enabled && config.message_on_toolcalling {
                        let _ = tx.send(format!("Calling tool: {}", tc.function.name));
                    }
                }

                let result = execute_tool_call(&state.db, user_id, tc, &state.plugins).await;
                state.db.add_message(
                    user_id,
                    &crate::db::messages::Message::tool(result.clone(), tc.id.clone()),
                )?;
                tool_call_count += 1;
            }
            // Continue loop for next LLM turn
            // Reload context to check if agent_complete set done=true
            ctx = state.db.load_context(user_id)?;
            if ctx.settings.done {
                completed = true;
            }
            continue;
        }

        // No tool calls — process response
        let raw_response = response.content.unwrap_or_default();

        // Strip think tags
        let response_text = crate::gateway::poml::strip_think_tags(&raw_response);

        // Extract agent signals first
        let (agent_signals, response_text) =
            crate::gateway::poml::extract_agent_signals(&response_text);

        // Process agent signals
        for signal in &agent_signals {
            match signal {
                crate::gateway::poml::AgentSignal::Complete => {
                    completed = true;
                }
                crate::gateway::poml::AgentSignal::Next => {
                    advanced = true;
                }
                crate::gateway::poml::AgentSignal::Feedback(msg) => {
                    if let Some(ref tx) = feedback_tx {
                        let _ = tx.send(msg.clone());
                    }
                }
                crate::gateway::poml::AgentSignal::Push(template) => {
                    ctx.settings.active_templates.push(template.clone());
                }
                crate::gateway::poml::AgentSignal::Pop => {
                    ctx.settings.active_templates.pop();
                }
                crate::gateway::poml::AgentSignal::Path(path) => {
                    ctx.settings.path = path.clone();
                }
                crate::gateway::poml::AgentSignal::Set(key, value) => {
                    if ctx.custom_data.is_null() {
                        ctx.custom_data = serde_json::json!({});
                    }
                    if let Some(map) = ctx.custom_data.as_object_mut() {
                        map.insert(key.clone(), serde_json::json!(value));
                    }
                }
                _ => {}
            }
        }

        // Parse and execute tags if enabled
        if config.tags_enabled {
            let tag_result = tags::parse_tags(&response_text);
            let tag_exec = tags::execute_tags(&tag_result, &mut ctx);

            // Send feedback messages
            for msg in &tag_exec.feedback_messages {
                if let Some(ref tx) = feedback_tx {
                    let _ = tx.send(msg.clone());
                }
            }

            if tag_exec.should_complete {
                completed = true;
            }
            if tag_exec.should_advance {
                advanced = true;
            }

            // Save learned data
            if !tag_exec.learned_facts.is_empty()
                || !tag_exec.learned_preferences.is_empty()
                || !tag_exec.learned_topics.is_empty()
            {
                let mut memory = crate::db::memory::load_memory(&state.db, user_id);
                for fact in &tag_exec.learned_facts {
                    crate::db::memory::add_learned_fact(&mut memory, fact);
                }
                for (key, val) in &tag_exec.learned_preferences {
                    crate::db::memory::update_preference(&mut memory, key, val);
                }
                for topic in &tag_exec.learned_topics {
                    crate::db::memory::add_topic(&mut memory, topic);
                }
                let _ = crate::db::memory::save_memory(&state.db, user_id, &memory);
            }

            last_tag_execution = Some(tag_exec);

            // Store cleaned response
            state.db.add_message(
                user_id,
                &crate::db::messages::Message::assistant(tag_result.cleaned_response.clone()),
            )?;
        } else {
            // Store raw response
            state.db.add_message(
                user_id,
                &crate::db::messages::Message::assistant(response_text.clone()),
            )?;
        }

        // Advance CL state if needed
        if advanced {
            if let Some(ref cl_path) = config.cl_file {
                if let Ok(cl) = cl::load_file(cl_path) {
                    let ctx_val = serde_json::to_value(&ctx)?;
                    if let Some(new_state) = cl::advance_state(&cl, &ctx_val) {
                        ctx.settings.active_state = Some(new_state.clone());
                        tracing::info!(user_id = %user_id, new_state = %new_state, "CL state advanced");
                    }
                }
            }
            advanced = false;
        }

        let _ = state.db.save_context(&ctx);

        // Auto-compact if messages exceed threshold
        if ctx.settings.compaction_enabled {
            let message_count = state.db.get_messages(user_id, 200)?.len();
            if message_count > 50 {
                tracing::info!(user_id = %user_id, count = message_count, "Auto-compacting conversation");
                if let Ok(summary) = generate_compaction_summary(state, user_id).await {
                    ctx.settings.compaction_summary = summary;
                    let _ = state.db.save_context(&ctx);
                }
            }
        }

        if completed {
            break;
        }

        // If no tool calls and not completed, we're done with this message
        break;
    }

    let final_response = state
        .db
        .get_messages(user_id, 1)?
        .first()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    Ok(AgentLoopResult {
        response: final_response,
        turns_used: turn,
        tag_execution: last_tag_execution,
        completed,
        advanced,
    })
}

/// Generate a compaction summary of the conversation using the LLM.
/// This condenses the conversation into a short summary for context preservation.
async fn generate_compaction_summary(
    state: &GatewayState,
    user_id: &str,
) -> anyhow::Result<String> {
    let messages = state.db.get_messages(user_id, 100)?;

    let conversation_text = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Summarize this conversation in 3-5 sentences. Reply with only the summary:\n\n{}",
        conversation_text
    );

    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some(prompt),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        temperature: Some(0.3),
        max_tokens: Some(500),
    };

    let response = state.llm.chat(request, None).await?;
    let summary = response
        .content
        .unwrap_or_else(|| "Summary not available.".to_string());

    tracing::info!(user_id = %user_id, summary_len = summary.len(), "Compaction summary generated");
    Ok(summary)
}

async fn build_system_prompt(
    _state: &GatewayState,
    ctx: &crate::db::contexts::Context,
    user_message: &str,
) -> String {
    let mut context_json = serde_json::json!({
        "user_id": ctx.user_id,
        "turn": ctx.turn,
        "mode": ctx.mode,
        "system_info": format!("Praxis v{}", env!("CARGO_PKG_VERSION")),
        "user_message": user_message,
        "user_prompt": user_message,
        "time": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });

    if let Some(ref name) = ctx.user_name {
        context_json["user_name"] = serde_json::json!(name);
    }

    let effective_path = if ctx.settings.path.is_empty() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string())
    } else {
        ctx.settings.path.clone()
    };
    context_json["path"] = serde_json::json!(effective_path);

    if let Some(ref state) = ctx.settings.active_state {
        context_json["active_state"] = serde_json::json!(state);
    }

    // Add tag instructions if enabled
    if ctx.settings.tags_enabled {
        context_json["tag_instructions"] = serde_json::json!(crate::tags::get_tag_instructions());
    }

    // Load skills for context
    let mut skills_registry = crate::skills::SkillRegistry::new();
    let _ = skills_registry.load_from_dir(std::path::Path::new("skills"));
    context_json["skills"] = skills_registry.to_context_array();

    // Load memory
    let memory = crate::db::memory::load_memory(&_state.db, &ctx.user_id);
    context_json["memory"] = serde_json::json!({
        "facts": memory.learned_facts,
        "topics": memory.last_topics,
        "preferences": memory.user_preferences,
        "variables": memory.custom_variables,
    });

    let template_path = "templates/system.poml";
    match crate::gateway::poml::render(template_path, &context_json).await {
        Ok(rendered) => rendered,
        Err(e) => {
            tracing::warn!("Failed to render POML: {}, using fallback", e);
            format!(
                "You are Praxis, an AI agent. Mode: {}. User: {}. Turn: {}.",
                ctx.mode,
                ctx.user_name.as_deref().unwrap_or("unknown"),
                ctx.turn
            )
        }
    }
}

async fn execute_tool_call(db: &crate::db::Database, user_id: &str, tc: &ToolCall, plugins: &crate::plugins::PluginRegistry) -> String {
    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing arguments: {}", e),
    };

    let ctx_data = db.load_context(user_id)
        .ok()
        .map(|ctx| ctx.custom_data)
        .filter(|v| !v.is_null());

    let plugin_secret_keys = plugins.collect_secrets();
    let all_secrets = crate::db::secrets::get_secrets();
    tracing::info!(
        plugin_secret_keys = ?plugin_secret_keys,
        custom_keys = ?all_secrets.custom.keys().collect::<Vec<_>>(),
        custom_values = ?all_secrets.custom,
        "Secrets before plugin filtering"
    );
    let plugin_secrets: std::collections::HashMap<String, String> = plugin_secret_keys
        .iter()
        .filter_map(|k| all_secrets.custom.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    tracing::info!(plugin_secrets = ?plugin_secrets, "Plugin secrets after filtering");

    match tc.function.name.as_str() {
        "execute_terminal" => {
            let command = args["command"].as_str().unwrap_or("");
            match crate::tools::execute_terminal::execute_terminal(command, None).await {
                Ok(result) => {
                    if result.exit_code == 0 {
                        if result.stdout.is_empty() {
                            "Command executed successfully (no output)".to_string()
                        } else {
                            result.stdout
                        }
                    } else {
                        format!(
                            "Exit code: {}\nStdout: {}\nStderr: {}",
                            result.exit_code, result.stdout, result.stderr
                        )
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            match crate::tools::write_file::write_file(path, content).await {
                Ok(_) => format!("File written: {}", path),
                Err(e) => format!("Error: {}", e),
            }
        }
        "edit_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let old_text = args["old_text"]
                .as_str()
                .or_else(|| args["old_string"].as_str())
                .unwrap_or("");
            let new_text = args["new_text"]
                .as_str()
                .or_else(|| args["new_string"].as_str())
                .unwrap_or("");
            match crate::tools::edit_file::edit_file(path, old_text, new_text).await {
                Ok(_) => format!("File edited: {}", path),
                Err(e) => format!("Error: {}", e),
            }
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if content.len() > 10000 {
                        format!(
                            "{}...\n\n[File truncated - {} bytes total]",
                            &content[..10000],
                            content.len()
                        )
                    } else {
                        content
                    }
                }
                Err(e) => format!("Error reading file: {}", e),
            }
        }
        "web_search" => {
            let query = args["query"].as_str().unwrap_or("");
            match crate::tools::web_search::web_search(query, 5).await {
                Ok(results) => {
                    if results.is_empty() {
                        "No results found".to_string()
                    } else {
                        let mut output = String::new();
                        for (i, r) in results.iter().enumerate() {
                            output.push_str(&format!(
                                "{}. {}\n   {}\n   {}\n\n",
                                i + 1,
                                r.title,
                                r.snippet,
                                r.url
                            ));
                        }
                        output
                    }
                }
                Err(e) => format!("Search error: {}", e),
            }
        }
        "get_context" => match db.load_context(user_id) {
            Ok(ctx) => serde_json::to_string_pretty(&ctx)
                .unwrap_or_else(|_| "Failed to serialize".to_string()),
            Err(e) => format!("Error: {}", e),
        },
        "set_context" => {
            let key = args["key"].as_str().unwrap_or("");
            let value = args
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match db.merge_context(user_id, serde_json::json!({key: value})) {
                Ok(_) => format!("Context key '{}' set", key),
                Err(e) => format!("Error: {}", e),
            }
        }
        "delete_context" => {
            let key = args["key"].as_str().unwrap_or("");
            match db.merge_context(user_id, serde_json::json!({key: null})) {
                Ok(_) => format!("Context key '{}' deleted", key),
                Err(e) => format!("Error: {}", e),
            }
        }
        "agent_next" => crate::tools::agent_control::run(
            db,
            user_id,
            crate::tools::agent_control::AgentControlSignal::Next,
        )
        .await
        .unwrap_or_else(|e| format!("Error: {}", e)),
        "agent_complete" => crate::tools::agent_control::run(
            db,
            user_id,
            crate::tools::agent_control::AgentControlSignal::Complete,
        )
        .await
        .unwrap_or_else(|e| format!("Error: {}", e)),
        "agent_set_path" => {
            let path = args["path"].as_str().unwrap_or("");
            crate::tools::agent_control::run(
                db,
                user_id,
                crate::tools::agent_control::AgentControlSignal::Path(path.to_string()),
            )
            .await
            .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "agent_feedback" => {
            let message = args["message"].as_str().unwrap_or("");
            crate::tools::agent_control::run(
                db,
                user_id,
                crate::tools::agent_control::AgentControlSignal::Feedback(message.to_string()),
            )
            .await
            .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "discord_upload_file" => {
            let filename = args["filename"].as_str().unwrap_or("");
            let base64_content = args["base64_content"].as_str().unwrap_or("");
            match crate::tools::discord_upload::upload_file(
                user_id,
                filename,
                base64_content,
                Some(""),
            )
            .await
            {
                Ok(_) => format!("File '{}' uploaded", filename),
                Err(e) => format!("Error: {}", e),
            }
        }
        "discord_send_message" => {
            let channel_id = args["channel_id"].as_str().unwrap_or("");
            let message = args["message"].as_str().unwrap_or("");
            match crate::tools::discord_send_message::send_message(channel_id, message).await {
                Ok(_) => "Message sent".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "discord_send_embed" => {
            let channel_id = args["channel_id"].as_str().unwrap_or("");
            let title = args.get("title").and_then(|v| v.as_str());
            let description = args.get("description").and_then(|v| v.as_str());
            let url = args.get("url").and_then(|v| v.as_str());
            let color = args
                .get("color")
                .and_then(|v| crate::tools::discord_send_embed::parse_color(v));
            let footer = args.get("footer").and_then(|v| v.as_str());
            let author = args.get("author").and_then(|v| v.as_str());
            let thumbnail = args.get("thumbnail").and_then(|v| v.as_str());
            let image = args.get("image").and_then(|v| v.as_str());
            let fields = args
                .get("fields")
                .map(|v| crate::tools::discord_send_embed::parse_fields(v))
                .unwrap_or_default();
            match crate::tools::discord_send_embed::send_embed(
                user_id,
                channel_id,
                title,
                description,
                url,
                color,
                footer,
                author,
                thumbnail,
                image,
                fields,
            )
            .await
            {
                Ok(_) => "Embed sent".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "learn_fact" => {
            let fact = args["fact"].as_str().unwrap_or("");
            match db.add_memory(user_id, fact, Some("fact")) {
                Ok(_) => format!("Learned: {}", fact),
                Err(e) => format!("Error: {}", e),
            }
        }
        "learn_preference" => {
            let key = args["key"].as_str().unwrap_or("");
            let value = args["value"].as_str().unwrap_or("");
            match db.merge_context(
                user_id,
                serde_json::json!({"custom_data": {format!("pref_{}", key): value}}),
            ) {
                Ok(_) => format!("Preference '{}' = '{}'", key, value),
                Err(e) => format!("Error: {}", e),
            }
        }
        "learn_topic" => {
            let topic = args["topic"].as_str().unwrap_or("");
            match db.add_memory(user_id, topic, Some("topic")) {
                Ok(_) => format!("Topic tracked: {}", topic),
                Err(e) => format!("Error: {}", e),
            }
        }
        _ => match plugins.execute_tool(&tc.function.name, &args, ctx_data.as_ref(), Some(&plugin_secrets)).await {
            Ok(result) => result,
            Err(e) => format!("Unknown tool: {} ({})", tc.function.name, e),
        },
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;

    #[test]
    fn test_agent_loop_config_default() {
        let config = AgentLoopConfig::default();
        assert_eq!(config.max_turns, 10);
        assert_eq!(config.max_tool_calls, 5);
        assert!(config.tags_enabled);
        assert!(config.cl_file.is_none());
        assert!(!config.feedback_enabled);
    }

    #[test]
    fn test_get_tool_definitions() {
        // Tool definitions now come from the database
        // This test verifies the structure is compatible
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::Database::new(dir.path()).unwrap();
        crate::db::tools::init_default_tools(&db).unwrap();
        let tools = crate::db::tools::to_tool_definitions(&db).unwrap();
        assert!(!tools.is_empty());
        let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"execute_terminal"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"web_search"));
    }

    #[test]
    fn test_agent_loop_result_debug() {
        let result = AgentLoopResult {
            response: "test".to_string(),
            turns_used: 1,
            tag_execution: None,
            completed: false,
            advanced: false,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("test"));
        assert!(debug.contains("turns_used: 1"));
    }
}
