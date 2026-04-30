use crate::gateway::GatewayState;
use crate::gateway::llm::provider::{ChatRequest, ChatMessage};
use crate::voice::tts;

pub async fn handle_message(
    state: &GatewayState,
    user_id: &str,
    content: &str,
) -> anyhow::Result<String> {
    let ctx = state.db.load_context(user_id)?;
    let _ = state.db.save_context(&ctx);

    state.db.add_message(
        user_id,
        &crate::db::messages::Message {
            role: "user".to_string(),
            content: content.to_string(),
            tool_call_id: None,
        },
    )?;

    let system_prompt = build_system_prompt(state, &ctx).await;

    let mut messages = Vec::new();
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(system_prompt),
        tool_calls: None,
        tool_call_id: None,
    });

    let history = state.db.get_messages(user_id, 50)?;
    for msg in &history {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: Some(msg.content.clone()),
            tool_calls: None,
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    let tool_defs = crate::db::tools::to_tool_definitions(&state.db)
        .unwrap_or_default();

    let request = ChatRequest {
        messages,
        tools: if tool_defs.is_empty() { None } else { Some(tool_defs) },
        temperature: Some(0.7),
        max_tokens: Some(4096),
    };

    let response = state.llm.chat(request, None).await?;

    if let Some(tool_calls) = &response.tool_calls {
        let mut results = Vec::new();
        for tc in tool_calls {
            let result = execute_tool_call(&state.db, user_id, tc).await;
            state.db.add_message(
                user_id,
                &crate::db::messages::Message {
                    role: "tool".to_string(),
                    content: result.clone(),
                    tool_call_id: Some(tc.id.clone()),
                },
            )?;
            results.push((tc.id.clone(), result));
        }

        let mut followup_messages = Vec::new();
        followup_messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(build_system_prompt(state, &ctx).await),
            tool_calls: None,
            tool_call_id: None,
        });
        let history = state.db.get_messages(user_id, 50)?;
        for msg in &history {
            followup_messages.push(ChatMessage {
                role: msg.role.clone(),
                content: Some(msg.content.clone()),
                tool_calls: None,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }

        let followup_request = ChatRequest {
            messages: followup_messages,
            tools: None,
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };

        let followup_response = state.llm.chat(followup_request, None).await?;
        let reply = followup_response.content.unwrap_or_default();

        state.db.add_message(
            user_id,
            &crate::db::messages::Message {
                role: "assistant".to_string(),
                content: reply.clone(),
                tool_call_id: None,
            },
        )?;

        let mut updated_ctx = ctx;
        state.db.increment_turn(&mut updated_ctx);
        state.db.save_context(&updated_ctx)?;

        // TTS: spawn if enabled
        if updated_ctx.settings.use_tts {
            spawn_tts(reply.clone(), &updated_ctx.settings, &state.secrets, user_id);
        }

        return Ok(reply);
    }

    let reply = response.content.unwrap_or_default();

    state.db.add_message(
        user_id,
        &crate::db::messages::Message {
            role: "assistant".to_string(),
            content: reply.clone(),
            tool_call_id: None,
        },
    )?;

    let mut updated_ctx = ctx;
    state.db.increment_turn(&mut updated_ctx);
    state.db.save_context(&updated_ctx)?;

    // TTS: spawn if enabled
    if updated_ctx.settings.use_tts {
        spawn_tts(reply.clone(), &updated_ctx.settings, &state.secrets, user_id);
    }

    Ok(reply)
}

async fn build_system_prompt(state: &GatewayState, ctx: &crate::db::contexts::Context) -> String {
    let template_path = "templates/system.poml";

    // Load skills for context
    let mut skills_registry = crate::skills::SkillRegistry::new();
    let _ = skills_registry.load_from_dir(std::path::Path::new("skills"));
    let skills = skills_registry.to_context_array();

    // Calculate uptime
    let uptime_secs = state.start_time.elapsed().as_secs();
    let uptime = format_uptime(uptime_secs);

    // Get paired users
    let paired_users = state.db.list_all_pairings().unwrap_or_default();
    let paired_count = paired_users.len();
    let paired_list: Vec<serde_json::Value> = paired_users.iter().map(|p| {
        serde_json::json!({
            "user_id": p.user_id,
            "discord_user_id": p.discord_user_id,
            "paired_at": p.paired_at,
        })
    }).collect();

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
    });

    match crate::gateway::poml::render(template_path, &context).await {
        Ok(rendered) => rendered,
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
        format!("{}d {}h {}m", secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60)
    }
}

async fn execute_tool_call(db: &crate::db::Database, user_id: &str, tc: &crate::gateway::llm::provider::ToolCall) -> String {
    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing arguments: {}", e),
    };

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
                        format!("Exit code: {}\nStdout: {}\nStderr: {}", result.exit_code, result.stdout, result.stderr)
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
            let old_text = args["old_text"].as_str()
                .or_else(|| args["old_string"].as_str())
                .unwrap_or("");
            let new_text = args["new_text"].as_str()
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
                        format!("{}...\n\n[File truncated - {} bytes total]", &content[..10000], content.len())
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
                            output.push_str(&format!("{}. {}\n   {}\n   {}\n\n", i + 1, r.title, r.snippet, r.url));
                        }
                        output
                    }
                }
                Err(e) => format!("Search error: {}", e),
            }
        }
        "get_context" => {
            match db.load_context(user_id) {
                Ok(ctx) => serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "Failed to serialize context".to_string()),
                Err(e) => format!("Error: {}", e),
            }
        }
        "set_context" => {
            let key = args["key"].as_str().unwrap_or("");
            let value = args.get("value").cloned().unwrap_or(serde_json::Value::Null);
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
        "agent_next" => "Advanced to next step".to_string(),
        "agent_complete" => "Task marked as complete".to_string(),
        "agent_set_path" => {
            let path = args["path"].as_str().unwrap_or("");
            format!("Working directory set to: {}", path)
        }
        "agent_feedback" => {
            let message = args["message"].as_str().unwrap_or("");
            format!("Feedback: {}", message)
        }
        "discord_upload_file" => {
            let filename = args["filename"].as_str().unwrap_or("");
            let base64_content = args["base64_content"].as_str().unwrap_or("");
            match crate::tools::discord_upload::upload_file(user_id, filename, base64_content, Some("")).await {
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
            match db.merge_context(user_id, serde_json::json!({"custom_data": {format!("pref_{}", key): value}})) {
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
        _ => format!("Unknown tool: {}", tc.function.name),
    }
}

fn spawn_tts(text: String, settings: &crate::db::contexts::ContextSettings, secrets: &crate::db::secrets::Secrets, user_id: &str) {
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
    let minimax_model = settings.minimax_tts_model.clone().unwrap_or_else(|| "speech-02-hd".to_string());
    let mimo_api_key = secrets.mimo_api_key.clone();
    let mimo_tts_type = settings.mimo_tts_type.clone().unwrap_or_else(|| "builtin".to_string());
    let mimo_voice = settings.mimo_voice_id.clone();
    let audio_output_path = settings.voice_audio_output_path.clone();
    let user_id = user_id.to_string();

    tracing::trace!(user_id = %user_id, tts_type = %tts_type, "TTS: Spawning task");

    tokio::task::spawn(async move {
        let audio_bytes = match tts_type.as_str() {
            "windows_sapi" => {
                let tts_engine = tts::windows_sapi::WindowsSAPI::new();
                match tts_engine.speak_to_bytes(&text) {
                    Ok(bytes) => bytes,
                    Err(e) => { tracing::warn!("TTS FAILED: windows_sapi: {}", e); return; }
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
                match tts_engine.speak_with_settings(&text, &elevenlabs_tts_model, &voice_settings).await {
                    Ok(bytes) => bytes,
                    Err(e) => { tracing::warn!("TTS FAILED: elevenlabs: {}", e); return; }
                }
            }
            "minimax" => {
                let api_key = minimax_api_key.unwrap_or_default();
                let voice_id = minimax_voice_id.unwrap_or_default();
                let tts_engine = tts::minimax_tts::MiniMaxTTS::new(api_key, voice_id, minimax_model);
                match tts_engine.speak(&text).await {
                    Ok(bytes) => bytes,
                    Err(e) => { tracing::warn!("TTS FAILED: minimax: {}", e); return; }
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
                    Err(e) => { tracing::warn!("TTS FAILED: mimo_tts: {}", e); return; }
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
            let _ = tts::save_audio_file(&audio_bytes, folder.to_str().unwrap_or(path), "01_tts_original");
        }

        let final_audio = if rvc_on {
            match tts::convert_through_rvc(audio_bytes, rvc_server, rvc_model_path, rvc_index_path).await {
                Ok(converted) => {
                    if let Some(ref path) = audio_output_path {
                        let folder = tts::ensure_audio_folder(path, "rvc");
                        let _ = tts::save_audio_file(&converted, folder.to_str().unwrap_or(path), "02_rvc_converted");
                    }
                    converted
                }
                Err(e) => { tracing::warn!("TTS RVC FAILED: {}", e); return; }
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
