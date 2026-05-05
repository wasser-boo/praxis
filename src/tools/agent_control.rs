use crate::db::Database;

#[derive(Debug, Clone)]
pub enum AgentControlSignal {
    Next,
    Complete,
    Path(String),
    Push(String),
    Pop,
    Feedback(String),
    SetMode(String),
    SetVariable(String, String),
}

pub async fn run(
    db: &Database,
    user_id: &str,
    signal: AgentControlSignal,
) -> Result<String, String> {
    let mut ctx = db
        .load_context(user_id)
        .map_err(|e| format!("Failed to load context: {}", e))?;

    let signal_name = format!("{:?}", signal);

    match signal {
        AgentControlSignal::Next => {
            // Advance to next template in queue
            if let Some(current) = ctx
                .settings
                .active_templates
                .iter()
                .position(|t| t == &ctx.settings.current_template)
            {
                if current + 1 < ctx.settings.active_templates.len() {
                    ctx.settings.current_template =
                        ctx.settings.active_templates[current + 1].clone();
                }
            }
            ctx.settings.llm_turn += 1;

            // Advance CL state if a CL file is configured
            let cl_path = ctx.settings.cl_file.clone().or(ctx.cl_file.clone());
            if let Some(ref path) = cl_path {
                match crate::cl::load_file(path) {
                    Ok(cl) => {
                        let mut ctx_val = serde_json::to_value(&ctx)
                            .map_err(|e| format!("Failed to serialize context: {}", e))?;
                        let current_state = ctx_val.get("active_state").and_then(|v| v.as_str()).unwrap_or("");
                        // Check all possible locations for state_key
                        let state_key = ctx_val.get("state_key").and_then(|v| v.as_str())
                            .or_else(|| ctx_val.pointer("/settings/state_key").and_then(|v| v.as_str()))
                            .unwrap_or("");
                        tracing::info!(user_id = %user_id, path = %path, current_state = %current_state, state_key = %state_key, 
                            context_keys = ?ctx_val.as_object().map(|o| o.keys().collect::<Vec<_>>()), 
                            "CL workflow loaded");
                        if let Some(new_state) = crate::cl::advance_state(&cl, &ctx_val) {
                            crate::cl::transition_to(&cl, &mut ctx_val, &new_state);
                            if let Ok(updated_ctx) = serde_json::from_value::<crate::db::contexts::Context>(ctx_val) {
                                ctx = updated_ctx;
                            }
                            tracing::info!(user_id = %user_id, new_state = %new_state, "CL state advanced via agent_next");
                        } else {
                            tracing::info!(user_id = %user_id, "CL no transition matched");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(user_id = %user_id, path = %path, error = %e, "Failed to load CL file");
                    }
                }
            }
        }
        AgentControlSignal::Complete => {
            ctx.settings.done = true;
        }
        AgentControlSignal::Path(path) => {
            ctx.settings.path = path;
        }
        AgentControlSignal::Push(template_name) => {
            ctx.settings.active_templates.push(template_name);
        }
        AgentControlSignal::Pop => {
            ctx.settings.active_templates.pop();
        }
        AgentControlSignal::Feedback(msg) => {
            ctx.settings.feedback_enabled = true;
            db.save_context(&ctx)
                .map_err(|e| format!("Failed to save context: {}", e))?;
            tracing::info!(user_id = %user_id, msg = %msg, "Agent feedback");
            return Ok(format!("Feedback sent: {}", msg));
        }
        AgentControlSignal::SetMode(mode) => {
            if ctx.custom_data.is_null() {
                ctx.custom_data = serde_json::json!({});
            }
            if let Some(map) = ctx.custom_data.as_object_mut() {
                map.insert("mode".to_string(), serde_json::json!(mode));
            }
        }
        AgentControlSignal::SetVariable(key, value) => {
            if ctx.custom_data.is_null() {
                ctx.custom_data = serde_json::json!({});
            }
            if let Some(map) = ctx.custom_data.as_object_mut() {
                map.insert(key, serde_json::json!(value));
            }
        }
    }

    db.save_context(&ctx)
        .map_err(|e| format!("Failed to save context: {}", e))?;

    Ok(format!("Agent signal processed: {}", signal_name))
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_setup() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn test_signal_variants() {
        let signals = vec![
            AgentControlSignal::Next,
            AgentControlSignal::Complete,
            AgentControlSignal::Path("/tmp".to_string()),
            AgentControlSignal::Push("template".to_string()),
            AgentControlSignal::Pop,
            AgentControlSignal::Feedback("msg".to_string()),
            AgentControlSignal::SetMode("code".to_string()),
            AgentControlSignal::SetVariable("key".to_string(), "val".to_string()),
        ];
        assert_eq!(signals.len(), 8);
    }

    #[tokio::test]
    async fn test_run_complete() {
        let (db, _dir) = test_setup();
        let result = run(&db, "user1", AgentControlSignal::Complete).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Complete"));

        let ctx = db.load_context("user1").unwrap();
        assert!(ctx.settings.done);
    }

    #[tokio::test]
    async fn test_run_path() {
        let (db, _dir) = test_setup();
        let result = run(
            &db,
            "user1",
            AgentControlSignal::Path("/home/user".to_string()),
        )
        .await;
        assert!(result.is_ok());

        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.settings.path, "/home/user");
    }

    #[tokio::test]
    async fn test_run_push_pop() {
        let (db, _dir) = test_setup();

        run(&db, "user1", AgentControlSignal::Push("t1".to_string()))
            .await
            .unwrap();
        run(&db, "user1", AgentControlSignal::Push("t2".to_string()))
            .await
            .unwrap();

        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.settings.active_templates, vec!["t1", "t2"]);

        run(&db, "user1", AgentControlSignal::Pop).await.unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.settings.active_templates, vec!["t1"]);
    }

    #[tokio::test]
    async fn test_run_next() {
        let (db, _dir) = test_setup();

        run(&db, "user1", AgentControlSignal::Push("t1".to_string()))
            .await
            .unwrap();
        run(&db, "user1", AgentControlSignal::Push("t2".to_string()))
            .await
            .unwrap();
        run(&db, "user1", AgentControlSignal::Next).await.unwrap();

        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.settings.llm_turn, 1);
    }

    #[tokio::test]
    async fn test_run_set_mode() {
        let (db, _dir) = test_setup();
        run(
            &db,
            "user1",
            AgentControlSignal::SetMode("code".to_string()),
        )
        .await
        .unwrap();

        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.custom_data["mode"], "code");
    }

    #[tokio::test]
    async fn test_run_set_variable() {
        let (db, _dir) = test_setup();
        run(
            &db,
            "user1",
            AgentControlSignal::SetVariable("language".to_string(), "rust".to_string()),
        )
        .await
        .unwrap();

        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.custom_data["language"], "rust");
    }

    #[tokio::test]
    async fn test_run_feedback() {
        let (db, _dir) = test_setup();
        let result = run(
            &db,
            "user1",
            AgentControlSignal::Feedback("progress update".to_string()),
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Feedback sent"));
    }
}
