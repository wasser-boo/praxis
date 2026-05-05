use crate::cl;
use crate::gateway::llm::provider::{ChatMessage, ChatRequest, ToolCall};
use crate::gateway::GatewayState;
use crate::tags::{self, TagExecution};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Global registry of active agent loops and their user input channels
static ACTIVE_LOOPS: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, UserInputChannel>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Global registry of stop signals for agent loops
static STOP_SIGNALS: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, bool>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Get a sender for injecting user input into an active agent loop
pub async fn get_user_input_sender(user_id: &str) -> Option<tokio::sync::mpsc::UnboundedSender<String>> {
    let loops = ACTIVE_LOOPS.read().await;
    loops.get(user_id).map(|ch| ch.sender())
}

/// Stop an active agent loop
pub async fn stop_agent_loop(user_id: &str) {
    let mut signals = STOP_SIGNALS.write().await;
    signals.insert(user_id.to_string(), true);
    
    // Also remove the input channel to unblock any waiting
    let mut loops = ACTIVE_LOOPS.write().await;
    loops.remove(user_id);
}

/// Check if a stop signal has been sent
async fn should_stop(user_id: &str) -> bool {
    let signals = STOP_SIGNALS.read().await;
    signals.get(user_id).copied().unwrap_or(false)
}

/// Register an active agent loop
async fn register_active_loop(user_id: &str, channel: UserInputChannel) {
    let mut loops = ACTIVE_LOOPS.write().await;
    loops.insert(user_id.to_string(), channel);
}

/// Unregister an active agent loop
async fn unregister_active_loop(user_id: &str) {
    let mut loops = ACTIVE_LOOPS.write().await;
    loops.remove(user_id);
}

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

/// Channel for injecting user messages into a running agent loop
pub struct UserInputChannel {
    rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl UserInputChannel {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self { rx, tx }
    }

    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<String> {
        self.tx.clone()
    }

    /// Try to receive a user message without blocking
    pub fn try_recv(&mut self) -> Option<String> {
        self.rx.try_recv().ok()
    }
}

pub async fn run_agent_loop(
    state: &GatewayState,
    user_id: &str,
    user_message_input: &str,
    config: AgentLoopConfig,
    feedback_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> anyhow::Result<AgentLoopResult> {
    let mut turn = 0;
    let mut completed = false;
    let mut advanced = false;
    let mut last_tag_execution = None;
    let mut user_message = user_message_input.to_string();

    // Create user input channel and register it globally
    let mut user_input = UserInputChannel::new();
    let user_input_tx = user_input.sender();
    register_active_loop(user_id, UserInputChannel::new()).await;

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

            let effective_path = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/".to_string());
            obj.insert("path".to_string(), serde_json::json!(effective_path));
            obj.insert(
                "time".to_string(),
                serde_json::json!(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
            );

            // VM context: expose VM state and available ISOs to the LLM
            let vm_enabled = std::env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if vm_enabled {
                let vm_mode = std::env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string());
                let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
                obj.insert("vm_enabled".to_string(), serde_json::json!(true));
                obj.insert("vm_mode".to_string(), serde_json::json!(vm_mode));
                obj.insert(
                    "vm_storage_dir".to_string(),
                    serde_json::json!(format!("{}/vm", data_dir)),
                );
                obj.insert(
                    "vm_iso_dir".to_string(),
                    serde_json::json!(format!("{}/vm/isos", data_dir)),
                );
                obj.insert(
                    "vm_shared_dir".to_string(),
                    serde_json::json!(format!("{}/shared", data_dir)),
                );

                if let Some(manager) = crate::tools::vm_tools::get_vm_manager().await {
                    obj.insert(
                        "installation_disks".to_string(),
                        serde_json::json!(manager.list_isos()),
                    );
                    obj.insert(
                        "running_vms".to_string(),
                        serde_json::json!(manager.list_vms().await),
                    );
                }
            }

            let user_template = obj
                .get("user_template")
                .cloned()
                .unwrap_or_else(|| serde_json::json!("user"));
            obj.insert("user_template".to_string(), user_template);

            // Default channel_id for Discord tools (can be overwritten via CL)
            if !obj.contains_key("channel_id") {
                let fallback_ch = ctx.settings.feedback_channel_id.clone().unwrap_or_default();
                if !fallback_ch.is_empty() {
                    obj.insert("channel_id".to_string(), serde_json::json!(fallback_ch));
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

            let tools = crate::db::tools::to_tool_definitions(&state.db).unwrap_or_default();
            let tools_context: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters
                })
            }).collect();
            obj.insert("tools".to_string(), serde_json::json!(tools_context));

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

    // Apply user template
    let user_template_name = ctx
        .custom_data
        .get("user_template")
        .and_then(|v| v.as_str())
        .unwrap_or("user");
    let user_template_path = format!("templates/{}.poml", user_template_name);
    tracing::info!(user_id = %ctx.user_id, user_template = %user_template_name, "Using user template");
    let rendered_user_message = if std::path::Path::new(&user_template_path).exists() {
        let tmpl_ctx = serde_json::json!({
            "user_prompt": user_message,
            "custom_data": ctx.custom_data,
        });
        crate::gateway::poml::render(&user_template_path, &tmpl_ctx)
            .await
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
        // Check for stop signal
        if should_stop(user_id).await {
            tracing::info!(user_id = %user_id, "Stop signal received, ending agent loop");
            break;
        }

        // Check for user input during execution
        let mut last_response_mode = false;
        if let Some(user_msg) = user_input.try_recv() {
            tracing::info!(user_id = %user_id, turn = turn, "Received user input during agent loop: {}", user_msg);
            
            // Check for special signals
            if user_msg == "__LAST_RESPONSE__" {
                last_response_mode = true;
                tracing::info!(user_id = %user_id, "Last response mode activated");
            } else {
                // Store user message in history
                state.db.add_message(
                    user_id,
                    &crate::db::messages::Message::user(user_msg.clone()),
                )?;
                
                // Reset turn counter to allow more turns
                turn = 0;
                
                // Update user_message for subsequent turns
                user_message = user_msg;
                
                tracing::info!(user_id = %user_id, "Updated user_message to: {}", user_message);
            }
        }

        if turn >= config.max_turns {
            tracing::warn!(user_id = %user_id, turn = turn, "Max turns reached");
            break;
        }

        turn += 1;
        ctx.turn = turn;

        // Reload context from DB to pick up changes from set_context
        // (e.g. screen transitions that change the system_template)
        if let Ok(fresh_ctx) = state.db.load_context(user_id) {
            ctx = fresh_ctx;
            ctx.turn = turn;
        }

        // Apply CL workflow (auto rules only, transitions are on agent_next)
        if let Some(ref cl_path) = config.cl_file {
            match cl::load_file(cl_path) {
                Ok(cl) => {
                    let mut ctx_val = serde_json::to_value(&ctx)?;
                    let old_template = ctx_val.pointer("/settings/system_template").and_then(|v| v.as_str()).unwrap_or("system").to_string();
                    let _secret_changes = cl::apply_to_context(&cl, &mut ctx_val);
                    let new_template = ctx_val.pointer("/settings/system_template").and_then(|v| v.as_str()).unwrap_or("system").to_string();
                    let current_state = ctx_val.get("active_state").and_then(|v| v.as_str()).unwrap_or("");
                    tracing::info!(cl_path = %cl_path, active_state = %current_state, old_template = %old_template, new_template = %new_template, "CL workflow applied");
                    if let Ok(updated_ctx) = serde_json::from_value::<crate::db::contexts::Context>(ctx_val)
                    {
                        ctx = updated_ctx;
                    }
                }
                Err(e) => {
                    tracing::warn!(cl_path = %cl_path, error = %e, "Failed to load CL file");
                }
            }
        }

        let _ = state.db.save_context(&ctx);

        // Build messages - always use current user_message
        let system_prompt = build_system_prompt(state, &ctx, &user_message).await;
        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt),
            content_parts: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }];

        // Add compaction summary if available
        if ctx.settings.compaction_enabled && !ctx.settings.compaction_summary.is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Some(format!(
                    "Previous conversation summary: {}",
                    ctx.settings.compaction_summary
                )),
                content_parts: None,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            });
        }

        // Inject latest VM screenshot so the LLM always sees the current state
        if let Some(ss_path) = ctx
            .custom_data
            .get("vm_last_screenshot")
            .and_then(|v| v.as_str())
        {
            if let Some(data_url) = crate::tools::vm_tools::screenshot_to_data_url(ss_path) {
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(
                        "[Current VM screenshot — this is what is on screen right now]".to_string(),
                    ),
                    content_parts: Some(vec![
                        crate::gateway::llm::provider::ContentPart::ImageUrl {
                            image_url: crate::gateway::llm::provider::ImageUrlDetail {
                                url: data_url,
                                detail: Some("high".to_string()),
                            },
                        },
                    ]),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                });
            }
        }

        let token_budget = ctx.settings.history_token_limit.unwrap_or(500000);
        let (history, _history_tokens) = state
            .db
            .get_messages_with_token_budget(user_id, token_budget)?;

        // Find the indices of the last 2 messages that have content_parts
        // Only those will get image data sent to the LLM
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
                content_parts: {
                    // Only include image data for the last 2 messages with content_parts
                    if image_msg_indices.contains(&msg_idx) {
                        let cp = msg.content_parts.as_ref().map(|parts| {
                            let converted: Vec<crate::gateway::llm::provider::ContentPart> = parts
                                .iter()
                                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                                .collect();
                            converted
                        });
                        cp
                    } else {
                        None
                    }
                },
                tool_calls,
                tool_call_id: msg.tool_call_id.clone(),
                tool_name: msg.tool_name.clone(),
            });
        }

        // Get tool definitions from database + plugins
        let mut tools = crate::db::tools::to_tool_definitions(&state.db).unwrap_or_default();
        tools.extend(state.plugins.tool_definitions());

        // Clone tool names and definitions for validation before moving tools into request
        let tool_names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();
        let tools_for_validation = tools.clone();

        let request = ChatRequest {
            messages: messages.clone(),
            tools: if tools.is_empty() { None } else { Some(tools) },
            temperature: Some(0.7),
            max_tokens: Some(4096),
            model: ctx.settings.model.clone(),
            vision_provider: ctx.settings.vision_provider.clone().or_else(|| state.config.vision_provider.clone()),
            vision_model: ctx.settings.vision_model.clone().or_else(|| state.config.vision_model.clone()),
        };

        // Log LLM request details
        let msg_count = request.messages.len();
        let tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        tracing::info!(
            user_id = %user_id,
            turn = ctx.turn,
            message_count = msg_count,
            tool_count = tool_count,
            model = ?request.model,
            provider = ?ctx.settings.provider,
            ">>> LLM REQUEST >>>"
        );

        // Call LLM
        let response = match state
            .llm
            .chat(request, ctx.settings.provider.as_deref())
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(user_id = %user_id, error = %e, "<<< LLM ERROR <<<");
                if let Some(ref tx) = feedback_tx {
                    let _ = tx.send(format!("LLM error: {}", e));
                }
                return Err(e);
            }
        };

        // Log LLM response details
        let has_tool_calls = response.tool_calls.is_some();
        let tool_calls_count = response.tool_calls.as_ref().map(|tc| tc.len()).unwrap_or(0);
        let response_preview = response.content.as_deref()
            .map(|c| if c.len() > 300 { format!("{}...", &c[..300]) } else { c.to_string() })
            .unwrap_or_else(|| "(no content)".to_string());
        tracing::info!(
            user_id = %user_id,
            has_tool_calls = has_tool_calls,
            tool_calls_count = tool_calls_count,
            response_preview = %response_preview,
            "<<< LLM RESPONSE <<<"
        );

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
                    let send_first = ctx
                        .custom_data
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

                // Validate tool exists before executing
                if !tool_names.contains(&tc.function.name) {
                    let result = format!("Error: Unknown tool '{}'. Check the tool name and try again.", tc.function.name);
                    tracing::warn!(user_id = %user_id, tool = %tc.function.name, "Unknown tool called");
                    
                    // Add error as tool result so LLM can recover
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(result.clone()),
                        content_parts: None,
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        tool_name: Some(tc.function.name.clone()),
                    });
                    continue;
                }

                // Validate required parameters
                let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                if let Err(e) = validate_tool_params(&tc.function.name, &args, &tools_for_validation) {
                    let result = format!("Error: {}", e);
                    tracing::warn!(user_id = %user_id, tool = %tc.function.name, error = %e, "Invalid tool parameters");
                    
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(result.clone()),
                        content_parts: None,
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        tool_name: Some(tc.function.name.clone()),
                    });
                    continue;
                }

                if let Some(ref tx) = feedback_tx {
                    if config.feedback_enabled && config.message_on_toolcalling {
                        let _ = tx.send(format!("Calling tool: {}", tc.function.name));
                    }
                }

                // Log tool call start with full details
                tracing::info!(
                    user_id = %user_id,
                    tool = %tc.function.name,
                    call_id = %tc.id,
                    arguments = %tc.function.arguments,
                    "=== TOOL CALL START ==="
                );

                let result = execute_tool_call(&state.db, user_id, tc, &state.plugins).await;
                let mut final_result = result.clone();
                let mut image_content_parts: Option<Vec<serde_json::Value>> = None;

                // Log tool call result
                let result_preview = if result.len() > 500 {
                    format!("{}... ({} chars total)", &result[..500], result.len())
                } else {
                    result.clone()
                };
                tracing::info!(
                    user_id = %user_id,
                    tool = %tc.function.name,
                    call_id = %tc.id,
                    result = %result_preview,
                    "=== TOOL CALL END ==="
                );

                // Check if understand_image returned image data
                if tc.function.name == "understand_image" {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result) {
                        if let Some(parts) = parsed.get("content_parts").and_then(|v| v.as_array())
                        {
                            if !parts.is_empty() {
                                final_result = parsed
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&result)
                                    .to_string();
                                image_content_parts = Some(parts.clone());
                                tracing::info!(
                                    "understand_image: got {} content_parts, text: {}",
                                    parts.len(),
                                    final_result
                                );
                            }
                        }
                    }
                }

                // Log ALL tool calls to vm_activity_log for dashboard visibility
                {
                    let conn = state.db.conn();
                    let _ = conn.execute(
                        "INSERT INTO vm_activity_log (vm_id, action, input, output) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![
                            "praxis-vm",
                            &tc.function.name,
                            &tc.function.arguments,
                            if result.len() > 2000 { &result[..2000] } else { &result }
                        ],
                    );
                }

                // Auto-screenshot after VM tool calls
                if tc.function.name.starts_with("vm_") && tc.function.name != "vm_screenshot" {
                    let auto_screenshot = ctx.settings.vm_screenshot_enabled;
                    if auto_screenshot {
                        let data_dir =
                            std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
                        if let Some(path) =
                            crate::tools::vm_tools::save_screenshot_to_disk("praxis-vm", &data_dir)
                                .await
                        {
                            final_result = format!("{}\n\nScreenshot saved to: {}", result, path);
                            if ctx.custom_data.is_object() {
                                ctx.custom_data["vm_last_screenshot"] = serde_json::json!(path);
                            } else {
                                ctx.custom_data = serde_json::json!({ "vm_last_screenshot": path });
                            }
                        }
                    }
                }

                let msg = if let Some(parts) = image_content_parts {
                    crate::db::messages::Message::tool_with_image(
                        final_result,
                        tc.id.clone(),
                        parts,
                    )
                } else {
                    crate::db::messages::Message::tool(final_result, tc.id.clone())
                };
                // Set tool_name for Ollama compatibility
                let mut msg = msg;
                msg.tool_name = Some(tc.function.name.clone());
                state.db.add_message(user_id, &msg)?;
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

        // No tool calls — process response (final answer)
        let raw_response = response.content.unwrap_or_default();
        tracing::info!(
            user_id = %user_id,
            turn = ctx.turn,
            response_len = raw_response.len(),
            "<<< LLM FINAL RESPONSE (no tool calls) <<<"
        );

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

        // Auto-compact if enabled and total history tokens exceed limit
        if ctx.settings.compaction_enabled {
            let compaction_limit = ctx.settings.compaction_token_limit.unwrap_or(500000);
            let msg_count = state.db.count_messages(user_id).unwrap_or(0);
            let (_all_messages, total_tokens) = state
                .db
                .get_messages_with_token_budget(user_id, usize::MAX)
                .unwrap_or((vec![], 0));
            tracing::info!(
                user_id = %user_id,
                msg_count = msg_count,
                total_tokens = total_tokens,
                compaction_limit = compaction_limit,
                compaction_enabled = ctx.settings.compaction_enabled,
                "Compaction check"
            );
            if total_tokens > compaction_limit {
                tracing::info!(user_id = %user_id, tokens = total_tokens, limit = compaction_limit, "Auto-compacting conversation");
                if let Ok(summary) = generate_compaction_summary(
                    state,
                    user_id,
                    ctx.settings.compaction_template.as_deref(),
                )
                .await
                {
                    ctx.settings.compaction_summary = summary;
                    let _ = state.db.save_context(&ctx);

                    let keep_budget = ctx.settings.history_token_limit.unwrap_or(500000) / 2;
                    if let Ok((recent, _)) = state
                        .db
                        .get_messages_with_token_budget(user_id, keep_budget)
                    {
                        // Filter out orphaned tool results (tool messages without preceding tool_calls)
                        let mut valid_tool_call_ids: std::collections::HashSet<String> =
                            std::collections::HashSet::new();
                        for msg in &recent {
                            if let Some(ref tcs) = msg.tool_calls {
                                for tc in tcs {
                                    valid_tool_call_ids.insert(tc.id.clone());
                                }
                            }
                        }
                        let filtered: Vec<_> = recent
                            .into_iter()
                            .filter(|msg| {
                                if msg.role == "tool" {
                                    msg.tool_call_id
                                        .as_ref()
                                        .map(|id| valid_tool_call_ids.contains(id))
                                        .unwrap_or(false)
                                } else {
                                    true
                                }
                            })
                            .collect();

                        let _ = state.db.clear_messages(user_id);
                        for msg in &filtered {
                            let _ = state.db.add_message(user_id, msg);
                        }
                        tracing::info!(user_id = %user_id, kept = filtered.len(), "Compaction: kept recent messages, deleted older ones");
                    }
                }
            }
        } else {
            let msg_count = state.db.count_messages(user_id).unwrap_or(0);
            let (_all_messages, total_tokens) = state
                .db
                .get_messages_with_token_budget(user_id, usize::MAX)
                .unwrap_or((vec![], 0));
            tracing::info!(
                user_id = %user_id,
                msg_count = msg_count,
                total_tokens = total_tokens,
                compaction_enabled = false,
                "Compaction skipped (disabled)"
            );
        }

        if completed {
            break;
        }

        // If in last response mode, mark as completed after this response
        if last_response_mode {
            tracing::info!(user_id = %user_id, "Last response mode: completing after this response");
            completed = true;
            break;
        }

        // If no tool calls and not completed, we're done with this message
        break;
    }

    // Unregister this loop and clean up stop signal
    unregister_active_loop(user_id).await;
    STOP_SIGNALS.write().await.remove(user_id);

    // Find the last assistant message with actual content
    let all_msgs = state.db.get_messages(user_id, 50)?;
    let final_response = all_msgs
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.is_empty() && m.tool_calls.is_none())
        .map(|m| m.content.clone())
        .unwrap_or_else(|| {
            tracing::warn!(user_id = %user_id, "No assistant response found after agent loop");
            "I processed your request but have no text response to share.".to_string()
        });

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
pub async fn generate_compaction_summary(
    state: &GatewayState,
    user_id: &str,
    compaction_template: Option<&str>,
) -> anyhow::Result<String> {
    let (messages, _tokens) = state.db.get_messages_with_token_budget(user_id, 30000)?;

    let conversation_text = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let template_name = compaction_template.unwrap_or("compaction");
    let template_path = format!("templates/{}.poml", template_name);
    let prompt = if std::path::Path::new(&template_path).exists() {
        let tmpl_ctx = serde_json::json!({ "conversation_text": conversation_text });
        crate::gateway::poml::render(&template_path, &tmpl_ctx)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Compaction template render failed: {}, using fallback", e);
                format!(
                    "Summarize this conversation in 3-5 sentences. Reply with only the summary:\n\n{}",
                    conversation_text
                )
            })
    } else {
        tracing::warn!(
            "Compaction template '{}' not found, using fallback",
            template_path
        );
        format!(
            "Summarize this conversation in 3-5 sentences. Reply with only the summary:\n\n{}",
            conversation_text
        )
    };

    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some(prompt),
            content_parts: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }],
        tools: None,
        temperature: Some(0.3),
        max_tokens: Some(500),
        model: None,
        vision_provider: None,
        vision_model: None,
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
    tracing::info!(user_id = %ctx.user_id, "build_system_prompt called with user_message: {}", user_message);
    let mut context_json = serde_json::json!({
        "user_id": ctx.user_id,
        "turn": ctx.turn,
        "mode": ctx.mode,
        "system_info": format!("Praxis v{}", env!("CARGO_PKG_VERSION")),
        "user_message": user_message,
        "user_prompt": user_message,
        "time": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });
    
    tracing::info!(user_id = %ctx.user_id, "context_json user_prompt: {}", context_json["user_prompt"]);

    context_json["user_name"] = serde_json::json!(ctx.user_name.as_deref().unwrap_or("User"));

    let effective_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/".to_string());
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

    // Load tools for context (so system prompt can list them)
    let tools = crate::db::tools::to_tool_definitions(&_state.db).unwrap_or_default();
    tracing::debug!(target: "agent_loop", "Loaded {} tool definitions", tools.len());
    let tools_context: Vec<serde_json::Value> = tools.iter().map(|t| {
        serde_json::json!({
            "name": t.function.name,
            "description": t.function.description,
            "parameters": t.function.parameters
        })
    }).collect();
    tracing::debug!(target: "agent_loop", "Tools context: {}", serde_json::to_string(&tools_context).unwrap_or_default());
    context_json["tools"] = serde_json::json!(tools_context);

    context_json["custom_data"] = if ctx.custom_data.is_null() {
        serde_json::json!({})
    } else {
        ctx.custom_data.clone()
    };

    context_json["cl_data"] = if ctx.cl_data.is_null() {
        serde_json::json!({})
    } else {
        ctx.cl_data.clone()
    };

        let template_name = ctx.settings.system_template.as_deref().unwrap_or("system");
        let template_path = format!("templates/{}.poml", template_name);
        tracing::info!(user_id = %ctx.user_id, template_name = %template_name, template_path = %template_path, "Building system prompt");
        match crate::gateway::poml::render(&template_path, &context_json).await {
            Ok(rendered) => {
                tracing::info!(user_id = %ctx.user_id, "Rendered system prompt (first 500 chars): {}", &rendered[..rendered.len().min(500)]);
                rendered
            },
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

async fn execute_tool_call(
    db: &crate::db::Database,
    user_id: &str,
    tc: &ToolCall,
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
            // If VM_MODE=vm, redirect to VM
            let vm_mode = std::env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string());
            let vm_enabled = std::env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if vm_enabled && vm_mode == "vm" {
                let command = args["command"].as_str().unwrap_or("");
                match crate::tools::vm_tools::dispatch_vm_tool(
                    "vm_shell",
                    &serde_json::json!({"command": command}),
                )
                .await
                {
                    Some(result) => result,
                    None => "Error: VM not running. Start a VM first with vm_start.".to_string(),
                }
            } else {
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
            } // end else (shared mode)
        }
        "write_file" => {
            let vm_mode = std::env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string());
            let vm_enabled = std::env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if vm_enabled && vm_mode == "vm" {
                let path = args["path"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                match crate::tools::vm_tools::dispatch_vm_tool(
                    "vm_file_transfer",
                    &serde_json::json!({"path": path, "content": content, "direction": "to_vm"}),
                )
                .await
                {
                    Some(result) => result,
                    None => "Error: VM not running. Start a VM first with vm_start.".to_string(),
                }
            } else {
                let path = args["path"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                match crate::tools::write_file::write_file(path, content).await {
                    Ok(_) => format!("File written: {}", path),
                    Err(e) => format!("Error: {}", e),
                }
            } // end else (shared mode)
        }
        "edit_file" => {
            let vm_mode = std::env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string());
            let vm_enabled = std::env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if vm_enabled && vm_mode == "vm" {
                let path = args["path"].as_str().unwrap_or("");
                let old_text = args["old_text"]
                    .as_str()
                    .or_else(|| args["old_string"].as_str())
                    .unwrap_or("");
                let new_text = args["new_text"]
                    .as_str()
                    .or_else(|| args["new_string"].as_str())
                    .unwrap_or("");
                let cmd = format!(
                    "sed -i 's/{}/{}/g' {}",
                    old_text.replace('/', "\\/"),
                    new_text.replace('/', "\\/"),
                    path
                );
                match crate::tools::vm_tools::dispatch_vm_tool(
                    "vm_shell",
                    &serde_json::json!({"command": cmd}),
                )
                .await
                {
                    Some(result) => result,
                    None => "Error: VM not running. Start a VM first with vm_start.".to_string(),
                }
            } else {
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
            } // end else (shared mode)
        }
        "read_file" => {
            let vm_mode = std::env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string());
            let vm_enabled = std::env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if vm_enabled && vm_mode == "vm" {
                let path = args["path"].as_str().unwrap_or("");
                let cmd = format!("cat {}", path);
                match crate::tools::vm_tools::dispatch_vm_tool(
                    "vm_shell",
                    &serde_json::json!({"command": cmd}),
                )
                .await
                {
                    Some(result) => result,
                    None => "Error: VM not running. Start a VM first with vm_start.".to_string(),
                }
            } else {
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
            } // end else (shared mode)
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
            // Decode base64 to temp file, then upload
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
            // Store image content_parts alongside the result
            // We return the text, but the caller needs to handle content_parts separately
            // Use a special marker to indicate this tool returned image data
            return serde_json::json!({
                "text": result.text,
                "content_parts": result.content_parts.iter().map(|cp| {
                    serde_json::to_value(cp).unwrap_or_default()
                }).collect::<Vec<_>>()
            })
            .to_string();
        }
        "send_screenshot_to_discord" => {
            let fallback_ch = ctx_data
                .as_ref()
                .and_then(|c| c.get("channel_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let channel_id = args["channel_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_ch);
            let caption = args["caption"].as_str();
            let vm_name = args["vm_name"].as_str().unwrap_or("praxis-vm");
            match crate::tools::discord_interactive::send_screenshot_to_discord(
                channel_id, caption, vm_name,
            )
            .await
            {
                Ok(result) => result,
                Err(e) => format!("Error: {}", e),
            }
        }
        "screenshot_with_feedback" => {
            let channel_id = args["channel_id"].as_str().unwrap_or("");
            let feedback = args["feedback"].as_str().unwrap_or("");
            let vm_name = args["vm_name"].as_str().unwrap_or("praxis-vm");
            match crate::tools::discord_interactive::screenshot_with_feedback(
                channel_id, feedback, vm_name,
            )
            .await
            {
                Ok(result) => result,
                Err(e) => format!("Error: {}", e),
            }
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
            let questions_raw = args["questions"].as_array();
            let mut questions: Vec<(String, String, Vec<String>)> = Vec::new();
            let mut format_error = None;
            if let Some(arr) = questions_raw {
                for q in arr {
                    // Check for invalid 'options' field
                    if q.get("options").is_some() {
                        format_error = Some("Invalid format: use 'suggestions' with plain strings, not 'options' with objects. Example: \"suggestions\": [\"yes\", \"no\", \"maybe\"]".to_string());
                        break;
                    }
                    let label = q["label"].as_str().unwrap_or("").to_string();
                    let text = q["question"].as_str().unwrap_or("").to_string();
                    let suggestions: Vec<String> = q["suggestions"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    questions.push((label, text, suggestions));
                }
            }
            if let Some(error) = format_error {
                error
            } else {
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
        }
        _ => {
            // Check VM tools first
            if tc.function.name.starts_with("vm_") {
                if let Some(result) =
                    crate::tools::vm_tools::dispatch_vm_tool(&tc.function.name, &args).await
                {
                    return result;
                }
            }
            match plugins
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
            }
        }
    }
}

/// Validate tool parameters against the tool definition
pub fn validate_tool_params(
    tool_name: &str,
    args: &serde_json::Value,
    tool_defs: &[crate::gateway::llm::provider::ToolDefinition],
) -> Result<(), String> {
    let tool_def = tool_defs.iter().find(|t| t.function.name == tool_name);
    if let Some(def) = tool_def {
        let params = &def.function.parameters;
        if let Some(required) = params.get("required").and_then(|r| r.as_array()) {
            let mut missing = Vec::new();
            for req in required {
                if let Some(key) = req.as_str() {
                    if args.get(key).is_none() || args.get(key).unwrap().is_null() {
                        missing.push(key.to_string());
                    }
                }
            }
            if !missing.is_empty() {
                return Err(format!(
                    "Missing required parameters: {}. Provide these in the arguments.",
                    missing.join(", ")
                ));
            }
        }
    }
    Ok(())
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
