use crate::db::Database;
use crate::skills::{execute_skill_by_name, SkillRegistry};
use serde_json::Value;
use std::path::Path;

/// Load instructions only. The agent performs any follow-up actions via normal
/// tools and their permission controls; loading a skill never runs its scripts.
pub async fn run(db: &Database, args: &Value) -> anyhow::Result<String> {
    anyhow::ensure!(
        crate::db::tools::get(db, "use_skill")?.is_enabled,
        "use_skill is disabled"
    );
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("use_skill requires a non-empty string 'name'"))?;
    let empty = serde_json::json!({});
    let parameters = args.get("parameters").unwrap_or(&empty);
    anyhow::ensure!(
        parameters.is_object(),
        "Skill parameters must be a JSON object"
    );

    // Look up a registered name, never construct a path from model arguments.
    let mut registry = SkillRegistry::new();
    registry.load_from_dir(Path::new("skills"))?;
    let instructions = execute_skill_by_name(&registry, name, parameters).await?;
    Ok(format!(
        "Loaded skill '{name}'. These are instructions, not a completed action. Follow them using the available tools and existing permissions.\n\n{instructions}"
    ))
}
