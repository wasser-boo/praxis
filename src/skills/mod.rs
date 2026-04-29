pub mod executor;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub template: String,
    pub triggers: Vec<String>,
    pub parameters: serde_json::Value,
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

    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    pub fn find_by_trigger(&self, input: &str) -> Option<&Skill> {
        let input_lower = input.to_lowercase();
        self.skills.values().find(|s| {
            s.triggers
                .iter()
                .any(|t| input_lower.contains(&t.to_lowercase()))
        })
    }

    pub fn load_from_dir(&mut self, dir: &Path) -> anyhow::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let content = std::fs::read_to_string(&path)?;
                let skill: Skill = serde_json::from_str(&content)?;
                self.register(skill);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn test_skill_registry() {
        let mut registry = SkillRegistry::new();
        registry.register(Skill {
            name: "code_review".to_string(),
            description: "Review code".to_string(),
            template: "review.poml".to_string(),
            triggers: vec!["review".to_string(), "check code".to_string()],
            parameters: serde_json::json!({}),
        });
        assert!(registry.get("code_review").is_some());
        assert!(registry.find_by_trigger("please review this").is_some());
        assert!(registry.find_by_trigger("hello").is_none());
    }

    #[test]
    fn test_skill_list() {
        let mut registry = SkillRegistry::new();
        registry.register(Skill {
            name: "a".to_string(),
            description: "A".to_string(),
            template: "a.poml".to_string(),
            triggers: vec![],
            parameters: serde_json::json!({}),
        });
        registry.register(Skill {
            name: "b".to_string(),
            description: "B".to_string(),
            template: "b.poml".to_string(),
            triggers: vec![],
            parameters: serde_json::json!({}),
        });
        assert_eq!(registry.list().len(), 2);
    }
}
