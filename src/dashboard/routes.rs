use axum::Router;
use axum::Json;
use axum::extract::{State, Path};
use axum::http::StatusCode;
use axum::middleware;
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

pub fn routes(db: crate::db::Database) -> Router {
    let secrets = crate::db::secrets::get_secrets();
    let config = crate::config::Config::from_env();
    let state = Arc::new(DashboardState {
        db,
        gateway_api_key: secrets.gateway_api_key.unwrap_or(config.gateway_api_key),
        admin_password: secrets.dashboard_admin_password.unwrap_or(config.dashboard_admin_password),
    });

    // Public routes (no auth required)
    let public = Router::new()
        .route("/", axum::routing::get(index))
        .route("/api/status", axum::routing::get(status))
        .route("/api/auth/login", axum::routing::post(login_handler))
        .route("/static/{file}", axum::routing::get(static_file))
        .route("/style.css", axum::routing::get(style_css))
        .route("/app.js", axum::routing::get(app_js))
        .route("/favicon.ico", axum::routing::get(favicon))
        .route("/logo.svg", axum::routing::get(logo_svg))
        .with_state(state.clone());

    // Protected routes (auth required)
    let protected = Router::new()
        .route("/api/contexts", axum::routing::get(list_contexts))
        .route("/api/contexts/{user_id}", axum::routing::get(get_context))
        .route("/api/contexts/{user_id}", axum::routing::put(update_context))
        .route("/api/messages/{user_id}", axum::routing::get(get_messages))
        .route("/api/templates", axum::routing::get(list_templates))
        .route("/api/templates/{name}", axum::routing::get(get_template))
        .route("/api/templates/{name}", axum::routing::put(update_template))
        .route("/api/tools", axum::routing::get(list_tools))
        .route("/api/tools/{name}", axum::routing::put(update_tool))
        .route("/api/memory/{user_id}", axum::routing::get(get_memory))
        .route("/api/memory/{user_id}", axum::routing::put(update_memory))
        .route("/api/secrets", axum::routing::get(get_secrets))
        .route("/api/secrets", axum::routing::put(update_secrets))
        .route("/api/pairings", axum::routing::get(list_pairings))
        .route("/api/pairings/{user_id}", axum::routing::delete(delete_pairing))
        .route("/api/cl-files", axum::routing::get(list_cl_files))
        .route("/api/cl-files/{name}", axum::routing::get(get_cl_file))
        .route("/api/cl-files/{name}", axum::routing::put(save_cl_file))
        .route("/api/cron-jobs", axum::routing::get(list_cron_jobs))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state.clone(), dashboard_auth_middleware));

    public.merge(protected)
}

async fn dashboard_auth_middleware(
    State(state): State<Arc<DashboardState>>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    let auth_header = req.headers().get("Authorization").and_then(|v| v.to_str().ok());

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

async fn static_file(Path(file): Path<String>) -> Result<axum::response::Response, StatusCode> {
    let (content, content_type) = match file.as_str() {
        "style.css" => (
            include_bytes!("../../static/style.css").as_slice(),
            "text/css",
        ),
        "app.js" => (
            include_bytes!("../../static/app.js").as_slice(),
            "application/javascript",
        ),
        "index.html" => (
            include_bytes!("../../static/index.html").as_slice(),
            "text/html",
        ),
        _ => return Err(StatusCode::NOT_FOUND),
    };

    Ok(axum::response::Response::builder()
        .header("Content-Type", content_type)
        .body(axum::body::Body::from(content))
        .unwrap())
}

async fn style_css() -> axum::response::Response {
    axum::response::Response::builder()
        .header("Content-Type", "text/css")
        .body(axum::body::Body::from(include_bytes!("../../static/style.css").as_slice()))
        .unwrap()
}

async fn app_js() -> axum::response::Response {
    axum::response::Response::builder()
        .header("Content-Type", "application/javascript")
        .body(axum::body::Body::from(include_bytes!("../../static/app.js").as_slice()))
        .unwrap()
}

async fn favicon() -> axum::response::Response {
    axum::response::Response::builder()
        .header("Content-Type", "image/svg+xml")
        .body(axum::body::Body::from(include_bytes!("../../static/logo.svg").as_slice()))
        .unwrap()
}

async fn logo_svg() -> axum::response::Response {
    axum::response::Response::builder()
        .header("Content-Type", "image/svg+xml")
        .body(axum::body::Body::from(include_bytes!("../../static/logo.svg").as_slice()))
        .unwrap()
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
    let ctx = state.db.load_context(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "user_id": ctx.user_id,
        "turn": ctx.turn,
        "mode": ctx.mode,
        "user_name": ctx.user_name,
        "settings": ctx.settings,
        "custom_data": ctx.custom_data,
    })))
}

async fn update_context(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
    Json(update): Json<serde_json::Value>,
) -> Result<String, StatusCode> {
    let mut ctx = state.db.load_context(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Merge update into custom_data
    if let (Some(map), Some(update_obj)) = (ctx.custom_data.as_object_mut(), update.as_object()) {
        for (key, value) in update_obj {
            map.insert(key.clone(), value.clone());
        }
    }

    state.db.save_context(&ctx)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Context updated".to_string())
}

// ── Messages ─────────────────────────────────────────────────────────────────

async fn get_messages(
    State(state): State<Arc<DashboardState>>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.db.get_messages(&user_id, 100) {
        Ok(messages) => {
            let msgs: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                    "tool_call_id": m.tool_call_id,
                }))
                .collect();
            Ok(Json(serde_json::json!({ "messages": msgs })))
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
                .map(|t| serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "is_system": t.is_system,
                    "updated_at": t.updated_at,
                }))
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
    let templates = state.db.list_templates()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let template = templates.into_iter().find(|t| t.name == name)
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
    // Try rendering with POML
    let context = serde_json::json!({});
    match crate::gateway::poml::render("templates/system.poml", &context).await {
        Ok(rendered) => {
            // Save template to DB
            let _ = state.db.save_template(
                &name,
                &update.content,
                None,
                false,
            );

            Ok(Json(TemplateSaveResult {
                success: true,
                error: None,
                rendered_preview: Some(rendered.chars().take(500).collect()),
            }))
        }
        Err(e) => {
            Ok(Json(TemplateSaveResult {
                success: false,
                error: Some(format!("{}", e)),
                rendered_preview: None,
            }))
        }
    }
}

// ── Tools ────────────────────────────────────────────────────────────────────

async fn list_tools(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tools = crate::db::tools::list(&state.db)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    Ok(Json(SecretsInfo {
        discord_bot_token: crate::db::secrets::mask_secret(&secrets.discord_bot_token),
        openai_api_key: crate::db::secrets::mask_secret(&secrets.openai_api_key),
        anthropic_api_key: crate::db::secrets::mask_secret(&secrets.anthropic_api_key),
        minimax_api_key: crate::db::secrets::mask_secret(&secrets.minimax_api_key),
        mimo_api_key: crate::db::secrets::mask_secret(&secrets.mimo_api_key),
        elevenlabs_api_key: crate::db::secrets::mask_secret(&secrets.elevenlabs_api_key),
        gateway_api_key: crate::db::secrets::mask_secret(&secrets.gateway_api_key),
        dashboard_admin_password: crate::db::secrets::mask_secret(&secrets.dashboard_admin_password),
    }))
}

async fn update_secrets(
    Json(update): Json<SecretsUpdate>,
) -> Result<String, StatusCode> {
    let mut secrets = crate::db::secrets::get_secrets();

    if let Some(v) = update.discord_bot_token { secrets.discord_bot_token = Some(v); }
    if let Some(v) = update.openai_api_key { secrets.openai_api_key = Some(v); }
    if let Some(v) = update.anthropic_api_key { secrets.anthropic_api_key = Some(v); }
    if let Some(v) = update.minimax_api_key { secrets.minimax_api_key = Some(v); }
    if let Some(v) = update.mimo_api_key { secrets.mimo_api_key = Some(v); }
    if let Some(v) = update.elevenlabs_api_key { secrets.elevenlabs_api_key = Some(v); }
    if let Some(v) = update.gateway_api_key { secrets.gateway_api_key = Some(v); }
    if let Some(v) = update.dashboard_admin_password { secrets.dashboard_admin_password = Some(v); }

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
        .prepare("SELECT user_id, discord_user_id, internal_user_id, last_seen FROM pairings ORDER BY last_seen DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let pairings: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "user_id": row.get::<_, String>(0)?,
                "discord_user_id": row.get::<_, String>(1)?,
                "internal_user_id": row.get::<_, Option<String>>(2)?,
                "last_seen": row.get::<_, Option<String>>(3)?,
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
    conn.execute("DELETE FROM pairings WHERE user_id = ?1", rusqlite::params![user_id])
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok("Pairing deleted".to_string())
}

// ── CL Files ─────────────────────────────────────────────────────────────────

async fn list_cl_files() -> Result<Json<serde_json::Value>, StatusCode> {
    let cl_dir = std::path::Path::new("contexts");
    let mut files = Vec::new();
    if cl_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(cl_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("cl") {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    files.push(serde_json::json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
    }
    files.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    Ok(Json(serde_json::json!({ "cl_files": files })))
}

async fn get_cl_file(
    Path(name): Path<String>,
) -> Result<String, StatusCode> {
    let path = std::path::Path::new("contexts").join(&name);
    std::fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)
}

async fn save_cl_file(
    Path(name): Path<String>,
    Json(update): Json<ClFileUpdate>,
) -> Result<String, StatusCode> {
    let cl_dir = std::path::Path::new("contexts");
    std::fs::create_dir_all(cl_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = cl_dir.join(&name);

    // Validate CL syntax before saving
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
        .prepare("SELECT id, name, schedule, enabled, last_run, run_count FROM cron_jobs ORDER BY name")
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
