use crate::gateway::llm::provider::{ChatMessage, ChatRequest};
use crate::gateway::GatewayState;
use crate::voice::tts;

pub async fn handle_message(
    state: &GatewayState,
    user_id: &str,
    content: &str,
    channel_id: Option<&str>,
) -> anyhow::Result<String> {
    let ctx = state.db.load_context(user_id)?;
    let max_turns = ctx.settings.max_llm_turns.unwrap_or(1);

    // Use agent loop when max_turns > 1
    if max_turns > 1 {
        return handle_message_agent_loop(state, user_id, content, channel_id, &ctx, max_turns)
            .await;
    }

    // Legacy single-pass path (max_turns == 1)
    let mut ctx = ctx;
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
            obj.insert("user_prompt".to_string(), serde_json::json!(content));

            let effective_path = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/".to_string());
            obj.insert("path".to_string(), serde_json::json!(effective_path));
            obj.insert(
                "time".to_string(),
                serde_json::json!(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
            );

            let user_template = obj
                .get("user_template")
                .cloned()
                .unwrap_or_else(|| serde_json::json!("user"));
            obj.insert("user_template".to_string(), user_template);

            // Default channel_id for Discord tools (can be overwritten via CL)
            // Prefer channel_id from the incoming message, fall back to settings
            if !obj.contains_key("channel_id") {
                let ch = channel_id
                    .filter(|s| !s.is_empty())
                    .or_else(|| ctx.settings.feedback_channel_id.as_deref())
                    .unwrap_or("");
                if !ch.is_empty() {
                    obj.insert("channel_id".to_string(), serde_json::json!(ch));
                }
            }

            let memory = crate::db::memory::load_memory(&state.db, user_id);
            obj.insert(
                "memory".to_string(),
                serde_json::json!({
                    "facts": memory.learned_facts,
                    "topics": memory.last_topics,
                    "preferences": memory.user_preferences,
                    "variables": memory.custom_variables,
                }),
            );

            let token_budget = ctx.settings.history_token_limit.unwrap_or(500000);
            let compaction_limit = ctx.settings.compaction_token_limit.unwrap_or(500000);
            let (_all_msgs, tokens_used) = state
                .db
                .get_messages_with_token_budget(user_id, usize::MAX)
                .unwrap_or((vec![], 0));
            let message_count = state.db.count_messages(user_id).unwrap_or(0);
            let tokens_pct = if token_budget > 0 {
                (tokens_used as f64 / token_budget as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let compaction_pct = if compaction_limit > 0 {
                (tokens_used as f64 / compaction_limit as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            obj.insert("tokens_used".to_string(), serde_json::json!(tokens_used));
            obj.insert("tokens_limit".to_string(), serde_json::json!(token_budget));
            obj.insert(
                "tokens_percentage".to_string(),
                serde_json::json!(format!("{:.1}", tokens_pct)),
            );
            obj.insert(
                "compaction_token_limit".to_string(),
                serde_json::json!(compaction_limit),
            );
            obj.insert(
                "compaction_percentage".to_string(),
                serde_json::json!(format!("{:.1}", compaction_pct)),
            );
            obj.insert(
                "message_count".to_string(),
                serde_json::json!(message_count),
            );
        }
    }

    let _ = state.db.save_context(&ctx);

    state.db.add_message(
        user_id,
        &crate::db::messages::Message::user(content.to_string()),
    )?;

    let system_prompt = build_system_prompt(state, &ctx).await;

    let mut messages = Vec::new();
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(system_prompt),
        content_parts: None,
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
    });

    let token_budget = ctx.settings.history_token_limit.unwrap_or(500000);
    let (history, _tokens) = state
        .db
        .get_messages_with_token_budget(user_id, token_budget)?;
    for msg in &history {
        let tool_calls = msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter()
                .map(|tc| crate::gateway::llm::provider::ToolCall {
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
            content_parts: None,
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
            tool_name: msg.tool_name.clone(),
        });
    }

    let mut tool_defs = crate::db::tools::to_tool_definitions(&state.db).unwrap_or_default();
    tool_defs.extend(state.plugins.tool_definitions());

    // Clone tool names and definitions for validation before moving tool_defs into request
    let tool_names: Vec<String> = tool_defs.iter().map(|t| t.function.name.clone()).collect();
    let tools_for_validation = tool_defs.clone();

    let request = ChatRequest {
        messages,
        tools: if tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs)
        },
        temperature: Some(0.7),
        max_tokens: Some(4096),
        model: ctx.settings.model.clone(),
        vision_provider: ctx.settings.vision_provider.clone().or_else(|| state.config.vision_provider.clone()),
        vision_model: ctx.settings.vision_model.clone().or_else(|| state.config.vision_model.clone()),
    };

    let response = state
        .llm
        .chat(request, ctx.settings.provider.as_deref())
        .await?;

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

        let mut results = Vec::new();
        for tc in tool_calls {
            // Validate tool exists before executing
            if !tool_names.contains(&tc.function.name) {
                let result = format!("Error: Unknown tool '{}'. Check the tool name and try again.", tc.function.name);
                tracing::warn!(user_id = %user_id, tool = %tc.function.name, "Unknown tool called");
                
                // Add error as tool result so LLM can recover
                let msg = crate::db::messages::Message::tool(result.clone(), tc.id.clone());
                let mut msg = msg;
                msg.tool_name = Some(tc.function.name.clone());
                state.db.add_message(user_id, &msg)?;
                results.push((tc.id.clone(), result));
                continue;
            }

            // Validate required parameters
            let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
            if let Err(e) = crate::gateway::agent_loop::validate_tool_params(&tc.function.name, &args, &tools_for_validation) {
                let result = format!("Error: {}", e);
                tracing::warn!(user_id = %user_id, tool = %tc.function.name, error = %e, "Invalid tool parameters");
                
                let msg = crate::db::messages::Message::tool(result.clone(), tc.id.clone());
                let mut msg = msg;
                msg.tool_name = Some(tc.function.name.clone());
                state.db.add_message(user_id, &msg)?;
                results.push((tc.id.clone(), result));
                continue;
            }

            let result = execute_tool_call(&state.db, user_id, tc, &state.plugins).await;
            let mut image_content_parts: Option<Vec<serde_json::Value>> = None;
            let mut final_result = result.clone();

            // Check if understand_image returned image data
            if tc.function.name == "understand_image" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result) {
                    if let Some(parts) = parsed.get("content_parts").and_then(|v| v.as_array()) {
                        if !parts.is_empty() {
                            final_result = parsed
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&result)
                                .to_string();
                            image_content_parts = Some(parts.clone());
                        }
                    }
                }
            }

            let msg = if let Some(parts) = image_content_parts {
                crate::db::messages::Message::tool_with_image(
                    final_result.clone(),
                    tc.id.clone(),
                    parts,
                )
            } else {
                crate::db::messages::Message::tool(final_result.clone(), tc.id.clone())
            };
            // Set tool_name for Ollama compatibility
            let mut msg = msg;
            msg.tool_name = Some(tc.function.name.clone());
            state.db.add_message(user_id, &msg)?;
            results.push((tc.id.clone(), final_result));
        }

        let mut followup_messages = Vec::new();
        followup_messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(build_system_prompt(state, &ctx).await),
            content_parts: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });
        let (history, _tokens) = state
            .db
            .get_messages_with_token_budget(user_id, token_budget)?;

        // Only include image data for the last 2 messages with content_parts
        let mut image_msg_indices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        let mut found = 0;
        for (i, msg) in history.iter().enumerate().rev() {
            if msg.content_parts.as_ref().map_or(false, |p| !p.is_empty()) {
                image_msg_indices.insert(i);
                found += 1;
                if found >= 2 {
                    break;
                }
            }
        }

        for (msg_idx, msg) in history.iter().enumerate() {
            let tool_calls = msg.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| crate::gateway::llm::provider::ToolCall {
                        id: tc.id.clone(),
                        function: crate::gateway::llm::provider::FunctionCall {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        },
                    })
                    .collect()
            });
            followup_messages.push(ChatMessage {
                role: msg.role.clone(),
                content: if msg.content.is_empty() {
                    None
                } else {
                    Some(msg.content.clone())
                },
                content_parts: if image_msg_indices.contains(&msg_idx) {
                    msg.content_parts.as_ref().map(|parts| {
                        parts
                            .iter()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect()
                    })
                } else {
                    None
                },
                tool_calls,
                tool_call_id: msg.tool_call_id.clone(),
                tool_name: msg.tool_name.clone(),
            });
        }

        let followup_request = ChatRequest {
            messages: followup_messages,
            tools: None,
            temperature: Some(0.7),
            max_tokens: Some(4096),
            model: ctx.settings.model.clone(),
            vision_provider: ctx.settings.vision_provider.clone().or_else(|| state.config.vision_provider.clone()),
            vision_model: ctx.settings.vision_model.clone().or_else(|| state.config.vision_model.clone()),
        };

        let followup_response = state
            .llm
            .chat(followup_request, ctx.settings.provider.as_deref())
            .await?;
        let reply = followup_response.content.unwrap_or_default();

        state.db.add_message(
            user_id,
            &crate::db::messages::Message::assistant(reply.clone()),
        )?;

        let mut updated_ctx = ctx;
        state.db.increment_turn(&mut updated_ctx);
        state.db.save_context(&updated_ctx)?;

        if updated_ctx.settings.use_tts {
            spawn_tts(
                reply.clone(),
                &updated_ctx.settings,
                &state.secrets,
                user_id,
            );
        }

        return Ok(reply);
    }

    let reply = response.content.unwrap_or_default();

    state.db.add_message(
        user_id,
        &crate::db::messages::Message::assistant(reply.clone()),
    )?;

    let mut updated_ctx = ctx;
    state.db.increment_turn(&mut updated_ctx);
    state.db.save_context(&updated_ctx)?;

    if updated_ctx.settings.use_tts {
        spawn_tts(
            reply.clone(),
            &updated_ctx.settings,
            &state.secrets,
            user_id,
        );
    }

    Ok(reply)
}

async fn handle_message_agent_loop(
    state: &GatewayState,
    user_id: &str,
    content: &str,
    channel_id: Option<&str>,
    ctx: &crate::db::contexts::Context,
    max_turns: i32,
) -> anyhow::Result<String> {
    let feedback_modes = &ctx.settings.feedback_mode;
    let feedback_channel = ctx
        .settings
        .feedback_channel_id
        .clone()
        .or_else(|| channel_id.map(|s| s.to_string()));
    let user_id_owned = user_id.to_string();
    let secrets = state.secrets.clone();
    let settings = ctx.settings.clone();

    // Inject channel_id into custom_data so tools can use it
    let mut ctx = ctx.clone();
    if ctx.custom_data.is_null() {
        ctx.custom_data = serde_json::json!({});
    }
    if let Some(obj) = ctx.custom_data.as_object_mut() {
        if !obj.contains_key("channel_id") {
            let ch = channel_id
                .filter(|s| !s.is_empty())
                .or_else(|| ctx.settings.feedback_channel_id.as_deref())
                .unwrap_or("");
            if !ch.is_empty() {
                obj.insert("channel_id".to_string(), serde_json::json!(ch));
            }
        }
    }
    let _ = state.db.save_context(&ctx);

    let config = crate::gateway::agent_loop::AgentLoopConfig {
        max_turns,
        max_tool_calls: ctx.settings.max_tool_calls.unwrap_or(5),
        tags_enabled: ctx.settings.tags_enabled,
        cl_file: ctx.settings.cl_file.clone().or(ctx.cl_file.clone()),
        feedback_enabled: !feedback_modes.is_empty(),
        message_on_toolcalling: ctx.settings.message_on_toolcalling,
    };

    let (feedback_tx, mut feedback_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let use_tts = ctx.settings.use_tts;
    let is_voice_input = channel_id.map_or(false, |ch| ch.starts_with("voice:"));
    let tts_for_feedback = use_tts || is_voice_input;

    // Spawn feedback routing task
    if !feedback_modes.is_empty() || tts_for_feedback {
        let feedback_modes = feedback_modes.clone();
        let uid = user_id_owned.clone();
        let ch = feedback_channel.clone();
        let tts_settings = settings.clone();
        let tts_secrets = secrets.clone();

        tokio::spawn(async move {
            while let Some(msg) = feedback_rx.recv().await {
                let mut handled = false;
                for mode in &feedback_modes {
                    match mode.as_str() {
                        "tts" => {
                            spawn_tts(msg.clone(), &tts_settings, &tts_secrets, &uid);
                            handled = true;
                        }
                        "dm" => {
                            crate::event_channel::broadcast_agent_feedback(&uid, &msg);
                            handled = true;
                        }
                        "text" => {
                            let sent_to_channel = ch.as_ref().map_or(false, |ch_id| {
                                if ch_id.parse::<u64>().ok().filter(|&id| id > 0).is_some() {
                                    crate::event_channel::broadcast_channel_message(
                                        &uid, ch_id, &msg,
                                    );
                                    true
                                } else {
                                    false
                                }
                            });
                            if !sent_to_channel {
                                crate::event_channel::broadcast_agent_feedback(&uid, &msg);
                            }
                            handled = true;
                        }
                        _ => {}
                    }
                }
                if !handled && tts_for_feedback {
                    spawn_tts(msg.clone(), &tts_settings, &tts_secrets, &uid);
                }
            }
        });
    }

    let result = crate::gateway::agent_loop::run_agent_loop(
        state,
        user_id,
        content,
        config,
        Some(feedback_tx),
    )
    .await?;

    let reply = result.response.clone();

    let mut updated_ctx = state.db.load_context(user_id)?;
    state.db.increment_turn(&mut updated_ctx);
    state.db.save_context(&updated_ctx)?;

    // Final response TTS: always speak if use_tts is on, or if input came from voice
    let is_voice_input = channel_id.map_or(false, |ch| ch.starts_with("voice:"));
    if updated_ctx.settings.use_tts || is_voice_input {
        spawn_tts(
            reply.clone(),
            &updated_ctx.settings,
            &state.secrets,
            user_id,
        );
    }

    Ok(reply)
}

async fn build_system_prompt(state: &GatewayState, ctx: &crate::db::contexts::Context) -> String {
    let template_name = ctx.settings.system_template.as_deref().unwrap_or("system");
    let template_path = format!("templates/{}.poml", template_name);

    // Load skills for context
    let mut skills_registry = crate::skills::SkillRegistry::new();
    let _ = skills_registry.load_from_dir(std::path::Path::new("skills"));
    let skills = skills_registry.to_context_array();

    // Load memory
    let memory = crate::db::memory::load_memory(&state.db, &ctx.user_id);

    // Calculate uptime
    let uptime_secs = state.start_time.elapsed().as_secs();
    let uptime = format_uptime(uptime_secs);

    // Get paired users
    let paired_users = state.db.list_all_pairings().unwrap_or_default();
    let paired_count = paired_users.len();
    let paired_list: Vec<serde_json::Value> = paired_users
        .iter()
        .map(|p| {
            serde_json::json!({
                "user_id": p.user_id,
                "discord_user_id": p.discord_user_id,
                "paired_at": p.paired_at,
            })
        })
        .collect();

    let context = serde_json::json!({
        "user_name": ctx.user_name.as_deref().unwrap_or("User"),
        "mode": ctx.mode,
        "turn": ctx.turn,
        "system_info": format!("Praxis v{}", env!("CARGO_PKG_VERSION")),
        "skills": skills,
        "uptime": uptime,
        "uptime_secs": uptime_secs,
        "paired_users_count": paired_count,
        "paired_users": paired_list,
        "path": std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string()),
        "time": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "memory": serde_json::json!({
            "facts": memory.learned_facts,
            "topics": memory.last_topics,
            "preferences": memory.user_preferences,
            "variables": memory.custom_variables,
        }),
        "custom_data": if ctx.custom_data.is_null() {
            serde_json::json!({})
        } else {
            ctx.custom_data.clone()
        },
        "cl_data": if ctx.cl_data.is_null() {
            serde_json::json!({})
        } else {
            ctx.cl_data.clone()
        },
    });

    match crate::gateway::poml::render(&template_path, &context).await {
        Ok(rendered) => {
            tracing::debug!(target: "message_handler", "Rendered system prompt (first 2000 chars): {}", &rendered[..rendered.len().min(2000)]);
            rendered
        },
        Err(e) => {
            tracing::warn!("Failed to render POML template: {}, using fallback", e);
            format!(
                "You are Praxis, an AI agent assistant. Current mode: {}. User: {}. Turn: {}. Uptime: {}. Paired users: {}.",
                ctx.mode,
                ctx.user_name.as_deref().unwrap_or("unknown"),
                ctx.turn,
                uptime,
                paired_count,
            )
        }
    }
}

fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!(
            "{}d {}h {}m",
            secs / 86400,
            (secs % 86400) / 3600,
            (secs % 3600) / 60
        )
    }
}

async fn execute_tool_call(
    db: &crate::db::Database,
    user_id: &str,
    tc: &crate::gateway::llm::provider::ToolCall,
    plugins: &crate::plugins::PluginRegistry,
) -> String {
    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing arguments: {}", e),
    };

    let ctx_data = db
        .load_context(user_id)
        .ok()
        .map(|ctx| ctx.custom_data)
        .filter(|v| !v.is_null());

    let plugin_secret_keys = plugins.collect_secrets();
    let all_secrets = crate::db::secrets::get_secrets();
    let plugin_secrets: std::collections::HashMap<String, String> = plugin_secret_keys
        .iter()
        .filter_map(|k| all_secrets.custom.get(k).map(|v| (k.clone(), v.clone())))
        .collect();

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
        "get_context" => match db.load_context(user_id) {
            Ok(ctx) => serde_json::to_string_pretty(&ctx)
                .unwrap_or_else(|_| "Failed to serialize context".to_string()),
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
            let fallback_ch = ctx_data
                .as_ref()
                .and_then(|c| c.get("channel_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let channel_id = args["channel_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_ch);
            let filename = args["filename"].as_str().unwrap_or("file");
            let base64_content = args["base64_content"].as_str().unwrap_or("");
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(base64_content) {
                Ok(bytes) => {
                    let tmp_path = format!("/tmp/{}", filename);
                    if let Err(e) = std::fs::write(&tmp_path, &bytes) {
                        format!("Error writing temp file: {}", e)
                    } else {
                        match crate::tools::discord_upload::upload_file(
                            channel_id,
                            &tmp_path,
                            filename,
                            args.get("message").and_then(|v| v.as_str()),
                        )
                        .await
                        {
                            Ok(result) => {
                                let _ = std::fs::remove_file(&tmp_path);
                                result
                            }
                            Err(e) => {
                                let _ = std::fs::remove_file(&tmp_path);
                                format!("Error: {}", e)
                            }
                        }
                    }
                }
                Err(e) => format!("Error decoding base64: {}", e),
            }
        }
        "discord_send_message" => {
            let fallback_ch = ctx_data
                .as_ref()
                .and_then(|c| c.get("channel_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let channel_id = args["channel_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_ch);
            let message = args["message"].as_str().unwrap_or("");
            match crate::tools::discord_send_message::send_message(channel_id, message).await {
                Ok(_) => "Message sent".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "discord_send_embed" => {
            let fallback_ch = ctx_data
                .as_ref()
                .and_then(|c| c.get("channel_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let channel_id = args["channel_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_ch);
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
        "rag_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            match crate::tools::vector::vector_search(db, user_id, query, limit).await {
                Ok(results) => {
                    if results.is_empty() {
                        "No relevant documents found.".to_string()
                    } else {
                        let mut output = String::new();
                        for (i, chunk) in results.iter().enumerate() {
                            output.push_str(&format!(
                                "--- Result {} (similarity: {:.3}) ---\n{}\n\n",
                                i + 1,
                                chunk.similarity,
                                chunk.content
                            ));
                        }
                        output
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        "rag_ingest" => {
            let filename = args["filename"].as_str().unwrap_or("untitled");
            let content = args["content"].as_str().unwrap_or("");
            let file_type = args["file_type"].as_str().unwrap_or("txt");
            match crate::tools::vector::ingest_with_embeddings(db, user_id, filename, content, file_type).await {
                Ok(doc_id) => format!("Document ingested successfully. ID: {}", doc_id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "rag_list" => {
            match crate::tools::rag_ingest::list_documents(db, user_id).await {
                Ok(docs) => {
                    if docs.is_empty() {
                        "No documents in knowledge base.".to_string()
                    } else {
                        let mut output = String::from("Documents in knowledge base:\n");
                        for doc in &docs {
                            output.push_str(&format!(
                                "- {} ({}) - {} chunks - ID: {}\n",
                                doc["filename"], doc["file_type"], doc["chunk_count"], doc["id"]
                            ));
                        }
                        output
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        "rag_delete" => {
            let document_id = args["document_id"].as_str().unwrap_or("");
            match crate::tools::rag_ingest::delete_document(db, user_id, document_id).await {
                Ok(_) => format!("Document {} deleted.", document_id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "understand_image" => {
            let result = crate::tools::understand_image::run(&args).await;
            serde_json::json!({
                "text": result.text,
                "content_parts": result.content_parts.iter().map(|cp| {
                    serde_json::to_value(cp).unwrap_or_default()
                }).collect::<Vec<_>>()
            })
            .to_string()
        }
        "ask_questions" => {
            let fallback_ch = ctx_data
                .as_ref()
                .and_then(|c| c.get("channel_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let channel_id = args["channel_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_ch);
            if channel_id.is_empty() {
                return "Error: No channel_id provided and no originating channel found. Please specify a channel_id.".to_string();
            }
            // Get timeout from args, then context, then default to 120
            let default_timeout = ctx_data
                .as_ref()
                .and_then(|c| c.get("question_timeout_secs"))
                .and_then(|v| v.as_u64())
                .unwrap_or(120);
            let timeout = args["timeout_secs"].as_u64().unwrap_or(default_timeout);
            let paired_discord_user_id = db
                .get_pairing_by_internal_user(user_id)
                .ok()
                .flatten()
                .map(|p| p.discord_user_id)
                .unwrap_or_default();
            if paired_discord_user_id.is_empty() {
                return "Error: No Discord user pairing found. The user must be paired with a Discord account first.".to_string();
            }
            let questions_raw = args["questions"].as_array();
            let Some(arr) = questions_raw else {
                return "Error: 'questions' field is required and must be an array.".to_string();
            };
            if arr.is_empty() {
                return "Error: 'questions' array must contain at least one question.".to_string();
            }
            let mut questions: Vec<(String, String, Vec<String>)> = Vec::new();
            for (i, q) in arr.iter().enumerate() {
                // Check for invalid 'options' field
                if q.get("options").is_some() {
                    return format!("Error: Question {}: use 'suggestions' with plain strings, not 'options' with objects. Example: \"suggestions\": [\"yes\", \"no\", \"maybe\"]", i + 1);
                }
                let label = q["label"].as_str().filter(|s| !s.is_empty());
                let Some(label) = label else {
                    return format!("Error: Question {} is missing a non-empty 'label' field.", i + 1);
                };
                let text = q["question"].as_str().filter(|s| !s.is_empty());
                let Some(text) = text else {
                    return format!("Error: Question {} (label: '{}') is missing a non-empty 'question' field.", i + 1, label);
                };
                let suggestions: Vec<String> = q["suggestions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                questions.push((label.to_string(), text.to_string(), suggestions));
            }
            tracing::info!(timeout_secs = timeout, "ask_questions: waiting for responses");
            match crate::tools::discord_interactive::ask_questions(
                channel_id,
                &questions,
                timeout,
                &paired_discord_user_id,
            )
            .await
            {
                Ok(result) => result,
                Err(e) => format!("Error: {}", e),
            }
        }
        _ => match plugins
            .execute_tool(
                &tc.function.name,
                &args,
                ctx_data.as_ref(),
                Some(&plugin_secrets),
            )
            .await
        {
            Ok(result) => result,
            Err(e) => format!("Unknown tool: {} ({})", tc.function.name, e),
        },
    }
}

fn spawn_tts(
    text: String,
    settings: &crate::db::contexts::ContextSettings,
    secrets: &crate::db::secrets::Secrets,
    user_id: &str,
) {
    let tts_type = settings.voice_tts_type.clone();
    let rvc_on = settings.rvc_on;
    let rvc_server = settings.rvc_server.clone();
    let rvc_model_path = settings.rvc_model_path.clone();
    let rvc_index_path = settings.rvc_index_path.clone();
    let elevenlabs_api_key = secrets.elevenlabs_api_key.clone();
    let elevenlabs_voice_id = settings.voice_elevenlabs_voice_id.clone();
    let elevenlabs_tts_model = settings.elevenlabs_tts_model.clone();
    let elevenlabs_stability = settings.elevenlabs_stability;
    let elevenlabs_similarity_boost = settings.elevenlabs_similarity_boost;
    let elevenlabs_style = settings.elevenlabs_style;
    let elevenlabs_speed = settings.elevenlabs_speed;
    let elevenlabs_tts_language = settings.elevenlabs_tts_language.clone();
    let minimax_api_key = secrets.minimax_api_key.clone();
    let minimax_voice_id = settings.minimax_voice_id.clone();
    let minimax_model = settings
        .minimax_tts_model
        .clone()
        .unwrap_or_else(|| "speech-02-hd".to_string());
    let mimo_api_key = secrets.mimo_api_key.clone();
    let mimo_tts_type = settings
        .mimo_tts_type
        .clone()
        .unwrap_or_else(|| "builtin".to_string());
    let mimo_voice = settings.mimo_voice_id.clone();
    let qwen_tts_server = settings.qwen_tts_server.clone();
    let qwen_tts_model = settings.qwen_tts_model.clone();
    let qwen_tts_speaker = settings.qwen_tts_speaker.clone();
    let qwen_tts_language = settings.qwen_tts_language.clone();
    let qwen_voice_clone_audio_path = settings.qwen_voice_clone_audio_path.clone();
    let qwen_voice_clone_enabled = settings.qwen_voice_clone_enabled;
    let qwen_voice_clone_prompt = settings.qwen_voice_clone_prompt.clone();
    let audio_output_path = settings.voice_audio_output_path.clone();
    let user_id = user_id.to_string();

    tracing::trace!(user_id = %user_id, tts_type = %tts_type, "TTS: Spawning task");

    tokio::task::spawn(async move {
        let audio_bytes = match tts_type.as_str() {
            "windows_sapi" => {
                let tts_engine = tts::windows_sapi::WindowsSAPI::new();
                match tts_engine.speak_to_bytes(&text) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!("TTS FAILED: windows_sapi: {}", e);
                        return;
                    }
                }
            }
            "elevenlabs" => {
                let api_key = elevenlabs_api_key.unwrap_or_default();
                let voice_id = elevenlabs_voice_id.unwrap_or_default();
                if api_key.is_empty() || voice_id.is_empty() {
                    tracing::warn!("TTS FAILED: elevenlabs: API key or voice_id not set");
                    return;
                }
                let tts_engine = tts::elevenlabs::ElevenLabsTTS::new(api_key, voice_id);
                let voice_settings = tts::elevenlabs::ElevenLabsVoiceSettings {
                    stability: elevenlabs_stability,
                    similarity_boost: elevenlabs_similarity_boost,
                    style: elevenlabs_style,
                    speed: elevenlabs_speed,
                    language: elevenlabs_tts_language,
                };
                match tts_engine
                    .speak_with_settings(&text, &elevenlabs_tts_model, &voice_settings)
                    .await
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!("TTS FAILED: elevenlabs: {}", e);
                        return;
                    }
                }
            }
            "minimax" => {
                let api_key = minimax_api_key.unwrap_or_default();
                let voice_id = minimax_voice_id.unwrap_or_default();
                let tts_engine =
                    tts::minimax_tts::MiniMaxTTS::new(api_key, voice_id, minimax_model);
                match tts_engine.speak(&text).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!("TTS FAILED: minimax: {}", e);
                        return;
                    }
                }
            }
            "mimo_tts" => {
                let api_key = mimo_api_key.unwrap_or_default();
                let model = match mimo_tts_type.as_str() {
                    "voicedesign" => "mimo-v2.5-tts-voicedesign",
                    "voiceclone" => "mimo-v2.5-tts-voiceclone",
                    _ => "mimo-v2.5-tts",
                };
                let tts_client = tts::mimo_tts::MiMoTTS::new(api_key, model.to_string(), None);
                let voice = mimo_voice.as_deref().unwrap_or("mimo_default");
                match tts_client.speak_builtin(&text, voice, None).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!("TTS FAILED: mimo_tts: {}", e);
                        return;
                    }
                }
            }
            "qwen_tts" => {
                let server_url = qwen_tts_server.unwrap_or_default();
                if server_url.is_empty() {
                    tracing::warn!("TTS FAILED: qwen_tts: server_url not set");
                    return;
                }
                let language = qwen_tts_language.unwrap_or_else(|| "English".to_string());
                let speaker = qwen_tts_speaker.clone();
                let tts_client = tts::qwen_tts::QwenTTSClient::new(server_url, language, speaker);
                let use_clone = qwen_voice_clone_enabled || qwen_voice_clone_audio_path.as_ref().map_or(false, |p| !p.is_empty());
                if use_clone {
                    if let Some(ref clone_path) = qwen_voice_clone_audio_path {
                        if !clone_path.is_empty() {
                            match tts_client.speak_voice_clone(&text, clone_path, qwen_voice_clone_prompt.as_deref()).await {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    tracing::warn!("TTS FAILED: qwen_tts voice_clone: {}", e);
                                    return;
                                }
                            }
                        } else {
                            tracing::warn!("TTS FAILED: qwen_tts: voice_clone_enabled but no audio path set");
                            return;
                        }
                    } else {
                        tracing::warn!("TTS FAILED: qwen_tts: voice_clone_enabled but no audio path set");
                        return;
                    }
                } else {
                    match tts_client.speak(&text).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            tracing::warn!("TTS FAILED: qwen_tts: {}", e);
                            return;
                        }
                    }
                }
            }
            other => {
                tracing::warn!("TTS FAILED: Unknown type: {}", other);
                return;
            }
        };

        tracing::trace!(user_id = %user_id, "TTS: Audio received ({} bytes)", audio_bytes.len());

        if let Some(ref path) = audio_output_path {
            let folder = tts::ensure_audio_folder(path, "generated_tts");
            let _ = tts::save_audio_file(
                &audio_bytes,
                folder.to_str().unwrap_or(path),
                "01_tts_original",
            );
        }

        let final_audio = if rvc_on {
            match tts::convert_through_rvc(audio_bytes, rvc_server, rvc_model_path, rvc_index_path)
                .await
            {
                Ok(converted) => {
                    if let Some(ref path) = audio_output_path {
                        let folder = tts::ensure_audio_folder(path, "rvc");
                        let _ = tts::save_audio_file(
                            &converted,
                            folder.to_str().unwrap_or(path),
                            "02_rvc_converted",
                        );
                    }
                    converted
                }
                Err(e) => {
                    tracing::warn!("TTS RVC FAILED: {}", e);
                    return;
                }
            }
        } else {
            audio_bytes
        };

        if let Some(ref path) = audio_output_path {
            let folder = tts::ensure_audio_folder(path, "final_output");
            let _ = tts::save_audio_file(&final_audio, folder.to_str().unwrap_or(path), "03_final");
        }

        crate::event_channel::broadcast_voice_tts(&user_id, final_audio);
    });
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_message_handler_compiles() {
        assert!(true);
    }
}
