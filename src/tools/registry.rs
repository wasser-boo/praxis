//! Central tool registry with metadata for state-based filtering.
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// Tools that execute actions with side effects
    Action,
    /// Discovery tools (search_tools, search_skills)
    Discovery,
    /// Skill loading
    SkillLoader,
    /// Agent loop control
    AgentControl,
    /// Memory operations
    Memory,
    /// Context operations
    Context,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ToolCategory,
    pub params_schema: serde_json::Value,
    pub default_enabled: bool,
}

/// Static registry initialized once at startup
static TOOL_REGISTRY: Lazy<Vec<ToolMeta>> = Lazy::new(build_registry);

/// Pre-defined tool groups for reuse across states
static TOOL_GROUPS: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("core_files", vec!["read_file", "write_file", "edit_file"]);
    m.insert("core_terminal", vec!["execute_terminal", "read_file", "write_file"]);
    m.insert("coding", vec!["execute_terminal", "read_file", "write_file", "edit_file", "search_tools"]);
    m.insert("agent_basic", vec!["agent_complete", "agent_next", "agent_feedback"]);
    m.insert("context", vec!["get_context", "set_context", "read_tool_result"]);
    m.insert("memory", vec!["memory_get", "memory_set", "learn_fact", "learn_preference", "learn_topic"]);
    m.insert("discovery", vec!["search_tools", "search_skills"]);
    m.insert("skills", vec!["search_skills", "use_skill"]);
    m.insert("discord", vec!["discord_send_message", "discord_send_embed", "discord_upload_file"]);
    m.insert("vm", vec!["vm_start", "vm_stop", "vm_shell", "vm_keys", "vm_mouse", "vm_screenshot", "vm_file_transfer", "vm_snapshot", "vm_shared_folder"]);
    m.insert("cron", vec!["cron_add", "cron_delete", "cron_list", "cron_toggle", "cron_run"]);
    m
});

fn build_registry() -> Vec<ToolMeta> {
    vec![
        // Action tools
        ToolMeta {
            name: "execute_terminal",
            description: "Run shell command on host (or VM if VM_MODE=vm)",
            category: ToolCategory::Action,
            params_schema: terminal_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "read_file",
            description: "Read file contents",
            category: ToolCategory::Action,
            params_schema: read_file_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "write_file",
            description: "Create/overwrite file",
            category: ToolCategory::Action,
            params_schema: write_file_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "edit_file",
            description: "Find/replace in file",
            category: ToolCategory::Action,
            params_schema: edit_file_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "discord_send_message",
            description: "Send Discord message to a channel",
            category: ToolCategory::Action,
            params_schema: discord_send_message_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "discord_send_embed",
            description: "Send Discord rich embed",
            category: ToolCategory::Action,
            params_schema: discord_send_embed_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "discord_upload_file",
            description: "Upload file to Discord channel",
            category: ToolCategory::Action,
            params_schema: discord_upload_file_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_start",
            description: "Start/create a VM",
            category: ToolCategory::Action,
            params_schema: vm_start_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_stop",
            description: "Stop a VM",
            category: ToolCategory::Action,
            params_schema: vm_stop_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_shell",
            description: "Execute shell command in VM",
            category: ToolCategory::Action,
            params_schema: vm_shell_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_keys",
            description: "Send keyboard input to VM",
            category: ToolCategory::Action,
            params_schema: vm_keys_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_mouse",
            description: "Mouse control in VM",
            category: ToolCategory::Action,
            params_schema: vm_mouse_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_screenshot",
            description: "Capture VM display as base64 image",
            category: ToolCategory::Action,
            params_schema: vm_screenshot_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_file_transfer",
            description: "Transfer files to/from VM",
            category: ToolCategory::Action,
            params_schema: vm_file_transfer_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_snapshot",
            description: "Create VM snapshot",
            category: ToolCategory::Action,
            params_schema: vm_snapshot_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "vm_shared_folder",
            description: "Add shared host<->VM folder",
            category: ToolCategory::Action,
            params_schema: vm_shared_folder_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "understand_image",
            description: "Analyze image content",
            category: ToolCategory::Action,
            params_schema: understand_image_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "update_template",
            description: "Strictly validate and save a POML template",
            category: ToolCategory::Action,
            params_schema: update_template_schema(),
            default_enabled: true,
        },
        // Discovery tools
        ToolMeta {
            name: "search_tools",
            description: "Find enabled tools by keyword. Returns name and description only.",
            category: ToolCategory::Discovery,
            params_schema: search_tools_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "search_skills",
            description: "Find skills by keyword. Returns name, description, required_parameters.",
            category: ToolCategory::Discovery,
            params_schema: search_skills_schema(),
            default_enabled: true,
        },
        // Skill Loader
        ToolMeta {
            name: "use_skill",
            description: "Load one registered skill's instructions on demand with explicit parameters",
            category: ToolCategory::SkillLoader,
            params_schema: use_skill_schema(),
            default_enabled: true,
        },
        // Agent Control
        ToolMeta {
            name: "agent_complete",
            description: "Mark current task as done",
            category: ToolCategory::AgentControl,
            params_schema: agent_complete_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "agent_next",
            description: "Advance to next workflow step",
            category: ToolCategory::AgentControl,
            params_schema: agent_next_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "agent_feedback",
            description: "Send progress feedback message",
            category: ToolCategory::AgentControl,
            params_schema: agent_feedback_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "agent_set_path",
            description: "Set working directory for workflow",
            category: ToolCategory::AgentControl,
            params_schema: agent_set_path_schema(),
            default_enabled: true,
        },
        // Context
        ToolMeta {
            name: "read_tool_result",
            description: "Recall saved tool output without re-execution",
            category: ToolCategory::Context,
            params_schema: read_tool_result_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "get_context",
            description: "Read current context variable",
            category: ToolCategory::Context,
            params_schema: get_context_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "set_context",
            description: "Set context variable",
            category: ToolCategory::Context,
            params_schema: set_context_schema(),
            default_enabled: true,
        },
        // Memory
        ToolMeta {
            name: "memory_get",
            description: "Get memory value from profile",
            category: ToolCategory::Memory,
            params_schema: memory_get_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "memory_set",
            description: "Set memory value in profile",
            category: ToolCategory::Memory,
            params_schema: memory_set_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "learn_fact",
            description: "Store a fact in memory",
            category: ToolCategory::Memory,
            params_schema: learn_fact_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "learn_preference",
            description: "Store user preference",
            category: ToolCategory::Memory,
            params_schema: learn_preference_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "learn_topic",
            description: "Track conversation topic",
            category: ToolCategory::Memory,
            params_schema: learn_topic_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "memory_profile_create",
            description: "Create new memory profile",
            category: ToolCategory::Memory,
            params_schema: memory_profile_create_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "memory_profile_load",
            description: "Load memory profile",
            category: ToolCategory::Memory,
            params_schema: memory_profile_load_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "memory_profile_list",
            description: "List available memory profiles",
            category: ToolCategory::Memory,
            params_schema: memory_profile_list_schema(),
            default_enabled: true,
        },
        // Cron
        ToolMeta {
            name: "cron_add",
            description: "Create scheduled cron job",
            category: ToolCategory::Action,
            params_schema: cron_add_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "cron_delete",
            description: "Delete cron job",
            category: ToolCategory::Action,
            params_schema: cron_delete_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "cron_list",
            description: "List cron jobs",
            category: ToolCategory::Action,
            params_schema: cron_list_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "cron_toggle",
            description: "Enable/disable cron job",
            category: ToolCategory::Action,
            params_schema: cron_toggle_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "cron_run",
            description: "Manually trigger cron job",
            category: ToolCategory::Action,
            params_schema: cron_run_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "ask_questions",
            description: "Ask interactive questions (Discord/Web)",
            category: ToolCategory::Action,
            params_schema: ask_questions_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "understand_image",
            description: "Analyze image content",
            category: ToolCategory::Action,
            params_schema: understand_image_schema(),
            default_enabled: true,
        },
        ToolMeta {
            name: "update_template",
            description: "Strictly validate and save a POML template",
            category: ToolCategory::Action,
            params_schema: update_template_schema(),
            default_enabled: true,
        },
    ]
}

/// Access the static registry
pub fn all_tool_meta() -> &'static [ToolMeta] {
    &TOOL_REGISTRY
}

/// Get tool metadata by name
pub fn get_tool_meta(name: &str) -> Option<&'static ToolMeta> {
    TOOL_REGISTRY.iter().find(|m| m.name == name)
}

/// Get all tool names for a category
pub fn tool_names_by_category(category: ToolCategory) -> Vec<&'static str> {
    TOOL_REGISTRY
        .iter()
        .filter(|m| m.category == category)
        .map(|m| m.name)
        .collect()
}

/// Get tool names for multiple categories
pub fn tool_names_by_categories(categories: &[ToolCategory]) -> Vec<&'static str> {
    TOOL_REGISTRY
        .iter()
        .filter(|m| categories.contains(&m.category))
        .map(|m| m.name)
        .collect()
}

/// Filter tools for LLM request based on context settings
pub fn filter_tools_for_request(
    allowed_names: Option<&[String]>,
    allowed_categories: Option<&[ToolCategory]>,
    full_schema_names: Option<&[String]>,
    full_schema_categories: Option<&[ToolCategory]>,
    discovery_mode: ToolDiscoveryMode,
) -> Vec<crate::gateway::llm::provider::ToolDefinition> {
    use crate::gateway::llm::provider::{FunctionDefinition, ToolDefinition};

    let full_names: HashSet<&str> = full_schema_names.map(|v| v.iter().map(|s| s.as_str()).collect()).unwrap_or_default();
    let full_cats: HashSet<ToolCategory> = full_schema_categories.map(|v| v.iter().cloned().collect()).unwrap_or_default();

    TOOL_REGISTRY
        .iter()
        .filter(|meta| {
            // Filter by allowed names (if specified)
            if let Some(names) = allowed_names {
                if !names.iter().any(|n| n == meta.name) {
                    return false;
                }
            }
            // Filter by allowed categories (if specified)
            if let Some(cats) = allowed_categories {
                if !cats.contains(&meta.category) {
                    return false;
                }
            }
            true
        })
        .map(|meta| {
            // Determine if this tool gets full schema or empty
            let has_full_schema = full_names.contains(meta.name) || full_cats.contains(&meta.category);

            let params = if has_full_schema {
                meta.params_schema.clone()
            } else if discovery_mode == ToolDiscoveryMode::Full {
                meta.params_schema.clone()
            } else {
                // DescriptionOnly or None: empty schema for search_tools to work
                json!({"type": "object", "properties": {}})
            };

            ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: meta.name.into(),
                    description: meta.description.into(),
                    parameters: params,
                },
            }
        })
        .collect()
}

/// Parse comma-separated category strings into ToolCategory vec
pub fn parse_tool_categories(cats: &[String]) -> Vec<ToolCategory> {
    cats.iter().filter_map(|s| match s.as_str() {
        "Action" => Some(ToolCategory::Action),
        "Discovery" => Some(ToolCategory::Discovery),
        "SkillLoader" => Some(ToolCategory::SkillLoader),
        "AgentControl" => Some(ToolCategory::AgentControl),
        "Memory" => Some(ToolCategory::Memory),
        "Context" => Some(ToolCategory::Context),
        _ => None,
    }).collect()
}

/// Build filtered tool definitions from context settings, merging with plugin tools
pub fn build_tool_definitions(
    ctx_settings: &crate::db::contexts::ContextSettings,
    plugin_tools: Option<&[crate::gateway::llm::provider::ToolDefinition]>, // dynamic plugin tools
) -> Vec<crate::gateway::llm::provider::ToolDefinition> {
    // Expand tool_groups into full_tool_schemas
    let mut full_schemas = ctx_settings.full_tool_schemas.clone();
    if let Some(groups) = &ctx_settings.tool_groups {
        for group in groups {
            if let Some(tools) = TOOL_GROUPS.get(group.as_str()) {
                for tool in tools {
                    if !full_schemas.contains(&tool.to_string()) {
                        full_schemas.push(tool.to_string());
                    }
                }
            }
        }
    }
    let full_names = if full_schemas.is_empty() {
        None
    } else {
        Some(full_schemas.as_slice())
    };
    let full_cats = if ctx_settings.full_tool_categories.is_empty() {
        None
    } else {
        Some(parse_tool_categories(&ctx_settings.full_tool_categories))
    };
    let discovery_mode = match ctx_settings.tool_discovery_mode.as_str() {
        "Full" => ToolDiscoveryMode::Full,
        "None" => ToolDiscoveryMode::None,
        _ => ToolDiscoveryMode::DescriptionOnly,
    };
    // If no explicit filters, allow all default-enabled tools
    let allowed_names = None::<&[String]>;
    let allowed_cats = None::<&[ToolCategory]>;

    let mut tools = filter_tools_for_request(allowed_names, allowed_cats, full_names, full_cats.as_deref(), discovery_mode);
    
    // Merge plugin tools - also filter them by state settings
    if let Some(plugins) = plugin_tools {
        for plugin_tool in plugins {
            // Check if tool already exists (static registry override)
            if tools.iter().any(|t| t.function.name == plugin_tool.function.name) {
                continue;
            }
            // Apply same state-based filtering to plugin tools
            // Plugin tools are treated as "Plugin" category
            let plugin_category = ToolCategory::Discovery; // or add a Plugin category
            
            // Check if plugin tool is allowed by state settings
            let is_allowed = if full_names.is_some() {
                // Check if in full_tool_schemas
                full_names.as_ref().unwrap().iter().any(|s| s.as_str() == plugin_tool.function.name.as_str())
            } else if full_cats.is_some() {
                // Check if Plugin category is in full_tool_categories
                full_cats.as_ref().unwrap().contains(&ToolCategory::Discovery)
            } else {
                // No explicit full schema config - use discovery mode
                matches!(discovery_mode, ToolDiscoveryMode::Full)
            };
            
            if is_allowed {
                let params = if discovery_mode == ToolDiscoveryMode::Full {
                    plugin_tool.function.parameters.clone()
                } else {
                    json!({"type": "object", "properties": {}})
                };
                tools.push(crate::gateway::llm::provider::ToolDefinition {
                    tool_type: "function".into(),
                    function: crate::gateway::llm::provider::FunctionDefinition {
                        name: plugin_tool.function.name.clone(),
                        description: plugin_tool.function.description.clone(),
                        parameters: params,
                    },
                });
            } else if discovery_mode == ToolDiscoveryMode::None {
                // Skip entirely
                continue;
            } else {
                // DescriptionOnly mode - add with empty schema
                tools.push(crate::gateway::llm::provider::ToolDefinition {
                    tool_type: "function".into(),
                    function: crate::gateway::llm::provider::FunctionDefinition {
                        name: plugin_tool.function.name.clone(),
                        description: plugin_tool.function.description.clone(),
                        parameters: json!({"type": "object", "properties": {}}),
                    },
                });
            }
        }
    }
    tools
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryMode {
    /// Full parameters for all allowed tools
    Full,
    /// Only name + description in schema (minimal payload)
    DescriptionOnly,
    /// No tools in request at all
    None,
}

impl Default for ToolDiscoveryMode {
    fn default() -> Self {
        ToolDiscoveryMode::DescriptionOnly
    }
}

// Schema functions for each tool
fn terminal_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "Shell command to execute"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["command"]
    })
}

fn read_file_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path to read"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["path"]
    })
}

fn write_file_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path to write"},
            "content": {"type": "string", "description": "Content to write"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["path", "content"]
    })
}

fn edit_file_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path to edit"},
            "old_string": {"type": "string", "description": "Text to find and replace"},
            "new_string": {"type": "string", "description": "Replacement text"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["path", "old_string", "new_string"]
    })
}

fn discord_send_message_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "channel_id": {"type": "string", "description": "Discord channel ID"},
            "content": {"type": "string", "description": "Message content"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["channel_id", "content"]
    })
}

fn discord_send_embed_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "channel_id": {"type": "string", "description": "Discord channel ID"},
            "title": {"type": "string"},
            "description": {"type": "string"},
            "color": {"type": "integer"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["channel_id"]
    })
}

fn discord_upload_file_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "channel_id": {"type": "string", "description": "Discord channel ID"},
            "file_path": {"type": "string", "description": "Local file path to upload"},
            "filename": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["channel_id", "file_path"]
    })
}

fn vm_start_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "cpu_cores": {"type": "integer"},
            "ram_mb": {"type": "integer"},
            "disk_size": {"type": "string"},
            "arch": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn vm_stop_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn vm_shell_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "command": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "command"]
    })
}

fn vm_keys_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "keys": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "keys"]
    })
}

fn vm_mouse_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "action": {"type": "string"},
            "x": {"type": "integer"},
            "y": {"type": "integer"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "action"]
    })
}

fn vm_screenshot_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn vm_file_transfer_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "direction": {"type": "string", "enum": ["to_vm", "from_vm"]},
            "src": {"type": "string"},
            "dst": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "direction", "src", "dst"]
    })
}

fn vm_snapshot_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "snapshot_name": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn vm_shared_folder_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "host_path": {"type": "string"},
            "vm_path": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn understand_image_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "image_path": {"type": "string"},
            "prompt": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["image_path", "prompt"]
    })
}

fn update_template_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "content": {"type": "string"},
            "user_id": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "content"]
    })
}

fn search_tools_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Keywords to search tools"},
            "limit": {"type": "integer", "default": 5, "minimum": 1, "maximum": 20},
            "replace": {"type": "boolean", "default": false},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["query"]
    })
}

fn search_skills_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Keywords to search skills"},
            "limit": {"type": "integer", "default": 5, "minimum": 1, "maximum": 20},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["query"]
    })
}

fn use_skill_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Exact registered skill name", "maxLength": 64},
            "parameters": {"type": "object", "description": "Required non-empty string inputs advertised by discovery"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "parameters"]
    })
}

fn agent_complete_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn agent_next_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn agent_feedback_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "message": {"type": "string", "description": "Feedback message"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["message"]
    })
}

fn agent_set_path_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Working directory path"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["path"]
    })
}

fn read_tool_result_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "output_id": {"type": "string", "description": "out_... reference from a previous tool response"},
            "view": {"type": "string", "enum": ["full", "lines", "tail", "search"], "default": "full"},
            "start_line": {"type": "integer", "default": 1, "minimum": 1},
            "line_count": {"type": "integer", "default": 100, "minimum": 1, "maximum": 2000},
            "max_chars": {"type": "integer", "minimum": 1, "maximum": 32000},
            "query": {"type": "string", "minLength": 1, "maxLength": 256},
            "json_pointer": {"type": "string", "maxLength": 1024},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["output_id"]
    })
}

fn get_context_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "key": {"type": "string", "description": "Context key to read"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["key"]
    })
}

fn set_context_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "key": {"type": "string", "description": "Context key to set"},
            "value": {"description": "Value to set"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["key", "value"]
    })
}

fn memory_get_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {"type": "string", "description": "Memory profile name"},
            "key": {"type": "string", "description": "Memory key"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["profile", "key"]
    })
}

fn memory_set_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {"type": "string", "description": "Memory profile name"},
            "key": {"type": "string", "description": "Memory key"},
            "value": {"description": "Value to set"},
            "expected_profile": {"type": "string", "description": "Expected profile for conflict detection"},
            "expected_value": {"description": "Expected current value for conflict detection"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["profile", "key", "value"]
    })
}

fn learn_fact_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "fact": {"type": "string", "description": "Fact to remember"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["fact"]
    })
}

fn learn_preference_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "key": {"type": "string", "description": "Preference key"},
            "value": {"description": "Preference value"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["key"]
    })
}

fn learn_topic_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "topic": {"type": "string", "description": "Topic to track"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["topic"]
    })
}

fn memory_profile_create_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Profile name"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name"]
    })
}

fn memory_profile_load_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Profile name"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name"]
    })
}

fn memory_profile_list_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn cron_add_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "schedule": {"type": "string"},
            "prompt": {"type": "string"},
            "template": {"type": "string"},
            "timezone": {"type": "string"},
            "description": {"type": "string"},
            "enabled": {"type": "boolean"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["name", "schedule", "prompt"]
    })
}

fn cron_delete_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "job_id": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["job_id"]
    })
}

fn cron_list_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        }
    })
}

fn cron_toggle_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "job_id": {"type": "string"},
            "enabled": {"type": "boolean"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["job_id", "enabled"]
    })
}

fn cron_run_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "job_id": {"type": "string"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["job_id"]
    })
}

fn ask_questions_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": {"type": "string"},
                        "question": {"type": "string"},
                        "suggestions": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["label", "question"]
                }
            },
            "channel_id": {"type": "string"},
            "timeout_secs": {"type": "integer"},
            "_output": {"$ref": "#/components/schemas/OutputSelection"}
        },
        "required": ["questions"]
    })
}
