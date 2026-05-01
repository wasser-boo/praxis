pub mod minimax_image;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub description: String,
    pub version: String,
    pub tools: Vec<PluginTool>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub handler: PluginHandler,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginHandler {
    Builtin(String),
    Http { url: String, method: String },
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

    pub fn enabled_tools(&self) -> Vec<&PluginTool> {
        self.plugins
            .values()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.tools)
            .collect()
    }

    pub fn tool_definitions(&self) -> Vec<crate::gateway::llm::provider::ToolDefinition> {
        self.enabled_tools()
            .iter()
            .map(|t| crate::gateway::llm::provider::ToolDefinition {
                tool_type: "function".to_string(),
                function: crate::gateway::llm::provider::FunctionDefinition {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    }

    pub async fn execute_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<String> {
        for plugin in self.plugins.values() {
            if !plugin.enabled {
                continue;
            }
            for tool in &plugin.tools {
                if tool.name == tool_name {
                    return match &tool.handler {
                        PluginHandler::Builtin(builtin_name) => {
                            minimax_image::execute_builtin(builtin_name, args).await
                        }
                        PluginHandler::Http { url, method } => {
                            execute_http_tool(url, method, args).await
                        }
                    };
                }
            }
        }
        Err(anyhow::anyhow!("Plugin tool not found: {}", tool_name))
    }
}

async fn execute_http_tool(
    url: &str,
    method: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let resp = match method.to_uppercase().as_str() {
        "POST" => client.post(url).json(args).send().await?,
        "GET" => {
            client
                .get(url)
                .query(&args.as_object().unwrap_or(&serde_json::Map::new()))
                .send()
                .await?
        }
        _ => anyhow::bail!("Unsupported HTTP method: {}", method),
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Plugin HTTP error {}: {}", status, text);
    }

    let data: serde_json::Value = resp.json().await?;
    Ok(serde_json::to_string_pretty(&data)?)
}

pub fn register_builtin_plugins(registry: &mut PluginRegistry) {
    registry.register(minimax_image::create_plugin());
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
            enabled: true,
        });
        assert!(registry.get("test").is_some());
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_plugin_enabled_tools() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin {
            name: "test".to_string(),
            description: "Test plugin".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![PluginTool {
                name: "tool1".to_string(),
                description: "A tool".to_string(),
                parameters: serde_json::json!({}),
                handler: PluginHandler::Builtin("test".to_string()),
            }],
            enabled: true,
        });
        assert_eq!(registry.enabled_tools().len(), 1);
    }

    #[test]
    fn test_plugin_disabled_tools() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin {
            name: "test".to_string(),
            description: "Test plugin".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![PluginTool {
                name: "tool1".to_string(),
                description: "A tool".to_string(),
                parameters: serde_json::json!({}),
                handler: PluginHandler::Builtin("test".to_string()),
            }],
            enabled: false,
        });
        assert_eq!(registry.enabled_tools().len(), 0);
    }

    #[test]
    fn test_builtin_plugins_registered() {
        let mut registry = PluginRegistry::new();
        register_builtin_plugins(&mut registry);
        assert!(registry.get("minimax_image").is_some());
    }
}
