use crate::gateway::llm::provider::{ChatMessage, ChatRequest};
use crate::gateway::GatewayState;
use crate::voice::tts;

pub async fn handle_message(
    state: &GatewayState,
    user_id: &str,
    content: &str,
    channel_id: Option<&str>,
) -> anyhow::Result<String> {
    let _task = crate::gateway::task_control::begin(user_id)?;
    handle_message_inner(state, user_id, content, channel_id).await
}

pub(crate) async fn handle_message_inner(
    state: &GatewayState,
    user_id: &str,
    content: &str,
    channel_id: Option<&str>,
) -> anyhow::Result<String> {
    // A prior agent_complete must not stop this independent task after one tool.
    crate::gateway::prompt::reset_task_completion(&state.db, user_id)?;
    // Route before deciding the path and before either prompt is rendered.
    let ctx = crate::gateway::prompt::prepare_runtime(state, user_id, content, None, channel_id)?;
    let max_turns = ctx.settings.max_llm_turns.unwrap_or(1);

    // Use agent loop when max_turns > 1
    if max_turns > 1 {
        return handle_message_agent_loop(state, user_id, content, channel_id, &ctx, max_turns)
            .await;
    }

    // One user-facing turn can require several tool-only LLM responses. Keep
    // this chat path bounded without mistaking a tool response for completion.
    // Snapshot the budget: a tool cannot grow its own budget during this task.
    let tool_limit = ctx.settings.max_tool_calls.unwrap_or(5).max(0) as usize;
    let mut tool_calls_used = 0usize;
    let mut current_tool_ids = std::collections::HashSet::new();
    let mut finalizing = tool_limit == 0;
    let mut ctx = ctx;
    let rendered_user = crate::gateway::prompt::render_user(state, &ctx, content).await?;
    state.db.add_message(user_id, &crate::db::messages::Message::user(rendered_user))?;
    let system_prompt = crate::gateway::prompt::render_system(state, &ctx, content).await?;

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
    let mut tool_names: Vec<String> = tool_defs.iter().map(|t| t.function.name.clone()).collect();
    let mut tools_for_validation = tool_defs.clone();

    let request = ChatRequest {
        messages,
        tools: if finalizing || tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs)
        },
        temperature: Some(0.7),
        max_tokens: Some(state.llm.task_output_tokens()),
        model: ctx.settings.model.clone(),
        vision_provider: ctx.settings.vision_provider.clone().or_else(|| state.config.vision_provider.clone()),
        vision_model: ctx.settings.vision_model.clone().or_else(|| state.config.vision_model.clone()),
        thinking: crate::gateway::llm::provider::ThinkingMode::from_setting(&ctx.settings.thinking_mode),
    };

    let mut response = state
        .llm
        .streaming_chat(request, ctx.settings.provider.as_deref(), user_id)
        .await?;

    while let Some(tool_calls) = &response.tool_calls {
        current_tool_ids.extend(tool_calls.iter().map(|call| call.id.clone()));
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
            if crate::gateway::task_control::cancellation(user_id).is_some_and(|token| token.is_cancelled()) {
                let mut msg = crate::db::messages::Message::tool("Error: Task cancelled; tool not executed".into(), tc.id.clone());
                msg.tool_name = Some(tc.function.name.clone());
                state.db.add_message(user_id, &msg)?;
                continue;
            }
            if tool_calls_used >= tool_limit {
                let mut msg = crate::db::messages::Message::tool(
                    "Error: Maximum tool-call limit reached; tool not executed".into(),
                    tc.id.clone(),
                );
                msg.tool_name = Some(tc.function.name.clone());
                state.db.add_message(user_id, &msg)?;
                continue;
            }
            // Invalid calls also consume budget; otherwise malformed/unknown
            // calls could keep a recovery loop running indefinitely.
            tool_calls_used += 1;
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

        if crate::gateway::task_control::cancellation(user_id).is_some_and(|token| token.is_cancelled()) {
            return Err(crate::gateway::llm::error::ProviderError::new(
                crate::gateway::llm::error::ErrorKind::Cancelled,
            ).into());
        }
        if finalizing {
            anyhow::bail!("Maximum tool-call limit reached ({tool_limit}); provider requested more tools instead of a final answer. Tool results are saved; the task was not silently completed.");
        }

        // Context tools must affect the very next LLM request, not be overwritten
        // by the pre-tool snapshot at the end of this handler.
        ctx = crate::gateway::prompt::prepare_runtime(state, user_id, content, None, channel_id)?;
        let mut followup_messages = Vec::new();
        followup_messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(crate::gateway::prompt::render_system(state, &ctx, content).await?),
            content_parts: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });
        let (history, _tokens) = state
            .db
            .get_messages_with_token_budget(user_id, ctx.settings.history_token_limit.unwrap_or(500000))?;

        let image_msg_indices = crate::gateway::prompt::history_image_indices(
            &history, &current_tool_ids, state.llm.history_image_messages(),
        );
        if history.iter().enumerate().any(|(i, m)|
            m.content_parts.as_ref().is_some_and(|p| !p.is_empty()) && !image_msg_indices.contains(&i)
        ) {
            followup_messages.push(ChatMessage {
                role: "system".into(),
                content: Some(crate::gateway::prompt::OMITTED_HISTORY_IMAGES_NOTICE.into()),
                content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None,
            });
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

        // Keep tools available after use_skill/read_file/etc. Refresh definitions
        // as well as context, so newly disabled tools cannot run next round.
        let mut tool_defs = crate::db::tools::to_tool_definitions(&state.db)?;
        tool_defs.extend(state.plugins.tool_definitions());
        tool_names = tool_defs.iter().map(|t| t.function.name.clone()).collect();
        tools_for_validation = tool_defs.clone();
        finalizing = tool_calls_used >= tool_limit;
        if finalizing {
            followup_messages.push(ChatMessage {
                role: "system".into(),
                content: Some("The tool-call budget for this request is exhausted. No more tools may run. Give an honest final status using the saved tool results, explicitly stating any unfinished work. Do not claim unexecuted actions succeeded.".into()),
                content_parts: None,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            });
        }
        let followup_request = ChatRequest {
            messages: followup_messages,
            tools: if finalizing || tool_defs.is_empty() { None } else { Some(tool_defs) },
            temperature: Some(0.7),
            max_tokens: Some(state.llm.task_output_tokens()),
            model: ctx.settings.model.clone(),
            vision_provider: ctx.settings.vision_provider.clone().or_else(|| state.config.vision_provider.clone()),
            vision_model: ctx.settings.vision_model.clone().or_else(|| state.config.vision_model.clone()),
            thinking: crate::gateway::llm::provider::ThinkingMode::from_setting(&ctx.settings.thinking_mode),
        };

        response = state
            .llm
            .streaming_chat(followup_request, ctx.settings.provider.as_deref(), user_id)
            .await?;
    }

    let reply = response.content.unwrap_or_default();

    state.db.add_message(
        user_id,
        &crate::db::messages::Message::assistant(reply.clone()),
    )?;

    let mut updated_ctx = state.db.load_context(user_id)?;
    state.db.increment_turn(&mut updated_ctx);
    state.db.save_context(&updated_ctx)?;

    if updated_ctx.settings.use_tts {
        spawn_tts(
            reply.clone(),
            &updated_ctx.settings,
            &state.secrets,
            user_id,
            &state.db,
        );
    }

    Ok(reply)
}

fn stream_response(user_id: &str, reply: &str) {
    // No-op: real streaming is handled by streaming_chat in LLMRouter
    // This function is kept for backward compatibility
    _ = user_id;
    _ = reply;
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

    // Inject channel_id into custom_data so tools can use it.
    // Always overwrite with the CURRENT channel so replies/tools follow the user
    // (previously it was written once and never updated, so later messages from
    // other channels kept the stale value and outputs landed in random channels).
    // Voice input keeps the last real text channel for text tools and records the
    // voice channel separately under voice_channel_id.
    let mut ctx = ctx.clone();
    if ctx.custom_data.is_null() {
        ctx.custom_data = serde_json::json!({});
    }
    if let Some(obj) = ctx.custom_data.as_object_mut() {
        let is_voice = channel_id.map_or(false, |ch| ch.starts_with("voice:"));
        let text_channel: Option<String> = if is_voice {
            // STT/voice input: text tools should target the last text channel the
            // user actually wrote in; fall back to a non-voice feedback channel.
            obj.get("user_channel_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    ctx.settings
                        .feedback_channel_id
                        .clone()
                        .filter(|s| !s.starts_with("voice:"))
                })
        } else {
            channel_id
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| {
                    ctx.settings
                        .feedback_channel_id
                        .clone()
                        .filter(|s| !s.starts_with("voice:"))
                })
        };
        if let Some(ch) = text_channel {
            if !ch.is_empty() {
                obj.insert("user_channel_id".to_string(), serde_json::json!(ch));
                obj.insert("channel_id".to_string(), serde_json::json!(ch));
            }
        }
        if is_voice {
            if let Some(ch) = channel_id {
                obj.insert("voice_channel_id".to_string(), serde_json::json!(ch));
            }
        }
        // STT language confidence for the transcript-check rule (see voice::last_stt_confidence)
        if let Some(c) = crate::voice::last_stt_confidence() {
            let threshold = ctx.settings.stt_low_confidence_threshold;
            obj.insert("stt_confidence".to_string(), serde_json::json!(c));
            if c < threshold {
                obj.insert("stt_low_confidence".to_string(), serde_json::json!(true));
            } else {
                obj.remove("stt_low_confidence");
            }
        }
    }
    state.db.save_context(&ctx)?;

    let config = crate::gateway::agent_loop::AgentLoopConfig {
        max_turns,
        max_tool_calls: ctx.settings.max_tool_calls.unwrap_or(5),
        tags_enabled: ctx.settings.tags_enabled,
        sm_file: ctx.settings.sm_file.clone().or(ctx.sm_file.clone()),
        feedback_enabled: !feedback_modes.is_empty(),
        message_on_toolcalling: ctx.settings.message_on_toolcalling,
        tool_history_limit: ctx.settings.tool_history_limit,
    };

    let (feedback_tx, mut feedback_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let use_tts = ctx.settings.use_tts;
    let is_voice_input = channel_id.map_or(false, |ch| ch.starts_with("voice:"));
    let tts_for_feedback = use_tts || is_voice_input;

    // Spawn feedback routing task (always for web stream)
    let is_web = channel_id.map_or(true, |ch| ch == "web" || ch.is_empty());
    let feedback_modes = feedback_modes.clone();
    let uid = user_id_owned.clone();
    let ch = feedback_channel.clone();
    let tts_settings = settings.clone();
    let tts_secrets = secrets.clone();
    let tts_db = state.db.clone();

    tokio::spawn(async move {
        while let Some(msg) = feedback_rx.recv().await {
            if is_web {
                crate::dashboard::stream::send(&uid, "feedback", &msg);
            }
            let mut handled = false;
            for mode in &feedback_modes {
                match mode.as_str() {
                    "tts" => {
                        spawn_tts(msg.clone(), &tts_settings, &tts_secrets, &uid, &tts_db);
                        handled = true;
                    }
                    "dm" => {
                        crate::event_channel::broadcast_agent_feedback(&uid, &msg);
                        crate::dashboard::stream::send(&uid, "feedback", &msg);
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
                        crate::dashboard::stream::send(&uid, "feedback", &msg);
                        handled = true;
                    }
                    _ => {}
                }
            }
            if !handled && tts_for_feedback {
                spawn_tts(msg.clone(), &tts_settings, &tts_secrets, &uid, &tts_db);
            }
        }
    });

    let result = crate::gateway::agent_loop::run_agent_loop_in_task(
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

    // Final response TTS: always speak if use_tts is on, or if input came from voice.
    // Web chat: speak only when the dashboard switch (web_chat_tts) is enabled.
    let is_voice_input = channel_id.map_or(false, |ch| ch.starts_with("voice:"));
    let is_web_chat = channel_id.map_or(false, |ch| ch == "web");
    let web_tts_on = updated_ctx.settings.web_chat_tts;
    if updated_ctx.settings.use_tts || is_voice_input || (is_web_chat && web_tts_on) {
        spawn_tts(
            reply.clone(),
            &updated_ctx.settings,
            &state.secrets,
            user_id,
            &state.db,
        );
    }

    Ok(reply)
}

async fn execute_tool_call(
    db: &crate::db::Database,
    user_id: &str,
    tc: &crate::gateway::llm::provider::ToolCall,
    plugins: &crate::plugins::PluginRegistry,
) -> String {
    tracing::info!(tool = %tc.function.name, args_bytes = tc.function.arguments.len(), "execute_tool_call: dispatching");

    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing arguments: {}", e),
    };

    let ctx_data = db
        .load_context(user_id)
        .ok()
        .map(|ctx| ctx.custom_data)
        .filter(|v| !v.is_null());

    let all_secrets = crate::db::secrets::get_secrets();
    let plugin_secrets = plugins.secrets_for_tool(&tc.function.name, &all_secrets);

    match tc.function.name.as_str() {
        "search_skills" => crate::tools::search_skills::run(db, &args).await
            .unwrap_or_else(|e| format!("Error: {e}")),
        "use_skill" => crate::tools::use_skill::run(db, &args).await
            .unwrap_or_else(|e| format!("Error: {}", e)),
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
                            crate::util::truncate_chars(&content, 10000),
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
            match db.merge_context_from_agent(user_id, serde_json::json!({key: value})) {
                Ok(_) => format!("Context key '{}' set", key),
                Err(e) => format!("Error: {}", e),
            }
        }
        "delete_context" => {
            let key = args["key"].as_str().unwrap_or("");
            match db.merge_context_from_agent(user_id, serde_json::json!({key: null})) {
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
            let value = args.get("value").cloned().unwrap_or(serde_json::Value::Null);
            match crate::db::memory::update_memory(db, user_id, |memory| {
                crate::db::memory::update_preference(memory, key, &value);
            }) {
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
            let is_web = channel_id == "web" || channel_id.is_empty() || paired_discord_user_id.is_empty();
            if channel_id.is_empty() && !paired_discord_user_id.is_empty() {
                return "Error: No channel_id provided and no originating channel found. Please specify a channel_id.".to_string();
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
            if is_web {
                tracing::info!(timeout_secs = timeout, "ask_questions: web mode");
                match crate::tools::web_interactive::ask_questions_web(user_id, &questions, timeout).await {
                    Ok(result) => result,
                    Err(e) => format!("Error: {}", e),
                }
            } else {
                tracing::info!(timeout_secs = timeout, "ask_questions: waiting for responses");
                match crate::tools::discord_interactive::ask_questions(channel_id, &questions, timeout, &paired_discord_user_id).await {
                    Ok(result) => result,
                    Err(e) => format!("Error: {}", e),
                }
            }
        }
        "update_template" => crate::tools::update_template::run(db, &args).await
            .unwrap_or_else(|e| format!("Error: {}", e)),
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
            Err(e) => format!("Plugin tool {} failed: {}", tc.function.name, e),
        },
    }
}

fn spawn_tts(
    text: String,
    settings: &crate::db::contexts::ContextSettings,
    secrets: &crate::db::secrets::Secrets,
    user_id: &str,
    db: &crate::db::Database,
) {
    // Tool-only turns, empty tag output and whitespace are not speech. Return
    // before spawning a task or constructing/contacting any TTS provider.
    if text.trim().is_empty() {
        return;
    }
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
    let web_chat_tts = settings.web_chat_tts;
    let user_id = user_id.to_string();
    let tts_db = db.clone();

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

        // Cost ledger: record ElevenLabs character usage per user.
        if tts_type == "elevenlabs" {
            let chars = text.chars().count();
            if let Err(e) = crate::db::memory::record_media_spend(&tts_db, &user_id, chars) {
                tracing::warn!("media_spend record failed: {}", e);
            }
        }

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

        // Web chat speech: deliver the rendered audio to the dashboard stream as
        // a base64 data URL so the web chat can play it (toggle: web_chat_tts).
        // Discord voice users receive the audio through the voice pipeline.
        if web_chat_tts {
            use base64::Engine;
            let data_url = format!(
                "data:audio/mpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&final_audio)
            );
            crate::dashboard::stream::send(
                &user_id,
                "chat_tts",
                &serde_json::json!({
                    "audio": data_url,
                    "mime": "audio/mpeg",
                })
                .to_string(),
            );
        }

        crate::event_channel::broadcast_voice_tts(&user_id, final_audio);
    });
}

#[cfg(test)]
#[path = "skill_dispatch_tests.rs"]
mod skill_dispatch_tests;

#[cfg(test)]
#[path = "backend_dispatch_tests.rs"]
mod backend_dispatch_tests;

#[cfg(test)]
#[path = "message_tool_loop_tests.rs"]
mod message_tool_loop_tests;

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_message_handler_compiles() {
        assert!(true);
    }
}
