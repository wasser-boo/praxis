use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub description: String,
    pub version: String,
    pub tools: Vec<PluginTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub endpoint: Option<String>,
}

pub struct PluginRegistry {
    plugins: HashMap<String, Plugin>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Plugin) {
        self.plugins.insert(plugin.name.clone(), plugin);
    }

    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    pub fn list(&self) -> Vec<&Plugin> {
        self.plugins.values().collect()
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn test_plugin_registry() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin {
            name: "test".to_string(),
            description: "Test plugin".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
        });
        assert!(registry.get("test").is_some());
        assert_eq!(registry.list().len(), 1);
    }
}
