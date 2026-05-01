pub mod minimax_image;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub description: String,
    pub version: String,
    pub tools: Vec<PluginTool>,
    pub context: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub handler: PluginHandler,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginHandler {
    #[serde(rename = "builtin")]
    Builtin { name: String },
    #[serde(rename = "http")]
    Http { url: String, method: String },
    #[serde(rename = "script")]
    Script { path: String, interpreter: String },
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

    pub fn context_defaults(&self) -> HashMap<String, serde_json::Value> {
        let mut defaults = HashMap::new();
        for plugin in self.plugins.values() {
            if !plugin.enabled {
                continue;
            }
            for (key, value) in &plugin.context {
                defaults.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
        defaults
    }

    pub fn collect_secrets(&self) -> Vec<String> {
        let mut secrets = Vec::new();
        for plugin in self.plugins.values() {
            if !plugin.enabled {
                continue;
            }
            for key in &plugin.secrets {
                if !secrets.contains(key) {
                    secrets.push(key.clone());
                }
            }
        }
        secrets
    }

    pub async fn execute_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        context: Option<&serde_json::Value>,
        secrets: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<String> {
        for plugin in self.plugins.values() {
            if !plugin.enabled {
                continue;
            }
            for tool in &plugin.tools {
                if tool.name == tool_name {
                    return match &tool.handler {
                        PluginHandler::Builtin { name } => {
                            minimax_image::execute_builtin(name, args).await
                        }
                        PluginHandler::Http { url, method } => {
                            execute_http_tool(url, method, args).await
                        }
                        PluginHandler::Script { path, interpreter } => {
                            execute_script(path, interpreter, args, context, secrets).await
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

async fn execute_script(
    path: &str,
    interpreter: &str,
    args: &serde_json::Value,
    context: Option<&serde_json::Value>,
    secrets: Option<&HashMap<String, String>>,
) -> anyhow::Result<String> {
    let input = serde_json::to_string(args)?;

    let mut cmd = tokio::process::Command::new(interpreter);
    cmd.arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("PLUGIN_ARGS", &input);

    if let Some(ctx) = context {
        cmd.env("PLUGIN_CONTEXT", serde_json::to_string(ctx).unwrap_or_default());
    }

    if let Some(sec) = secrets {
        if !sec.is_empty() {
            cmd.env("PLUGIN_SECRETS", serde_json::to_string(sec).unwrap_or_default());
        }
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute script {}: {}", path, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Script {} exited with {}: {}",
            path,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        return Ok("Script executed successfully (no output)".to_string());
    }

    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        Ok(trimmed.to_string())
    } else {
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "result": trimmed
        }))?)
    }
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: String,
    description: String,
    version: String,
    #[serde(default = "default_true")]
    enabled: bool,
    tools: Vec<PluginTool>,
    #[serde(default)]
    context: HashMap<String, serde_json::Value>,
    #[serde(default)]
    secrets: Vec<String>,
}

pub fn load_plugins_from_dir(dir: &Path) -> anyhow::Result<Vec<Plugin>> {
    let mut plugins = Vec::new();
    if !dir.exists() {
        return Ok(plugins);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let plugin_dir = entry.path();

        if !plugin_dir.is_dir() {
            continue;
        }

        let manifest_path = plugin_dir.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }

        match load_plugin_from_manifest(&manifest_path, &plugin_dir) {
            Ok(plugin) => {
                tracing::info!(
                    name = %plugin.name,
                    version = %plugin.version,
                    tools = plugin.tools.len(),
                    "Loaded plugin"
                );
                plugins.push(plugin);
            }
            Err(e) => {
                tracing::warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "Failed to load plugin"
                );
            }
        }
    }

    Ok(plugins)
}

fn load_plugin_from_manifest(manifest_path: &Path, plugin_dir: &Path) -> anyhow::Result<Plugin> {
    let data = std::fs::read_to_string(manifest_path)?;
    let manifest: PluginManifest = serde_json::from_str(&data)?;

    let tools = manifest
        .tools
        .into_iter()
        .map(|t| {
            if let PluginHandler::Script { path, interpreter } = t.handler {
                let abs_path = plugin_dir.join(&path);
                PluginTool {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                    handler: PluginHandler::Script {
                        path: abs_path.to_string_lossy().to_string(),
                        interpreter,
                    },
                }
            } else {
                t
            }
        })
        .collect();

    Ok(Plugin {
        name: manifest.name,
        description: manifest.description,
        version: manifest.version,
        tools,
        context: manifest.context,
        secrets: manifest.secrets,
        enabled: manifest.enabled,
    })
}

pub fn register_builtin_plugins(registry: &mut PluginRegistry) {
    registry.register(minimax_image::create_plugin());
}

pub fn load_all_plugins(plugins_dir: &Path) -> PluginRegistry {
    let mut registry = PluginRegistry::new();

    register_builtin_plugins(&mut registry);

    match load_plugins_from_dir(plugins_dir) {
        Ok(plugins) => {
            for plugin in plugins {
                registry.register(plugin);
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load plugins from directory");
        }
    }

    registry
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
            context: HashMap::new(),
            secrets: Vec::new(),
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
                handler: PluginHandler::Builtin { name: "test".to_string() },
            }],
            context: HashMap::new(),
            secrets: Vec::new(),
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
                handler: PluginHandler::Builtin { name: "test".to_string() },
            }],
            context: HashMap::new(),
            secrets: Vec::new(),
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

    #[test]
    fn test_load_folder_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("my_plugin");
        std::fs::create_dir(&plugin_dir).unwrap();

        let manifest = serde_json::json!({
            "name": "my_plugin",
            "description": "A test plugin",
            "version": "0.1.0",
            "enabled": true,
            "tools": [{
                "name": "hello",
                "description": "Say hello",
                "parameters": {"type": "object", "properties": {"name": {"type": "string"}}},
                "handler": {"type": "script", "path": "hello.sh", "interpreter": "bash"}
            }]
        });
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let plugins = load_plugins_from_dir(dir.path()).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "my_plugin");
        assert_eq!(plugins[0].tools.len(), 1);
        assert_eq!(plugins[0].tools[0].name, "hello");

        match &plugins[0].tools[0].handler {
            PluginHandler::Script { path, interpreter } => {
                assert!(path.ends_with("hello.sh"));
                assert_eq!(interpreter, "bash");
            }
            _ => panic!("Expected Script handler"),
        }
    }

    #[test]
    fn test_load_folder_skips_non_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a plugin").unwrap();

        let plugins = load_plugins_from_dir(dir.path()).unwrap();
        assert_eq!(plugins.len(), 0);
    }

    #[test]
    fn test_manifest_handler_deserialization() {
        let script_json = r#"{"type": "script", "path": "run.py", "interpreter": "python3"}"#;
        let handler: PluginHandler = serde_json::from_str(script_json).unwrap();
        match handler {
            PluginHandler::Script { path, interpreter } => {
                assert_eq!(path, "run.py");
                assert_eq!(interpreter, "python3");
            }
            _ => panic!("Expected Script"),
        }

        let http_json = r#"{"type": "http", "url": "http://localhost:8080", "method": "POST"}"#;
        let handler: PluginHandler = serde_json::from_str(http_json).unwrap();
        match handler {
            PluginHandler::Http { url, method } => {
                assert_eq!(url, "http://localhost:8080");
                assert_eq!(method, "POST");
            }
            _ => panic!("Expected Http"),
        }

        let builtin_json = r#"{"type": "builtin", "name": "image_generate"}"#;
        let handler: PluginHandler = serde_json::from_str(builtin_json).unwrap();
        match handler {
            PluginHandler::Builtin { name } => {
                assert_eq!(name, "image_generate");
            }
            _ => panic!("Expected Builtin"),
        }
    }

    #[test]
    fn test_context_defaults_from_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("ctx_plugin");
        std::fs::create_dir(&plugin_dir).unwrap();

        let manifest = serde_json::json!({
            "name": "ctx_plugin",
            "description": "Plugin with context",
            "version": "1.0.0",
            "enabled": true,
            "tools": [],
            "context": {
                "api_url": "http://localhost:8080",
                "api_key": "",
                "max_results": 20
            }
        });
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let plugins = load_plugins_from_dir(dir.path()).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].context.len(), 3);
        assert_eq!(plugins[0].context["api_url"], "http://localhost:8080");
        assert_eq!(plugins[0].context["max_results"], 20);

        let mut registry = PluginRegistry::new();
        for p in plugins {
            registry.register(p);
        }
        let defaults = registry.context_defaults();
        assert_eq!(defaults.len(), 3);
        assert_eq!(defaults["api_url"], "http://localhost:8080");
    }

    #[test]
    fn test_context_defaults_empty() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin {
            name: "no_ctx".to_string(),
            description: "No context".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            context: HashMap::new(),
            secrets: Vec::new(),
            enabled: true,
        });
        assert!(registry.context_defaults().is_empty());
    }

    #[test]
    fn test_collect_secrets() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin {
            name: "sec_plugin".to_string(),
            description: "Plugin with secrets".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            context: HashMap::new(),
            secrets: vec!["api_key".to_string(), "api_secret".to_string()],
            enabled: true,
        });
        let sec = registry.collect_secrets();
        assert_eq!(sec.len(), 2);
        assert!(sec.contains(&"api_key".to_string()));
        assert!(sec.contains(&"api_secret".to_string()));
    }
}
