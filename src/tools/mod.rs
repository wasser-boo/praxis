pub mod execute_terminal;
pub mod write_file;
pub mod edit_file;
pub mod agent_control;
pub mod context_tools;
pub mod get_context;
pub mod discord_upload;
pub mod discord_send_message;
pub mod web_search;
pub mod rag_query;
pub mod rag_ingest;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    pub fn definitions(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn test_tool_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDefinition {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({}),
        });
        assert!(registry.get("test").is_some());
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_tool_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDefinition {
            name: "echo".to_string(),
            description: "Echo input".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        });
        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
    }
}
