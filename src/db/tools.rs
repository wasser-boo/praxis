use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
    pub is_enabled: bool,
}

fn tools_file(db: &Database) -> std::path::PathBuf {
    db.data_dir().join("tools.json")
}

fn load_tools(db: &Database) -> anyhow::Result<Vec<Tool>> {
    let path = tools_file(db);
    if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data)?)
    } else {
        Ok(Vec::new())
    }
}

fn save_tools(db: &Database, tools: &[Tool]) -> anyhow::Result<()> {
    let path = tools_file(db);
    let json = serde_json::to_string_pretty(tools)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn list_enabled(db: &Database) -> anyhow::Result<Vec<Tool>> {
    let tools = load_tools(db)?;
    Ok(tools.into_iter().filter(|t| t.is_enabled).collect())
}

pub fn list(db: &Database) -> anyhow::Result<Vec<Tool>> {
    load_tools(db)
}

pub fn get(db: &Database, name: &str) -> anyhow::Result<Tool> {
    let tools = load_tools(db)?;
    tools
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))
}

pub fn save(db: &Database, tool: &Tool) -> anyhow::Result<()> {
    let mut tools = load_tools(db)?;
    if let Some(existing) = tools.iter_mut().find(|t| t.name == tool.name) {
        *existing = tool.clone();
    } else {
        tools.push(tool.clone());
    }
    save_tools(db, &tools)?;
    Ok(())
}

pub fn enable(db: &Database, name: &str) -> anyhow::Result<()> {
    let mut tools = load_tools(db)?;
    if let Some(tool) = tools.iter_mut().find(|t| t.name == name) {
        tool.is_enabled = true;
        save_tools(db, &tools)?;
    }
    Ok(())
}

pub fn disable(db: &Database, name: &str) -> anyhow::Result<()> {
    let mut tools = load_tools(db)?;
    if let Some(tool) = tools.iter_mut().find(|t| t.name == name) {
        tool.is_enabled = false;
        save_tools(db, &tools)?;
    }
    Ok(())
}

pub fn set_enabled(db: &Database, name: &str, enabled: bool) -> anyhow::Result<()> {
    let mut tools = load_tools(db)?;
    if let Some(tool) = tools.iter_mut().find(|t| t.name == name) {
        tool.is_enabled = enabled;
        save_tools(db, &tools)?;
    }
    Ok(())
}

pub fn init_default_tools(db: &Database) -> anyhow::Result<()> {
    let existing = load_tools(db)?;
    if existing.is_empty() {
        // Fresh install: write all defaults
        let defaults = get_default_tools();
        save_tools(db, &defaults)?;
        return Ok(());
    }

    // Existing install: add any missing default tools (preserves user customizations)
    let mut tools = existing;
    let defaults = get_default_tools();
    let mut changed = false;
    for default in &defaults {
        if !tools.iter().any(|t| t.name == default.name) {
            tracing::info!("Adding missing default tool: {}", default.name);
            tools.push(default.clone());
            changed = true;
        }
    }
    if changed {
        save_tools(db, &tools)?;
    }
    Ok(())
}

fn get_default_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "execute_terminal".into(),
            description: Some("Run shell command".into()),
            parameters: serde_json::json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            is_enabled: true,
        },
        Tool {
            name: "write_file".into(),
            description: Some("Create or overwrite file".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            is_enabled: true,
        },
        Tool {
            name: "edit_file".into(),
            description: Some("Edit file with find/replace".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"}},"required":["path","old_string","new_string"]}),
            is_enabled: true,
        },
        Tool {
            name: "read_file".into(),
            description: Some("Read file contents".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            is_enabled: true,
        },
        Tool {
            name: "get_context".into(),
            description: Some("Read current context".into()),
            parameters: serde_json::json!({"type":"object","properties":{}}),
            is_enabled: true,
        },
        Tool {
            name: "set_context".into(),
            description: Some("Set context variable".into()),
            parameters: serde_json::json!({"type":"object","properties":{"key":{"type":"string"},"value":{}},"required":["key","value"]}),
            is_enabled: true,
        },
        Tool {
            name: "delete_context".into(),
            description: Some("Delete context variable".into()),
            parameters: serde_json::json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}),
            is_enabled: true,
        },
        Tool {
            name: "agent_next".into(),
            description: Some("Advance to next step".into()),
            parameters: serde_json::json!({"type":"object","properties":{}}),
            is_enabled: true,
        },
        Tool {
            name: "agent_complete".into(),
            description: Some("Mark task as done".into()),
            parameters: serde_json::json!({"type":"object","properties":{}}),
            is_enabled: true,
        },
        Tool {
            name: "agent_set_path".into(),
            description: Some("Set working directory".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            is_enabled: true,
        },
        Tool {
            name: "agent_feedback".into(),
            description: Some("Send progress update".into()),
            parameters: serde_json::json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"]}),
            is_enabled: true,
        },
        Tool {
            name: "discord_upload_file".into(),
            description: Some("Upload file to Discord channel. Provide channel_id, filename (display name), and base64_content (base64-encoded file data).".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string","description":"Discord channel ID"},"filename":{"type":"string","description":"Display filename for the attachment"},"base64_content":{"type":"string","description":"Base64-encoded file content"},"message":{"type":"string","description":"Optional message text"}},"required":["channel_id","filename","base64_content"]}),
            is_enabled: true,
        },
        Tool {
            name: "discord_send_message".into(),
            description: Some("Send message to Discord channel".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string"},"message":{"type":"string"}},"required":["channel_id","message"]}),
            is_enabled: true,
        },
        Tool {
            name: "discord_send_embed".into(),
            description: Some("Send rich embed to Discord channel".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},"url":{"type":"string"},"color":{"type":["string","number"],"description":"Hex color (e.g. '6C5CE7' or '#FF0000') or integer"},"footer":{"type":"string"},"author":{"type":"string"},"thumbnail":{"type":"string","description":"URL to thumbnail image"},"image":{"type":"string","description":"URL to full image"},"fields":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"value":{"type":"string"},"inline":{"type":"boolean"}},"required":["name","value"]}}},"required":["channel_id"]}),
            is_enabled: true,
        },
        Tool {
            name: "learn_fact".into(),
            description: Some("Learn and store a fact".into()),
            parameters: serde_json::json!({"type":"object","properties":{"fact":{"type":"string"}},"required":["fact"]}),
            is_enabled: true,
        },
        Tool {
            name: "learn_preference".into(),
            description: Some("Learn a user preference".into()),
            parameters: serde_json::json!({"type":"object","properties":{"key":{"type":"string"},"value":{"type":"string"}},"required":["key","value"]}),
            is_enabled: true,
        },
        Tool {
            name: "learn_topic".into(),
            description: Some("Track a conversation topic".into()),
            parameters: serde_json::json!({"type":"object","properties":{"topic":{"type":"string"}},"required":["topic"]}),
            is_enabled: true,
        },
        Tool {
            name: "understand_image".into(),
            description: Some("Load an image for visual analysis. The image is sent to the vision API - you WILL see and understand the image content in your next response. After calling this tool, describe what you see in the image. Use for screenshots, photos, or any visual content.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Path to the image file (supports PPM, PNG, JPEG)"}},"required":["path"]}),
            is_enabled: true,
        },
        // VM Tools (only enabled when VM=true)
        Tool {
            name: "vm_start".into(),
            description: Some("Start a QEMU VM. Creates a new VM if none exists. The VM runs a full Linux environment you control.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm","description":"VM name"},"cpu_cores":{"type":"integer","default":2},"ram_mb":{"type":"integer","default":4096},"disk_size":{"type":"string","default":"40G"},"iso_path":{"type":"string","description":"Path to ISO for OS installation"},"arch":{"type":"string","enum":["x86_64","aarch64"],"default":"x86_64"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_stop".into(),
            description: Some("Stop a running VM".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"},"force":{"type":"boolean","default":false,"description":"Force kill instead of graceful shutdown"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_shell".into(),
            description: Some("Execute a shell command inside the VM. Returns stdout, stderr, and exit code. The VM has bash and you have full root access.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"},"name":{"type":"string","default":"praxis-vm"},"timeout_secs":{"type":"integer","default":30}},"required":["command"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_keys".into(),
            description: Some("Send keyboard input to the VM. Use this for TUI apps (vim, htop, nano, etc) and installer menus. Special keys: 'enter', 'esc', 'tab', 'backspace', 'space', 'delete', 'insert', 'home', 'end', 'pageup', 'pagedown', 'capslock', 'numlock', 'print_screen', 'arrow_up', 'arrow_down', 'arrow_left', 'arrow_right', 'f1'-'f24', 'super'/'meta'/'win'. Numpad: 'kp0'-'kp9', 'kp_enter', 'kp_plus', 'kp_minus', 'kp_multiply', 'kp_divide', 'kp_dot'. Symbols: - = [ ] \\ ; ' ` , . / and shifted: ! @ # $ % ^ & * ( ) _ + { } | : \" ~ < > ?. Letters a-z, digits 0-9. Modifier combos: 'ctrl+a'-'ctrl+z', 'alt+f1'-'alt+f12', 'alt+tab', 'alt+enter', 'ctrl+alt+delete', 'ctrl+alt+f1'-'ctrl+alt+f6'. Regular text strings (e.g. 'ls -la', 'hello') are sent as-is via serial.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"keys":{"type":"string","description":"Text or special key to send"},"name":{"type":"string","default":"praxis-vm"}},"required":["keys"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_screenshot".into(),
            description: Some("Take a screenshot of the VM display. Returns base64-encoded image. Use this to see what's on the VM screen (GUI, TUI apps, etc).".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_file_transfer".into(),
            description: Some("Transfer a file to/from the VM. Use direction='to_vm' to upload or 'from_vm' to download. Content is base64 encoded.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path in VM"},"content":{"type":"string","description":"File content (base64 for binary, plain text for text)"},"direction":{"type":"string","enum":["to_vm","from_vm"],"default":"to_vm"},"name":{"type":"string","default":"praxis-vm"}},"required":["path"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_snapshot".into(),
            description: Some("Create a VM snapshot for later restore. Great for saving state before risky operations.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"snapshot_name":{"type":"string","description":"Name for the snapshot"},"name":{"type":"string","default":"praxis-vm"}},"required":["snapshot_name"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_shared_folder".into(),
            description: Some("Add a shared folder between host and VM. Files in host_path will be accessible at mount_point inside the VM. Requires VM restart.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"host_path":{"type":"string","description":"Path on the host machine"},"mount_point":{"type":"string","description":"Mount path inside VM","default":"/mnt/shared"},"readonly":{"type":"boolean","default":false},"name":{"type":"string","default":"praxis-vm"}},"required":["host_path"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse".into(),
            description: Some("Control the mouse in the VM. Actions: move_absolute (x,y), move_relative (dx,dy), click (x,y,button), double_click, drag (x,y to dx,dy), scroll (vertical/horizontal). Button: 0=left, 1=middle, 2=right.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["move_absolute","move_relative","click","double_click","drag","scroll"]},"x":{"type":"integer"},"y":{"type":"integer"},"dx":{"type":"integer"},"dy":{"type":"integer"},"button":{"type":"integer","description":"0=left,1=middle,2=right","default":0},"scroll_vertical":{"type":"integer","description":"Positive=up, negative=down"},"scroll_horizontal":{"type":"integer","description":"Positive=left, negative=right"},"name":{"type":"string","default":"praxis-vm"}},"required":["action"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_look_screenshot".into(),
            description: Some("Look at a saved VM screenshot. index: -1=latest, 0=oldest, N=specific screenshot number. Returns base64 image. Max 500 screenshots are kept (oldest auto-deleted).".into()),
            parameters: serde_json::json!({"type":"object","properties":{"index":{"type":"integer","description":"-1=latest, 0=oldest, N=specific","default":-1},"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_install".into(),
            description: Some("Start a VM with an installation ISO to install an OS. Provide either iso_name (searches in installation_disks context) or iso_path (direct path). The VM boots from the ISO. Use vm_keys and vm_screenshot to complete the installation.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"iso_name":{"type":"string","description":"Name to search for in installation_disks (e.g. 'alpine', 'ubuntu', 'arch')"},"iso_path":{"type":"string","description":"Direct path to ISO file (alternative to iso_name)"},"vm_name":{"type":"string","default":"praxis-vm"},"cpu_cores":{"type":"integer","default":2},"ram_mb":{"type":"integer","default":4096},"disk_size":{"type":"string","default":"40G"}}}),
            is_enabled: false,
        },
        Tool {
            name: "send_screenshot_to_discord".into(),
            description: Some("Take a VM screenshot and send it to a Discord channel. Use this to show the user what's happening on the VM desktop.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string","description":"Discord channel ID"},"caption":{"type":"string","description":"Optional caption for the screenshot"},"vm_name":{"type":"string","default":"praxis-vm"}},"required":["channel_id"]}),
            is_enabled: false,
        },
        Tool {
            name: "screenshot_with_feedback".into(),
            description: Some("Take a VM screenshot and send it to Discord with a message/feedback. Use this to show progress and ask for input.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string","description":"Discord channel ID"},"feedback":{"type":"string","description":"Message to send with the screenshot"},"vm_name":{"type":"string","default":"praxis-vm"}},"required":["channel_id","feedback"]}),
            is_enabled: false,
        },
        Tool {
            name: "ask_questions".into(),
            description: Some("Ask the user one or more questions sequentially via Discord and wait for responses. Each question is asked one after another. The user can react with emojis (3s debounce) or type their answer. Returns a JSON object mapping each label to its answer. Works for a single question too - just provide one entry in the questions array.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string","description":"Discord channel ID"},"questions":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string","description":"Short label for this answer (e.g. 'username', 'choice')"},"question":{"type":"string","description":"The question text"},"suggestions":{"type":"array","items":{"type":"string"},"description":"Quick reply options (shown as emoji buttons)"}},"required":["label","question"]},"description":"Array of one or more questions to ask sequentially"},"timeout_secs":{"type":"integer","default":120,"description":"Timeout per individual question (seconds)"}},"required":["channel_id","questions"]}),
            is_enabled: false,
        },
    ]
}

/// Convert enabled tools to LLM tool definitions
pub fn to_tool_definitions(
    db: &Database,
) -> anyhow::Result<Vec<crate::gateway::llm::provider::ToolDefinition>> {
    let tools = list_enabled(db)?;
    Ok(tools
        .into_iter()
        .map(|t| crate::gateway::llm::provider::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::gateway::llm::provider::FunctionDefinition {
                name: t.name,
                description: t.description.unwrap_or_default(),
                parameters: t.parameters,
            },
        })
        .collect())
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn test_init_default_tools() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        let tools = list(&db).unwrap();
        assert_eq!(tools.len(), 31);
    }

    #[test]
    fn test_init_default_tools_idempotent() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        init_default_tools(&db).unwrap();
        let tools = list(&db).unwrap();
        assert_eq!(tools.len(), 31);
    }

    #[test]
    fn test_list_enabled() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        let enabled = list_enabled(&db).unwrap();
        assert_eq!(enabled.len(), 18); // 18 enabled (14 VM/discord tools disabled by default)

        disable(&db, "execute_terminal").unwrap();
        let enabled = list_enabled(&db).unwrap();
        assert_eq!(enabled.len(), 17);
    }

    #[test]
    fn test_get_tool() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        let tool = get(&db, "execute_terminal").unwrap();
        assert_eq!(tool.name, "execute_terminal");
        assert!(tool.is_enabled);
    }

    #[test]
    fn test_get_tool_not_found() {
        let (db, _dir) = test_db();
        assert!(get(&db, "nonexistent").is_err());
    }

    #[test]
    fn test_enable_disable() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();

        disable(&db, "write_file").unwrap();
        let tool = get(&db, "write_file").unwrap();
        assert!(!tool.is_enabled);

        enable(&db, "write_file").unwrap();
        let tool = get(&db, "write_file").unwrap();
        assert!(tool.is_enabled);
    }

    #[test]
    fn test_set_enabled() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();

        set_enabled(&db, "edit_file", false).unwrap();
        let tool = get(&db, "edit_file").unwrap();
        assert!(!tool.is_enabled);

        set_enabled(&db, "edit_file", true).unwrap();
        let tool = get(&db, "edit_file").unwrap();
        assert!(tool.is_enabled);
    }

    #[test]
    fn test_save_custom_tool() {
        let (db, _dir) = test_db();
        let tool = Tool {
            name: "custom_tool".to_string(),
            description: Some("My custom tool".to_string()),
            parameters: serde_json::json!({"type": "object"}),
            is_enabled: true,
        };
        save(&db, &tool).unwrap();

        let loaded = get(&db, "custom_tool").unwrap();
        assert_eq!(loaded.description, Some("My custom tool".to_string()));
    }

    #[test]
    fn test_save_update_existing() {
        let (db, _dir) = test_db();
        let tool = Tool {
            name: "my_tool".to_string(),
            description: Some("v1".to_string()),
            parameters: serde_json::json!({}),
            is_enabled: true,
        };
        save(&db, &tool).unwrap();

        let tool2 = Tool {
            name: "my_tool".to_string(),
            description: Some("v2".to_string()),
            parameters: serde_json::json!({}),
            is_enabled: false,
        };
        save(&db, &tool2).unwrap();

        let loaded = get(&db, "my_tool").unwrap();
        assert_eq!(loaded.description, Some("v2".to_string()));
        assert!(!loaded.is_enabled);
    }

    #[test]
    fn test_to_tool_definitions() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        let defs = to_tool_definitions(&db).unwrap();
        assert_eq!(defs.len(), 18); // 18 enabled (14 VM/discord tools disabled by default)
        assert_eq!(defs[0].function.name, "execute_terminal");
    }

    #[test]
    fn test_to_tool_definitions_filtered() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        disable(&db, "execute_terminal").unwrap();
        disable(&db, "write_file").unwrap();

        let defs = to_tool_definitions(&db).unwrap();
        assert_eq!(defs.len(), 16);
        assert!(defs.iter().all(|d| d.function.name != "execute_terminal"));
    }
}
