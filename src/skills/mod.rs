use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(skip)]
    pub folder: String,
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
                    let content = std::fs::read_to_string(&skill_json)?;
                    let mut skill: Skill = serde_json::from_str(&content)?;
                    skill.folder = path.to_string_lossy().to_string();
                    self.skills.insert(skill.name.clone(), skill);
                }
            }
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Return skills as JSON array for POML context
    pub fn to_context_array(&self) -> serde_json::Value {
        let skills: Vec<serde_json::Value> = self
            .skills
            .values()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description
                })
            })
            .collect();
        serde_json::json!(skills)
    }
}

pub async fn execute_skill(skill: &Skill, context: &serde_json::Value) -> anyhow::Result<String> {
    let poml_path = format!("{}/skill.poml", skill.folder);
    tracing::info!("Executing skill: {} from {}", skill.name, poml_path);
    crate::gateway::poml::render(&poml_path, context).await
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
    fn test_empty_registry() {
        let registry = SkillRegistry::new();
        assert!(registry.list().is_empty());
        assert!(registry.to_context_array().as_array().unwrap().is_empty());
    }
}
