//! Opt-in experiment fixture. Reuses the tool benchmark's real gateway,
//! provider observation and fail-closed executor guard; never routes by rubric.
use super::*;

#[tokio::test]
#[ignore = "Paid live inference: one isolated task/arm of the 20-task experiment"]
async fn local_model_live_state_task() -> anyhow::Result<()> {
    anyhow::ensure!(std::env::var_os("GPU_ROUTER_URL").is_none(),"Unset GPU_ROUTER_URL; no wake/rental in tests");
    let url=std::env::var("PRAXIS_LIVE_URL")?;
    let model=std::env::var("PRAXIS_LIVE_MODEL")?;
    let out=PathBuf::from(std::env::var("PRAXIS_LIVE_ARTIFACTS")?);
    std::fs::create_dir(&out)?;
    let id=std::env::var("PRAXIS_STATE_CASE")?;
    let arm=std::env::var("PRAXIS_STATE_ARM")?;
    anyhow::ensure!(["fixed","entry","continuous","decision"].contains(&arm.as_str()),"Unknown experiment arm");
    let manifest:serde_json::Value=serde_json::from_str(&std::fs::read_to_string("tests/fixtures/20-tasks.json")?)?;
    let task=manifest.as_array().and_then(|tasks|tasks.iter().find(|t|t["id"]==id))
        .ok_or_else(||anyhow::anyhow!("Unknown state experiment case"))?;
    let dir=tempfile::tempdir()?;
    let db=crate::db::Database::new(dir.path())?;
    crate::db::tools::init_default_tools(&db)?;
    let user=format!("state-live-{}",uuid::Uuid::new_v4());
    let fixture_file=dir.path().join("task.txt");
    if let Some(text)=task["fixture"].as_str() {std::fs::write(&fixture_file,text)?;}
    let prompt=task["prompt"].as_str().ok_or_else(||anyhow::anyhow!("Missing prompt"))?
        .replace("{fixture}",&fixture_file.to_string_lossy());
    let mut ctx=db.load_context(&user)?;
    ctx.settings.provider=Some("llamacpp".into());
    ctx.settings.model=Some(model.clone());
    ctx.settings.sm_file=Some("20-tasks".into());
    ctx.settings.system_template=Some("20-tasks".into());
    ctx.active_state=Some(task["initial_state"].as_str().unwrap_or("standard").into());
    ctx.settings.active_state=ctx.active_state.clone();
    ctx.settings.thinking_mode="low".into();
    ctx.settings.show_thinking=true;
    ctx.settings.compaction_enabled=false;
    ctx.settings.max_llm_turns=Some(1); // real chat tool-followup pipeline
    ctx.settings.max_tool_calls=Some(8);
    ctx.settings.path=dir.path().to_string_lossy().into();
    let mut custom_data = json!({"state_eval_policy":arm});
    if arm == "decision" {
        custom_data["decision_profile"] = json!("task-router");
        ctx.settings.decision_profile = Some("task-router".into());
    }
    ctx.custom_data = custom_data;
    db.save_context(&ctx)?;
    std::fs::write(out.join("fixture.json"),serde_json::to_vec_pretty(&json!({
        "task":task,"arm":arm,"initial_context":ctx,"message":prompt,
        "policy":"isolated DB; normal discovery; set_context only active_state to declared states; read-only fixture; no shell, network tools or file writes; rubric never in model context"
    }))?)?;
    let provider=ObservedProvider {
        inner:LlamaCppProvider::new(std::env::var("PRAXIS_LIVE_API_KEY").ok(),model,url),
        fixture_file,artifacts:out.clone(),calls:AtomicUsize::new(0),state_user:Some((db.clone(),user.clone())),
    };
    let state=GatewayState {
        db:db.clone(),config:crate::config::Config::from_env(),secrets:Default::default(),
        llm:crate::gateway::LlmHandle::new(LLMRouter::with_providers(vec![Box::new(provider)],"llamacpp".into(),vec![],ResilienceConfig {max_attempts:1,..Default::default()})),
        plugins:Arc::new(crate::plugins::PluginRegistry::new()),
        event_tx:tokio::sync::broadcast::channel(16).0,start_time:Instant::now(),
    };
    let start=Instant::now();
    let result=fixture_http_chat(state,&user,&prompt).await;
    std::fs::write(out.join("result.json"),serde_json::to_vec_pretty(&json!({
        "http_result":result.as_ref().ok(),"transport_error":result.as_ref().err().map(|e|e.to_string()),
        "elapsed_ms":start.elapsed().as_millis(),"history":db.get_messages(&user,200)?,
        "final_context":db.load_context(&user)?,
    }))?)?;
    let result=result?;
    anyhow::ensure!(result["success"]==true,"Gateway task failed: {}",result["error"]);
    // Completion is not quality or correct state selection. The runner scores
    // those separately, including successful answers with no model transition.
    eprintln!("COMPLETED: task {id}, arm {arm}, elapsed {:?}, artifacts {}",start.elapsed(),out.display());
    Ok(())
}