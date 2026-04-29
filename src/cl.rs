use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClFile {
    pub name: String,
    pub initial_state: String,
    pub states: HashMap<String, ClState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClState {
    pub task_template: Option<String>,
    pub role_template: Option<String>,
    pub transitions: Vec<ClTransition>,
    pub auto_rules: Vec<ClAutoRule>,
    pub overrides: Option<serde_json::Value>,
    pub secret_overrides: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClTransition {
    pub target: String,
    pub condition: Option<String>,
    pub on: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClAutoRule {
    pub condition: String,
    pub action: String,
    pub target: Option<String>,
    pub value: Option<String>,
}

#[derive(Default)]
struct ClStateBuilder {
    task_template: Option<String>,
    role_template: Option<String>,
    transitions: Vec<ClTransition>,
    auto_rules: Vec<ClAutoRule>,
}

impl ClStateBuilder {
    fn build(self) -> ClState {
        ClState {
            task_template: self.task_template,
            role_template: self.role_template,
            transitions: self.transitions,
            auto_rules: self.auto_rules,
            overrides: None,
            secret_overrides: None,
        }
    }
}

impl ClFile {
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        let mut states = HashMap::new();
        let mut current_state: Option<String> = None;
        let mut current_state_data = ClStateBuilder::default();
        let mut initial_state = String::new();
        let mut name = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                if let Some(state_name) = current_state.take() {
                    if initial_state.is_empty() {
                        initial_state = state_name.clone();
                    }
                    states.insert(state_name, current_state_data.build());
                    current_state_data = ClStateBuilder::default();
                }
                current_state = Some(line[1..line.len() - 1].to_string());
            } else if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let value = line[eq_pos + 1..].trim();

                match key {
                    "name" => name = value.to_string(),
                    "task_template" => {
                        current_state_data.task_template = Some(value.to_string())
                    }
                    "role_template" => {
                        current_state_data.role_template = Some(value.to_string())
                    }
                    _ => {}
                }
            } else if line.contains("->") {
                let parts: Vec<&str> = line.split("->").collect();
                if parts.len() == 2 {
                    let target = parts[1].trim().to_string();
                    current_state_data.transitions.push(ClTransition {
                        target,
                        condition: None,
                        on: Some(parts[0].trim().to_string()),
                    });
                }
            }
        }

        if let Some(state_name) = current_state {
            if initial_state.is_empty() {
                initial_state = state_name.clone();
            }
            states.insert(state_name, current_state_data.build());
        }

        Ok(Self {
            name,
            initial_state,
            states,
        })
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_parse_simple_cl() {
        let content = r#"
name = test

[understand]
task_template = tasks/understand
next -> plan

[plan]
task_template = tasks/plan
next -> code
"#;
        let cl = ClFile::parse(content).unwrap();
        assert_eq!(cl.name, "test");
        assert_eq!(cl.initial_state, "understand");
        assert_eq!(cl.states.len(), 2);
        assert!(cl.states.contains_key("understand"));
        assert!(cl.states.contains_key("plan"));
    }

    #[test]
    fn test_parse_transitions() {
        let content = r#"
[name]
[state1]
task_template = tasks/a
next -> state2

[state2]
task_template = tasks/b
"#;
        let cl = ClFile::parse(content).unwrap();
        let state1 = cl.states.get("state1").unwrap();
        assert_eq!(state1.transitions.len(), 1);
        assert_eq!(state1.transitions[0].target, "state2");
        assert_eq!(state1.transitions[0].on, Some("next".to_string()));
    }

    #[test]
    fn test_parse_empty() {
        let content = "";
        let cl = ClFile::parse(content).unwrap();
        assert!(cl.states.is_empty());
    }
}
