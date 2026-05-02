use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::Json;
use axum::Router;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct DashboardState {
    pub db: crate::db::Database,
    pub gateway_api_key: String,
    pub admin_password: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
}

#[derive(Deserialize)]
pub struct ToolUpdate {
    pub is_enabled: bool,
}

#[derive(Deserialize)]
pub struct MemoryUpdate {
    pub custom_variables: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub learned_facts: Option<Vec<String>>,
    pub last_topics: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct MemoryInfo {
    pub user_id: String,
    pub custom_variables: std::collections::HashMap<String, serde_json::Value>,
    pub learned_facts: Vec<String>,
    pub last_topics: Vec<String>,
}

#[derive(Deserialize)]
pub struct TemplateUpdate {
    pub content: String,
    pub user_id: Option<String>,
}

#[derive(Deserialize)]
pub struct TemplateCreate {
    pub name: String,
    pub content: String,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct TemplateSaveResult {
    pub success: bool,
    pub error: Option<String>,
    pub rendered_preview: Option<String>,
}

#[derive(Serialize)]
pub struct SecretsInfo {
    pub discord_bot_token: String,
    pub openai_api_key: String,
    pub anthropic_api_key: String,
    pub minimax_api_key: String,
    pub mimo_api_key: String,
    pub elevenlabs_api_key: String,
    pub gateway_api_key: String,
    pub dashboard_admin_password: String,
    #[serde(flatten)]
    pub custom: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct SecretsUpdate {
    pub discord_bot_token: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub minimax_api_key: Option<String>,
    pub mimo_api_key: Option<String>,
    pub elevenlabs_api_key: Option<String>,
    pub gateway_api_key: Option<String>,
    pub dashboard_admin_password: Option<String>,
    pub master_password: Option<String>,
    #[serde(flatten)]
    pub custom: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct ClFileInfo {
    pub name: String,
    pub path: String,
}

#[derive(Deserialize)]
pub struct ClFileUpdate {
    pub content: String,
}

#[derive(Deserialize)]
pub struct DashboardLoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct DashboardLoginResponse {
    pub token: String,
}

pub fn sync_templates_from_disk(db: &crate::db::Database) {
    let templates_dir = std::path::Path::new("templates");
    if !templates_dir.exists() {
        return;
    }
    sync_templates_dir(db, templates_dir, "");
}

fn sync_templates_dir(db: &crate::db::Database, dir: &std::path::Path, prefix: &str) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_prefix = if prefix.is_empty() {
                    entry.file_name().to_string_lossy().to_string()
                } else {
                    format!("{}/{}", prefix, entry.file_name().to_string_lossy())
                };
                sync_templates_dir(db, &path, &sub_prefix);
            } else if path.extension().and_then(|e| e.to_str()) == Some("poml") {
                let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let name = if prefix.is_empty() {
                    file_stem.to_string()
                } else {
                    format!("{}/{}", prefix, file_stem)
                };
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let existing = db.get_template(&name).ok().flatten();
                    let needs_sync = match &existing {
                        None => true,
                        Some(t) => t.content != content,
                    };
                    if needs_sync {
                        let _ = db.save_template(&name, &content, None, true);
                        tracing::info!("Synced template from disk: {}", name);
                    }
                }
            }
        }
    }
}

pub fn routes(db: crate::db::Database) -> Router {
    let secrets = crate::db::secrets::get_secrets();
    let config = crate::config::Config::from_env();
    let state = Arc::new(DashboardState {
        db,
        gateway_api_key: secrets.gateway_api_key.unwrap_or(config.gateway_api_key),
        admin_password: secrets
            .dashboard_admin_password
            .unwrap_or(config.dashboard_admin_password),
    });

    // Protected API routes with auth middleware
    let protected = Router::new()
        .route("/contexts", axum::routing::get(list_contexts))
        .route("/contexts/:user_id", axum::routing::get(get_context))
        .route("/contexts/:user_id", axum::routing::put(update_context))
        .route("/messages/:user_id", axum::routing::get(get_messages))
        .route("/templates", axum::routing::get(list_templates))
        .route("/templates", axum::routing::post(create_template))
        .route("/templates/:name", axum::routing::get(get_template))
        .route("/templates/:name", axum::routing::put(update_template))
        .route("/templates/:name", axum::routing::delete(delete_template))
        .route("/tools", axum::routing::get(list_tools))
        .route("/tools/:name", axum::routing::put(update_tool))
        .route("/memory/:user_id", axum::routing::get(get_memory))
        .route("/memory/:user_id", axum::routing::put(update_memory))
        .route("/secrets", axum::routing::get(get_secrets))
        .route("/secrets", axum::routing::put(update_secrets))
        .route("/pairings", axum::routing::get(list_pairings))
        .route("/pairings/:user_id", axum::routing::delete(delete_pairing))
        .route(
            "/pairings/pending",
            axum::routing::get(list_pending_pairings),
        )
        .route(
            "/pairings/pending/:code/approve",
            axum::routing::post(approve_pending_pairing),
        )
        .route(
            "/pairings/pending/:code",
            axum::routing::delete(delete_pending_pairing),
        )
        .route("/cl-files", axum::routing::get(list_cl_files))
        .route("/cl-files/:name", axum::routing::get(get_cl_file))
        .route("/cl-files/:name", axum::routing::put(save_cl_file))
        .route("/cron-jobs", axum::routing::get(list_cron_jobs))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            dashboard_auth_middleware,
        ));

    // Static file service
    let static_service = tower_http::services::ServeDir::new("static");

    Router::new()
        .route("/", axum::routing::get(index))
        .route("/logo.svg", axum::routing::get(logo_svg))
        .route("/api/status", axum::routing::get(status))
        .route("/api/auth/login", axum::routing::post(login_handler))
        .nest_service("/static", static_service)
        .nest("/api", protected)
        .with_state(state.clone())
}

async fn dashboard_auth_middleware(
    State(state): State<Arc<DashboardState>>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) => match header.strip_prefix("Bearer ") {
            Some(t) => t,
            None => return Err(StatusCode::UNAUTHORIZED),
        },
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Try JWT first
    use jsonwebtoken::{decode, DecodingKey, Validation};
    let jwt_result = decode::<crate::gateway::auth::Claims>(
        token,
        &DecodingKey::from_secret(state.gateway_api_key.as_bytes()),
        &Validation::default(),
    );

    if jwt_result.is_ok() {
        return Ok(next.run(req).await);
    }

    // Fall back to raw API key
    if token == state.gateway_api_key {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

async fn login_handler(
    State(state): State<Arc<DashboardState>>,
    Json(payload): Json<DashboardLoginRequest>,
) -> Result<Json<DashboardLoginResponse>, StatusCode> {
    if payload.password != state.admin_password {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let claims = crate::gateway::auth::Claims {
        sub: "admin".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };

    use jsonwebtoken::{encode, EncodingKey, Header};
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.gateway_api_key.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(DashboardLoginResponse { token }))
}

async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../static/index.html"))
}

async fn logo_svg() -> impl axum::response::IntoResponse {
    (
        axum::http::header::HeaderMap::from_iter([(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml".parse().unwrap(),
        )]),
        include_str!("../../static/logo.svg"),
    )
}

async fn status(State(_state): State<Arc<DashboardState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ── Contexts ─────────────────────────────────────────────────────────────────

async fn list_contexts(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.conn();
    let mut stmt = conn
        .prepare("SELECT user_id, data, updated_at FROM contexts ORDER BY updated_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let contexts: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            let user_id: String = row.get(0)?;
            let data: String = row.get(1)?;
            let updated_at: String = row.get(2)?;
            Ok(serde_json::json!({
                "user_id": user_id,
                "data": serde_json::from_str::<serde_json::Value>(&data).unwrap_or_default(),
                "updated_at": updated_at,
            }))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();

    Ok(Json(serde_json::json!({ "contexts": contexts })))
}

async fn get_context(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ctx = state
        .db
        .load_context(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "user_id": ctx.user_id,
        "turn": ctx.turn,
        "mode": ctx.mode,
        "user_name": ctx.user_name,
        "cl_file": ctx.cl_file,
        "active_state": ctx.active_state,
        "active_templates": ctx.active_templates,
        "settings": ctx.settings,
        "custom_data": ctx.custom_data,
    })))
}

async fn update_context(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
    Json(update): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ctx = state
        .db
        .merge_context(&user_id, update)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "user_id": ctx.user_id,
        "turn": ctx.turn,
        "mode": ctx.mode,
        "user_name": ctx.user_name,
        "cl_file": ctx.cl_file,
        "active_state": ctx.active_state,
        "active_templates": ctx.active_templates,
        "settings": ctx.settings,
        "custom_data": ctx.custom_data,
    })))
}

// ── Messages ─────────────────────────────────────────────────────────────────

async fn get_messages(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let budget = 500000usize;
    match state.db.get_messages_with_token_budget(&user_id, budget) {
        Ok((messages, total_tokens)) => {
            let msgs: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    let mut val = serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                        "tool_call_id": m.tool_call_id,
                    });
                    if let Some(ref tool_calls) = m.tool_calls {
                        val["tool_calls"] = serde_json::json!(tool_calls
                            .iter()
                            .map(|tc| {
                                serde_json::json!({
                                    "id": tc.id,
                                    "name": tc.function.name,
                                    "arguments": tc.function.arguments,
                                })
                            })
                            .collect::<Vec<_>>());
                    }
                    val
                })
                .collect();
            Ok(Json(serde_json::json!({
                "messages": msgs,
                "total_tokens": total_tokens,
                "message_count": messages.len(),
            })))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// ── Templates ────────────────────────────────────────────────────────────────

async fn list_templates(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.db.list_templates() {
        Ok(templates) => {
            let tpls: Vec<serde_json::Value> = templates
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "is_system": t.is_system,
                        "updated_at": t.updated_at,
                    })
                })
                .collect();
            Ok(Json(serde_json::json!({ "templates": tpls })))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_template(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let templates = state
        .db
        .list_templates()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let template = templates
        .into_iter()
        .find(|t| t.name == name)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "name": template.name,
        "content": template.content,
        "description": template.description,
    })))
}

async fn update_template(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
    Json(update): Json<TemplateUpdate>,
) -> Result<Json<TemplateSaveResult>, StatusCode> {
    // Write template to file so POML can render it
    let file_path = format!("templates/{}.poml", name);
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&file_path, &update.content) {
        return Ok(Json(TemplateSaveResult {
            success: false,
            error: Some(format!("Failed to write template file: {}", e)),
            rendered_preview: None,
        }));
    }

    // Save to DB
    let _ = state.db.save_template(&name, &update.content, None, false);

    // Load user context for preview
    let (user_id, ctx) = if let Some(uid) = update.user_id.filter(|s| !s.is_empty()) {
        let ctx = state.db.load_context(&uid).unwrap_or_default();
        (uid, ctx)
    } else {
        (String::new(), crate::db::contexts::Context::default())
    };

    let mut skills_registry = crate::skills::SkillRegistry::new();
    let _ = skills_registry.load_from_dir(std::path::Path::new("skills"));
    let memory = crate::db::memory::load_memory(&state.db, &user_id);

    let token_budget = ctx.settings.history_token_limit.unwrap_or(500000);
    let compaction_limit = ctx.settings.compaction_token_limit.unwrap_or(500000);
    let (messages, tokens_used) = state
        .db
        .get_messages_with_token_budget(&user_id, usize::MAX)
        .unwrap_or((vec![], 0));
    let message_count = messages.len();
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

    let effective_path = if ctx.settings.path.is_empty() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string())
    } else {
        ctx.settings.path.clone()
    };

    let context = serde_json::json!({
        "user_id": ctx.user_id,
        "user_name": ctx.user_name.as_deref().unwrap_or("User"),
        "mode": ctx.mode,
        "turn": ctx.turn,
        "system_info": format!("Praxis v{}", env!("CARGO_PKG_VERSION")),
        "skills": skills_registry.to_context_array(),
        "uptime": "0m",
        "uptime_secs": 0,
        "paired_users_count": 0,
        "paired_users": [],
        "path": effective_path,
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
        "user_message": if user_id.is_empty() {
            "Preview message".to_string()
        } else {
            state.db.get_messages(&user_id, 1).ok()
                .and_then(|msgs| msgs.first().map(|m| m.content.clone()))
                .unwrap_or_else(|| "Preview message".to_string())
        },
        "user_prompt": if user_id.is_empty() {
            "Preview message".to_string()
        } else {
            state.db.get_messages(&user_id, 1).ok()
                .and_then(|msgs| msgs.first().map(|m| m.content.clone()))
                .unwrap_or_else(|| "Preview message".to_string())
        },
        "user_template": ctx.custom_data.get("user_template").cloned().unwrap_or(serde_json::json!("user")),
        "conversation_text": "user: Preview message",
        "tokens_used": tokens_used,
        "tokens_limit": token_budget,
        "tokens_percentage": format!("{:.1}", tokens_pct),
        "compaction_token_limit": compaction_limit,
        "compaction_percentage": format!("{:.1}", compaction_pct),
        "message_count": message_count,
    });

    match crate::gateway::poml::render(&file_path, &context).await {
        Ok(rendered) => Ok(Json(TemplateSaveResult {
            success: true,
            error: None,
            rendered_preview: Some(rendered.chars().take(2000).collect()),
        })),
        Err(e) => Ok(Json(TemplateSaveResult {
            success: true,
            error: Some(format!("Template saved but preview failed: {}", e)),
            rendered_preview: None,
        })),
    }
}

async fn create_template(
    State(state): State<Arc<DashboardState>>,
    Json(create): Json<TemplateCreate>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let file_path = format!("templates/{}.poml", create.name);
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&file_path, &create.content) {
        return Ok(Json(serde_json::json!({
            "success": false,
            "error": format!("Failed to write template file: {}", e)
        })));
    }
    let _ = state.db.save_template(
        &create.name,
        &create.content,
        create.description.as_deref(),
        false,
    );
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn delete_template(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.conn();
    let _ = conn.execute(
        "DELETE FROM templates WHERE name = ?1",
        rusqlite::params![name],
    );
    let file_path = format!("templates/{}.poml", name);
    let _ = std::fs::remove_file(&file_path);
    Ok(Json(serde_json::json!({ "success": true })))
}

// ── Tools ────────────────────────────────────────────────────────────────────

async fn list_tools(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tools = crate::db::tools::list(&state.db).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "tools": tools })))
}

async fn update_tool(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
    Json(update): Json<ToolUpdate>,
) -> Result<String, StatusCode> {
    crate::db::tools::set_enabled(&state.db, &name, update.is_enabled)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Tool updated".to_string())
}

// ── Memory ───────────────────────────────────────────────────────────────────

async fn get_memory(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
) -> Result<Json<MemoryInfo>, StatusCode> {
    let memory = crate::db::memory::load_memory(&state.db, &user_id);
    Ok(Json(MemoryInfo {
        user_id,
        custom_variables: memory.custom_variables,
        learned_facts: memory.learned_facts,
        last_topics: memory.last_topics,
    }))
}

async fn update_memory(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
    Json(update): Json<MemoryUpdate>,
) -> Result<String, StatusCode> {
    let mut memory = crate::db::memory::load_memory(&state.db, &user_id);

    if let Some(vars) = update.custom_variables {
        memory.custom_variables = vars;
    }
    if let Some(facts) = update.learned_facts {
        memory.learned_facts = facts;
    }
    if let Some(topics) = update.last_topics {
        memory.last_topics = topics;
    }

    crate::db::memory::save_memory(&state.db, &user_id, &memory)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Memory updated".to_string())
}

// ── Secrets ──────────────────────────────────────────────────────────────────

async fn get_secrets() -> Result<Json<SecretsInfo>, StatusCode> {
    let secrets = crate::db::secrets::get_secrets();
    let custom_masked: std::collections::HashMap<String, String> = secrets
        .custom
        .iter()
        .map(|(k, v)| (k.clone(), crate::db::secrets::mask_secret(&Some(v.clone()))))
        .collect();
    Ok(Json(SecretsInfo {
        discord_bot_token: crate::db::secrets::mask_secret(&secrets.discord_bot_token),
        openai_api_key: crate::db::secrets::mask_secret(&secrets.openai_api_key),
        anthropic_api_key: crate::db::secrets::mask_secret(&secrets.anthropic_api_key),
        minimax_api_key: crate::db::secrets::mask_secret(&secrets.minimax_api_key),
        mimo_api_key: crate::db::secrets::mask_secret(&secrets.mimo_api_key),
        elevenlabs_api_key: crate::db::secrets::mask_secret(&secrets.elevenlabs_api_key),
        gateway_api_key: crate::db::secrets::mask_secret(&secrets.gateway_api_key),
        dashboard_admin_password: crate::db::secrets::mask_secret(
            &secrets.dashboard_admin_password,
        ),
        custom: custom_masked,
    }))
}

async fn update_secrets(Json(update): Json<SecretsUpdate>) -> Result<String, StatusCode> {
    let mut secrets = crate::db::secrets::get_secrets();

    if let Some(v) = update.discord_bot_token {
        secrets.discord_bot_token = Some(v);
    }
    if let Some(v) = update.openai_api_key {
        secrets.openai_api_key = Some(v);
    }
    if let Some(v) = update.anthropic_api_key {
        secrets.anthropic_api_key = Some(v);
    }
    if let Some(v) = update.minimax_api_key {
        secrets.minimax_api_key = Some(v);
    }
    if let Some(v) = update.mimo_api_key {
        secrets.mimo_api_key = Some(v);
    }
    if let Some(v) = update.elevenlabs_api_key {
        secrets.elevenlabs_api_key = Some(v);
    }
    if let Some(v) = update.gateway_api_key {
        secrets.gateway_api_key = Some(v);
    }
    if let Some(v) = update.dashboard_admin_password {
        secrets.dashboard_admin_password = Some(v);
    }

    for (k, v) in update.custom {
        if v.is_empty() {
            secrets.custom.remove(&k);
        } else {
            secrets.custom.insert(k, v);
        }
    }

    // Persist to enc2 if master password provided
    if let Some(ref password) = update.master_password {
        if let Err(_e) = crate::db::secrets::save_secrets(&secrets, password) {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        crate::db::secrets::init_secrets(secrets);
        Ok("Secrets saved and encrypted.".to_string())
    } else {
        crate::db::secrets::init_secrets(secrets);
        Ok("Secrets updated in memory. Provide master_password to persist to disk.".to_string())
    }
}

// ── Pairings ─────────────────────────────────────────────────────────────────

async fn list_pairings(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.conn();
    let mut stmt = conn
        .prepare("SELECT user_id, discord_user_id, discord_guild_id, paired_at, last_seen_at FROM pairings ORDER BY paired_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let pairings: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": row.get::<_, String>(0)?,
                "discord_user_id": row.get::<_, String>(1)?,
                "discord_guild_id": row.get::<_, Option<String>>(2)?,
                "paired_at": row.get::<_, Option<String>>(3)?,
                "last_seen_at": row.get::<_, Option<String>>(4)?,
            }))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();

    Ok(Json(serde_json::json!({ "pairings": pairings })))
}

async fn delete_pairing(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
) -> Result<String, StatusCode> {
    let conn = state.db.conn();
    conn.execute(
        "DELETE FROM pairings WHERE user_id = ?1",
        rusqlite::params![user_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Pairing deleted".to_string())
}

async fn list_pending_pairings(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.conn();
    let mut stmt = conn
        .prepare("SELECT code, discord_user_id, expires_at, created_at FROM pending_pairings ORDER BY created_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let pending: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "code": row.get::<_, String>(0)?,
                "discord_user_id": row.get::<_, String>(1)?,
                "expires_at": row.get::<_, String>(2)?,
                "created_at": row.get::<_, Option<String>>(3)?,
            }))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();

    Ok(Json(serde_json::json!({ "pending_pairings": pending })))
}

async fn approve_pending_pairing(
    State(state): State<Arc<DashboardState>>,
    Path(code): Path<String>,
) -> Result<String, StatusCode> {
    let pending = state
        .db
        .get_pending_pairing(&code)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let pending = match pending {
        Some(p) => p,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let user_id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .create_pairing(&user_id, &pending.discord_user_id, None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    state
        .db
        .delete_pending_pairing(&code)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(format!(
        "Pairing approved for Discord user {}",
        pending.discord_user_id
    ))
}

async fn delete_pending_pairing(
    State(state): State<Arc<DashboardState>>,
    Path(code): Path<String>,
) -> Result<String, StatusCode> {
    state
        .db
        .delete_pending_pairing(&code)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Pending pairing deleted".to_string())
}

// ── CL Files ─────────────────────────────────────────────────────────────────

async fn list_cl_files(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cl_dir = std::path::Path::new("contexts");
    let data_cl_dir = std::path::PathBuf::from(&state.db.data_dir).join("contexts");
    let mut files = Vec::new();

    for dir in [&cl_dir, &data_cl_dir.as_path()] {
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("cl") {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if !files
                            .iter()
                            .any(|f: &serde_json::Value| f["name"].as_str() == Some(&name))
                        {
                            files.push(serde_json::json!({
                                "name": name,
                                "path": path.to_string_lossy(),
                            }));
                        }
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(Json(serde_json::json!({ "cl_files": files })))
}

async fn get_cl_file(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
) -> Result<String, StatusCode> {
    let path = std::path::Path::new("contexts").join(&name);
    if path.exists() {
        return std::fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND);
    }
    let data_path = std::path::PathBuf::from(&state.db.data_dir)
        .join("contexts")
        .join(&name);
    std::fs::read_to_string(&data_path).map_err(|_| StatusCode::NOT_FOUND)
}

async fn save_cl_file(
    State(state): State<Arc<DashboardState>>,
    Path(name): Path<String>,
    Json(update): Json<ClFileUpdate>,
) -> Result<String, StatusCode> {
    let cl_dir = std::path::PathBuf::from(&state.db.data_dir).join("contexts");
    std::fs::create_dir_all(&cl_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = cl_dir.join(&name);

    if let Err(e) = crate::cl::parse(&update.content) {
        return Ok(format!("CL Error: {}", e));
    }

    std::fs::write(&path, &update.content).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("CL file saved".to_string())
}

// ── Cron Jobs ────────────────────────────────────────────────────────────────

async fn list_cron_jobs(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, schedule, enabled, last_run, run_count FROM cron_jobs ORDER BY name",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let jobs: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "schedule": row.get::<_, String>(2)?,
                "enabled": row.get::<_, i32>(3)? != 0,
                "last_run": row.get::<_, Option<String>>(4)?,
                "run_count": row.get::<_, i64>(5)?,
            }))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();

    Ok(Json(serde_json::json!({ "cron_jobs": jobs })))
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_setup() -> (crate::db::Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = crate::db::Database::new(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn test_routes_compiles() {
        assert!(true);
    }

    #[test]
    fn test_dashboard_state_clone() {
        let (db, _dir) = test_setup();
        let state = DashboardState {
            db,
            gateway_api_key: "test-api-key-12345678".to_string(),
            admin_password: "testpassword".to_string(),
        };
        let _cloned = state.clone();
    }

    #[test]
    fn test_tool_update_deserialize() {
        let json = r#"{"is_enabled": true}"#;
        let update: ToolUpdate = serde_json::from_str(json).unwrap();
        assert!(update.is_enabled);
    }

    #[test]
    fn test_memory_update_deserialize() {
        let json = r#"{"learned_facts": ["fact1", "fact2"]}"#;
        let update: MemoryUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.learned_facts.unwrap().len(), 2);
    }

    #[test]
    fn test_secrets_update_deserialize() {
        let json = r#"{"discord_bot_token": "new_token"}"#;
        let update: SecretsUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.discord_bot_token, Some("new_token".to_string()));
    }

    #[test]
    fn test_template_update_deserialize() {
        let json = r#"{"content": "Hello {{name}}"}"#;
        let update: TemplateUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.content, "Hello {{name}}");
    }

    #[test]
    fn test_cl_file_update_deserialize() {
        let json = r#"{"content": "[state test]\nmode = chat"}"#;
        let update: ClFileUpdate = serde_json::from_str(json).unwrap();
        assert!(update.content.contains("state test"));
    }
}
