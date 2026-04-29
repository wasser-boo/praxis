use crate::skills::Skill;

pub async fn execute_skill(
    skill: &Skill,
    context: &serde_json::Value,
) -> anyhow::Result<String> {
    tracing::info!("Executing skill: {}", skill.name);

    let mut result = skill.template.clone();

    if let Some(obj) = context.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }

    Ok(result)
}

pub async fn execute_skill_by_name(
    registry: &crate::skills::SkillRegistry,
    name: &str,
    context: &serde_json::Value,
) -> anyhow::Result<String> {
    let skill = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Skill not found: {}", name))?;
    execute_skill(skill, context).await
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    use crate::skills::Skill;

    #[tokio::test]
    async fn test_execute_skill() {
        let skill = Skill {
            name: "test".to_string(),
            description: "Test skill".to_string(),
            template: "Hello {{name}}!".to_string(),
            triggers: vec![],
            parameters: serde_json::json!({}),
        };
        let context = serde_json::json!({"name": "World"});
        let result = execute_skill(&skill, &context).await.unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[tokio::test]
    async fn test_execute_skill_no_vars() {
        let skill = Skill {
            name: "test".to_string(),
            description: "Test skill".to_string(),
            template: "Static content".to_string(),
            triggers: vec![],
            parameters: serde_json::json!({}),
        };
        let result = execute_skill(&skill, &serde_json::json!({})).await.unwrap();
        assert_eq!(result, "Static content");
    }
}
