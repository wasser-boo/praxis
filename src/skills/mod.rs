use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Required non-empty string inputs, injected as top-level POML variables.
    #[serde(default)]
    pub required_parameters: Vec<String>,
    #[serde(skip)]
    pub folder: String,
}

impl Skill {
    /// Automatic activation maps only the documented task-input aliases.
    /// Other required inputs must already exist in the shared context.
    pub fn parameters_for_task(&self, context: &serde_json::Value, task: &str) -> anyhow::Result<serde_json::Value> {
        let mut parameters = context.clone();
        anyhow::ensure!(parameters.is_object(), "Skill context must be an object");
        for key in ["code", "error", "user_request"] {
            parameters[key] = serde_json::json!(task);
        }
        self.validate_parameters(&parameters)?;
        Ok(parameters)
    }

    pub fn validate_parameters(&self, parameters: &serde_json::Value) -> anyhow::Result<()> {
        anyhow::ensure!(
            parameters.is_object(),
            "Skill parameters must be a JSON object"
        );
        for key in &self.required_parameters {
            anyhow::ensure!(
                parameters
                    .get(key)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.trim().is_empty()),
                "Skill '{}' requires a non-empty string parameter '{}'",
                self.name,
                key
            );
        }
        Ok(())
    }
}

pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    pub fn load_from_dir(&mut self, dir: &Path) -> anyhow::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let skill_json = path.join("skill.json");
                let skill_poml = path.join("skill.poml");

                if skill_json.exists() && skill_poml.exists() {
                    // A broken optional skill must not hide unrelated valid skills.
                    let loaded = (|| -> anyhow::Result<Skill> {
                        let content = std::fs::read_to_string(&skill_json)?;
                        let mut skill: Skill = serde_json::from_str(&content)?;
                        anyhow::ensure!(
                            !skill.name.is_empty()
                                && skill
                                    .name
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                            "Invalid skill name"
                        );
                        anyhow::ensure!(
                            !skill.description.trim().is_empty(),
                            "Missing skill description"
                        );
                        skill.folder = path.to_string_lossy().to_string();
                        Ok(skill)
                    })();
                    match loaded {
                        Ok(skill) => {
                            self.skills.insert(skill.name.clone(), skill);
                        }
                        Err(error) => {
                            tracing::warn!(path = %skill_json.display(), %error, "Skipping invalid skill")
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        let mut skills: Vec<_> = self.skills.values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// Return skills as JSON array for POML context
    pub fn to_context_array(&self) -> serde_json::Value {
        let skills: Vec<serde_json::Value> = self
            .list()
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "required_parameters": s.required_parameters
                })
            })
            .collect();
        serde_json::json!(skills)
    }
}

pub async fn execute_skill(skill: &Skill, context: &serde_json::Value) -> anyhow::Result<String> {
    skill.validate_parameters(context)?;
    let poml_path = format!("{}/skill.poml", skill.folder);
    tracing::info!("Rendering skill: {} from {}", skill.name, poml_path);
    // Never present unrendered fallback markup as a successfully loaded skill.
    crate::gateway::poml::render_strict(&poml_path, context).await
}

pub async fn execute_skill_by_name(
    registry: &SkillRegistry,
    name: &str,
    context: &serde_json::Value,
) -> anyhow::Result<String> {
    let skill = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Skill not found: {}", name))?;
    execute_skill(skill, context).await
}

#[cfg(test)]
mod skill_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_skill(dir: &Path, name: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.json"),
            format!(r#"{{"name": "{}", "description": "Test skill"}}"#, name),
        )
        .unwrap();
        fs::write(
            skill_dir.join("skill.poml"),
            "<poml><p>Hello {{name}}</p></poml>",
        )
        .unwrap();
    }

    #[test]
    fn test_load_skills() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "test");
        let mut registry = SkillRegistry::new();
        registry.load_from_dir(dir.path()).unwrap();
        assert!(registry.get("test").is_some());
    }

    #[test]
    fn test_skill_missing_poml() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("bad");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.json"),
            r#"{"name":"bad","description":"x"}"#,
        )
        .unwrap();
        let mut registry = SkillRegistry::new();
        registry.load_from_dir(dir.path()).unwrap();
        assert!(registry.get("bad").is_none());
    }

    #[test]
    fn test_to_context_array() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "a");
        create_test_skill(dir.path(), "b");
        let mut registry = SkillRegistry::new();
        registry.load_from_dir(dir.path()).unwrap();
        let arr = registry.to_context_array();
        assert!(arr.is_array());
        assert_eq!(arr.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_invalid_manifest_does_not_hide_valid_skills() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "valid");
        create_test_skill(dir.path(), "broken");
        fs::write(dir.path().join("broken/skill.json"), "not JSON").unwrap();
        let mut registry = SkillRegistry::new();
        registry.load_from_dir(dir.path()).unwrap();
        assert!(registry.get("valid").is_some());
        assert!(registry.get("broken").is_none());
    }

    #[test]
    fn test_required_parameters_are_advertised_and_checked() {
        let skill: Skill = serde_json::from_value(serde_json::json!({
            "name": "debug", "description": "Debug errors", "required_parameters": ["error"]
        }))
        .unwrap();
        for context in [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({"error": null}),
            serde_json::json!({"error": " "}),
            serde_json::json!({"error": 42}),
        ] {
            assert!(skill.validate_parameters(&context).is_err());
        }
        assert!(skill
            .validate_parameters(&serde_json::json!({"error": "test"}))
            .is_ok());
        let mut registry = SkillRegistry::new();
        registry.skills.insert(skill.name.clone(), skill);
        assert_eq!(
            registry.to_context_array()[0]["required_parameters"],
            serde_json::json!(["error"])
        );
    }

    #[test]
    fn backend_active_skill_maps_task_aliases_without_inventing_inputs() {
        for key in ["code", "error", "user_request"] {
            let skill: Skill = serde_json::from_value(serde_json::json!({"name":"test", "description":"Test", "required_parameters":[key]})).unwrap();
            let parameters = skill.parameters_for_task(&serde_json::json!({"user_prompt":"old"}), "current task").unwrap();
            assert_eq!(parameters[key], "current task");
        }
        let skill: Skill = serde_json::from_value(serde_json::json!({"name":"test", "description":"Test", "required_parameters":["unprovided"]})).unwrap();
        assert!(skill.parameters_for_task(&serde_json::json!({}), "task").is_err());
    }

    #[test]
    fn test_empty_registry() {
        let registry = SkillRegistry::new();
        assert!(registry.list().is_empty());
        assert!(registry.to_context_array().as_array().unwrap().is_empty());
    }
}
