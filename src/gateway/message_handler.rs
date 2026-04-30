use crate::gateway::GatewayState;
use crate::gateway::llm::provider::{ChatRequest, ChatMessage};
use crate::voice::tts;

pub async fn handle_message(
    state: &GatewayState,
    user_id: &str,
    content: &str,
) -> anyhow::Result<String> {
    let ctx = state.db.load_context(user_id)?;

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

    let tool_defs = get_tool_definitions();

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
            let result = execute_tool_call(tc).await;
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
            spawn_tts(reply.clone(), &updated_ctx.settings, user_id);
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
        spawn_tts(reply.clone(), &updated_ctx.settings, user_id);
    }

    Ok(reply)
}

async fn build_system_prompt(state: &GatewayState, ctx: &crate::db::contexts::Context) -> String {
    let template_path = match ctx.mode.as_str() {
        "agent" => "templates/system.poml",
        "chat" => "templates/system.poml",
        _ => "templates/system.poml",
    };

    let context = serde_json::json!({
        "user_name": ctx.user_name.as_deref().unwrap_or("User"),
        "mode": ctx.mode,
        "turn": ctx.turn,
        "system_info": format!("Praxis v{}", env!("CARGO_PKG_VERSION")),
    });

    match crate::gateway::poml::render(template_path, &context).await {
        Ok(rendered) => rendered,
        Err(e) => {
            tracing::warn!("Failed to render POML template: {}, using fallback", e);
            format!(
                "You are Praxis, an AI agent assistant. Current mode: {}. User: {}. Turn: {}.",
                ctx.mode,
                ctx.user_name.as_deref().unwrap_or("unknown"),
                ctx.turn
            )
        }
    }
}

fn get_tool_definitions() -> Vec<crate::gateway::llm::provider::ToolDefinition> {
    vec![
        crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: "execute_terminal".to_string(),
                description: "Execute a shell command and return output".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: "write_file".to_string(),
                description: "Write content to a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to write to"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: "edit_file".to_string(),
                description: "Edit a file by replacing text".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to edit"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "Text to find and replace"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "Replacement text"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
        },
        crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: "web_search".to_string(),
                description: "Search the web using DuckDuckGo".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: "read_file".to_string(),
                description: "Read the contents of a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to read"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
    ]
}

async fn execute_tool_call(tc: &crate::gateway::llm::provider::ToolCall) -> String {
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
            let old_text = args["old_text"].as_str().unwrap_or("");
            let new_text = args["new_text"].as_str().unwrap_or("");
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
        _ => format!("Unknown tool: {}", tc.function.name),
    }
}

fn spawn_tts(text: String, settings: &crate::db::contexts::ContextSettings, user_id: &str) {
    let tts_type = settings.voice_tts_type.clone();
    let rvc_on = settings.rvc_on;
    let rvc_server = settings.rvc_server.clone();
    let rvc_model_path = settings.rvc_model_path.clone();
    let rvc_index_path = settings.rvc_index_path.clone();
    let elevenlabs_api_key = settings.voice_elevenlabs_api_key.clone();
    let elevenlabs_voice_id = settings.voice_elevenlabs_voice_id.clone();
    let minimax_api_key = settings.minimax_api_key.clone();
    let minimax_voice_id = settings.minimax_voice_id.clone();
    let minimax_model = settings.minimax_tts_model.clone().unwrap_or_else(|| "speech-02-hd".to_string());
    let mimo_api_key = settings.mimo_api_key.clone();
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
                let tts_engine = tts::elevenlabs::ElevenLabsTTS::new(api_key, voice_id);
                match tts_engine.speak(&text).await {
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

        if let Err(e) = tts::play_audio_locally(&final_audio) {
            tracing::warn!("TTS local playback failed: {}", e);
        }
    });
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_message_handler_compiles() {
        assert!(true);
    }
}
