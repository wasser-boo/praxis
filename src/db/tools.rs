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
        // RAG Tools
        Tool {
            name: "rag_search".into(),
            description: Some("Search knowledge base for relevant information. Returns the most similar document chunks.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Max results (default 5)","default":5}},"required":["query"]}),
            is_enabled: true,
        },
        Tool {
            name: "rag_ingest".into(),
            description: Some("Add a document to the knowledge base. Chunks the text and computes embeddings for later retrieval.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Document filename"},"content":{"type":"string","description":"Document content"},"file_type":{"type":"string","description":"File type (e.g., 'txt', 'md', 'pdf')","default":"txt"}},"required":["filename","content"]}),
            is_enabled: true,
        },
        Tool {
            name: "rag_list".into(),
            description: Some("List all documents in the knowledge base.".into()),
            parameters: serde_json::json!({"type":"object","properties":{}}),
            is_enabled: true,
        },
        Tool {
            name: "rag_delete".into(),
            description: Some("Delete a document from the knowledge base by ID.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"document_id":{"type":"string","description":"Document ID to delete"}},"required":["document_id"]}),
            is_enabled: true,
        },
        Tool {
            name: "understand_image".into(),
            description: Some("Load an image for visual analysis. The image is sent to the vision API - you WILL see and understand the image content in your next response. After calling this tool, describe what you see in the image. Use for screenshots, photos, or any visual content.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Path to the image file (supports PPM, PNG, JPEG)"},"prompt":{"type":"string","description":"What to look for in the image. E.g. 'What menu options are shown?' or 'Describe the installation step shown'."}},"required":["path","prompt"]}),
            is_enabled: true,
        },
        // VM Tools (only enabled when VM=true)
        Tool {
            name: "vm_start".into(),
            description: Some("Start a QEMU VM. Creates a new VM if none exists. The VM runs a full Linux environment you control.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm","description":"VM name"},"cpu_cores":{"type":"integer","default":2},"ram_mb":{"type":"integer","default":4096},"disk_size":{"type":"string","default":"40G"},"iso_path":{"type":"string","description":"Path to ISO for OS installation"},"arch":{"type":"string","enum":["x86_64","aarch64"],"default":"x86_64"},"keyboard_layout":{"type":"string","enum":["us","de","fr","es","it","gb"],"default":"us","description":"Keyboard layout for the VM (us, de, fr, es, it, gb)"},"firmware":{"type":"string","enum":["bios","uefi"],"default":"bios","description":"Boot firmware: 'bios' (legacy) or 'uefi' (OVMF). Use 'uefi' for modern OSes that require EFI boot."}}}),
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
            description: Some("Send keyboard input to the VM. Supports:\n\
                - Text: 'root', 'ls -la', 'hello world'\n\
                - Special keys: 'enter', 'esc', 'tab', 'backspace', 'space', 'delete', 'insert', 'home', 'end', 'pageup', 'pagedown'\n\
                - Arrow keys: 'arrow_up', 'arrow_down', 'arrow_left', 'arrow_right'\n\
                - Function keys: 'f1'-'f24'\n\
                - Modifiers: 'ctrl', 'alt', 'shift', 'super'/'meta'/'win'\n\
                - Numpad: 'kp0'-'kp9', 'kp_enter', 'kp_plus', 'kp_minus', 'kp_multiply', 'kp_divide', 'kp_dot'\n\
                - Symbols: - = [ ] \\ ; ' ` , . / and shifted: ! @ # $ % ^ & * ( ) _ + { } | : \" ~ < > ?\n\
                - Modifier combos: 'ctrl+a', 'alt+tab', 'ctrl+alt+delete'\n\
                - Macros (repeat): '(key)count' - e.g. '(arrowdown)5' presses down 5 times, '(enter)3' presses enter 3 times, '(tab)2' presses tab twice\n\
                - Newlines '\\n' are converted to enter key\n\
                - Use keyboard_layout parameter to set VM keyboard layout (us, de, fr, es, it, gb)".into()),
            parameters: serde_json::json!({"type":"object","properties":{"keys":{"type":"string","description":"Text, special key, or macro. Examples: 'root\\n', '(arrowdown)5', 'ctrl+c', 'enter'"},"name":{"type":"string","default":"praxis-vm"},"keyboard_layout":{"type":"string","enum":["us","de","fr","es","it","gb"],"default":"us","description":"Keyboard layout for this input"}},"required":["keys"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_input".into(),
            description: Some("High-level VM input tool for easy navigation. Vim-like shortcuts:\n\
                - Navigation: up/k, down/j, left/h, right/l (with count)\n\
                - Pages: pageup/pgup, pagedown/pgdn, home, end\n\
                - Tab: tab, shifttab/backtab\n\
                - Actions: enter/confirm/select, esc/cancel/back, space/toggle, backspace/delete\n\
                - Type: type (text), typeenter (text+enter), num (number+enter)\n\
                - Function keys: f1-f12\n\
                - Ctrl: ctrl_c (interrupt), ctrl_z, ctrl_a, ctrl_l (clear)\n\
                - Wait: wait (ms parameter)\n\
                - Bash: bash (command) - Execute command directly via serial, returns output\n\
                - File: write_file (path, content) - Write file directly to VM\n\
                - Status: system_status - Check keymap, locale, network, disk, memory, services\n\
                - Service: restart_service (service) - Restart a Void Linux service\n\
                - Use keyboard_layout parameter to set VM keyboard layout (us, de, fr, es, it, gb)\n\
                Examples: {action:'bash', command:'ls -la'}, {action:'write_file', path:'/tmp/test.txt', content:'hello'}, {action:'system_status'}".into()),
            parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"Action: up/down/left/right, tab/shifttab, enter/confirm, esc/cancel, space/toggle, type, typeenter, num, f1-f12, ctrl_c, wait"},"text":{"type":"string","description":"Text for type/typeenter/num actions"},"count":{"type":"integer","default":1,"description":"Repeat count for navigation"},"ms":{"type":"integer","default":1000,"description":"Wait time in ms for wait action"},"enter":{"type":"boolean","default":false,"description":"Press enter after type"},"name":{"type":"string","default":"praxis-vm"},"keyboard_layout":{"type":"string","enum":["us","de","fr","es","it","gb"],"default":"us","description":"Keyboard layout for this input"}},"required":["action"]}),
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
            name: "vm_process_list".into(),
            description: Some("List running processes in the VM. Returns PID, user, CPU%, MEM%, and command. Use to see what's running.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_file_read".into(),
            description: Some("Read a file from the VM and return its content. Use for config files, logs, etc.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path to read"},"name":{"type":"string","default":"praxis-vm"}},"required":["path"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_network_test".into(),
            description: Some("Test network connectivity in the VM. Actions: ping (host), curl (url), dns (domain), interfaces, routes.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["ping","curl","dns","interfaces","routes"],"default":"ping"},"target":{"type":"string","description":"Host/URL/domain to test"},"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_service_list".into(),
            description: Some("List all services in the VM and their status (Void Linux runit). Shows running/stopped services.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_package_install".into(),
            description: Some("Install packages in the VM using xbps-install (Void Linux). Updates package list and installs specified packages.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"packages":{"type":"string","description":"Space-separated package names to install"},"name":{"type":"string","default":"praxis-vm"}},"required":["packages"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_snapshot_list".into(),
            description: Some("List all snapshots for the VM. Shows snapshot names and creation info.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_snapshot_restore".into(),
            description: Some("Restore a VM snapshot. The VM will be reset to the state when the snapshot was created.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"snapshot_name":{"type":"string","description":"Name of the snapshot to restore"},"name":{"type":"string","default":"praxis-vm"}},"required":["snapshot_name"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_snapshot_delete".into(),
            description: Some("Delete a VM snapshot.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"snapshot_name":{"type":"string","description":"Name of the snapshot to delete"},"name":{"type":"string","default":"praxis-vm"}},"required":["snapshot_name"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_cron_add".into(),
            description: Some("Add a cron job in the VM. Schedule examples: '@reboot', '0 2 * * *' (daily at 2am), '*/5 * * * *' (every 5 minutes).".into()),
            parameters: serde_json::json!({"type":"object","properties":{"schedule":{"type":"string","description":"Cron schedule expression"},"command":{"type":"string","description":"Command to execute"},"name":{"type":"string","default":"praxis-vm"}},"required":["schedule","command"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_cron_list".into(),
            description: Some("List all cron jobs in the VM.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_cron_remove".into(),
            description: Some("Remove a cron job from the VM by its command or schedule.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Pattern to match cron job (command or schedule)"},"name":{"type":"string","default":"praxis-vm"}},"required":["pattern"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_shortcut".into(),
            description: Some("Send common keyboard shortcuts to the VM. Predefined shortcuts:\n\
                - copy, paste, cut, select_all, undo, redo\n\
                - save, open, new, close, quit\n\
                - find, replace, print\n\
                - alt_tab, alt_f4, ctrl_alt_delete\n\
                - minimize, maximize, fullscreen\n\
                - volume_up, volume_down, mute\n\
                - brightness_up, brightness_down".into()),
            parameters: serde_json::json!({"type":"object","properties":{"shortcut":{"type":"string","description":"Shortcut name (e.g. 'copy', 'paste', 'alt_tab')"},"name":{"type":"string","default":"praxis-vm"}},"required":["shortcut"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_key_combo".into(),
            description: Some("Send arbitrary key combinations to the VM. Use QEMU key names.\n\
                Examples: 'ctrl+shift+t', 'alt+F2', 'super+l', 'ctrl+alt+delete'\n\
                Supports modifiers: ctrl, alt, shift, super/meta/win\n\
                Supports keys: a-z, 0-9, f1-f12, enter, esc, tab, space, backspace, delete, arrows, etc.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"combo":{"type":"string","description":"Key combination (e.g. 'ctrl+shift+t')"},"repeat":{"type":"integer","default":1,"description":"Number of times to repeat"},"name":{"type":"string","default":"praxis-vm"}},"required":["combo"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_type_fast".into(),
            description: Some("Type text quickly into the VM. Uses faster key delays for installers and text fields. Optionally press Enter after.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string","description":"Text to type"},"enter":{"type":"boolean","default":false,"description":"Press Enter after typing"},"name":{"type":"string","default":"praxis-vm"},"keyboard_layout":{"type":"string","enum":["us","de","fr","es","it","gb"],"default":"us"}},"required":["text"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_wait_for_text".into(),
            description: Some("Wait for specific text to appear on the VM screen. Takes a screenshot and checks if the text is present. Useful for waiting for boot/installation completion.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string","description":"Text to wait for"},"timeout_secs":{"type":"integer","default":60,"description":"Maximum wait time in seconds"},"interval_secs":{"type":"integer","default":5,"description":"Check interval in seconds"},"name":{"type":"string","default":"praxis-vm"}},"required":["text"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_window_list".into(),
            description: Some("List open windows in the VM (if running a desktop environment). Uses wmctrl or xdotool.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_window_focus".into(),
            description: Some("Focus/activate a window in the VM by title or ID.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"window":{"type":"string","description":"Window title or ID to focus"},"name":{"type":"string","default":"praxis-vm"}},"required":["window"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_clipboard_set".into(),
            description: Some("Set clipboard content in the VM. Requires xclip or xsel.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"content":{"type":"string","description":"Content to set in clipboard"},"name":{"type":"string","default":"praxis-vm"}},"required":["content"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_clipboard_get".into(),
            description: Some("Get clipboard content from the VM. Requires xclip or xsel.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"name":{"type":"string","default":"praxis-vm"}}}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse_move".into(),
            description: Some("Move mouse to absolute position in the VM. Coordinates are in pixels.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"x":{"type":"integer","description":"X coordinate"},"y":{"type":"integer","description":"Y coordinate"},"name":{"type":"string","default":"praxis-vm"}},"required":["x","y"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse_click_at".into(),
            description: Some("Click at specific position in the VM. Button: 0=left, 1=middle, 2=right.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"x":{"type":"integer","description":"X coordinate"},"y":{"type":"integer","description":"Y coordinate"},"button":{"type":"integer","default":0,"description":"0=left, 1=middle, 2=right"},"name":{"type":"string","default":"praxis-vm"}},"required":["x","y"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse_double_click_at".into(),
            description: Some("Double-click at specific position in the VM.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"x":{"type":"integer","description":"X coordinate"},"y":{"type":"integer","description":"Y coordinate"},"name":{"type":"string","default":"praxis-vm"}},"required":["x","y"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse_drag_to".into(),
            description: Some("Drag from current position to target position in the VM.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"from_x":{"type":"integer","description":"Start X coordinate"},"from_y":{"type":"integer","description":"Start Y coordinate"},"to_x":{"type":"integer","description":"End X coordinate"},"to_y":{"type":"integer","description":"End Y coordinate"},"name":{"type":"string","default":"praxis-vm"}},"required":["from_x","from_y","to_x","to_y"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_mouse_scroll_at".into(),
            description: Some("Scroll at specific position in the VM. Direction: up, down, left, right.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"x":{"type":"integer","description":"X coordinate"},"y":{"type":"integer","description":"Y coordinate"},"direction":{"type":"string","enum":["up","down","left","right"],"default":"down"},"amount":{"type":"integer","default":3,"description":"Scroll amount (lines)"},"name":{"type":"string","default":"praxis-vm"}},"required":["x","y"]}),
            is_enabled: false,
        },
        Tool {
            name: "vm_install".into(),
            description: Some("Start a VM with an installation ISO to install an OS. Provide either iso_name (searches in installation_disks context) or iso_path (direct path). The VM boots from the ISO. Use vm_keys and vm_screenshot to complete the installation.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"iso_name":{"type":"string","description":"Name to search for in installation_disks (e.g. 'alpine', 'ubuntu', 'arch')"},"iso_path":{"type":"string","description":"Direct path to ISO file (alternative to iso_name)"},"vm_name":{"type":"string","default":"praxis-vm"},"cpu_cores":{"type":"integer","default":2},"ram_mb":{"type":"integer","default":4096},"disk_size":{"type":"string","default":"40G"},"firmware":{"type":"string","enum":["bios","uefi"],"default":"bios","description":"Boot firmware: 'bios' (legacy) or 'uefi' (OVMF). Use 'uefi' for modern OSes that require EFI boot."}}}),
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
            description: Some("Ask the user questions via Discord. Each question needs 'label' (short key for the answer), 'question' (the text to ask), and optionally 'suggestions' (array of plain strings like [\"yes\", \"no\", \"maybe\"] shown as emoji buttons). Do NOT use 'options' or objects - use 'suggestions' with simple strings only.".into()),
            parameters: serde_json::json!({"type":"object","properties":{"channel_id":{"type":"string","description":"Discord channel ID"},"questions":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string","description":"Short key for answer, e.g. 'color', 'choice'"},"question":{"type":"string","description":"The question text to ask"},"suggestions":{"type":"array","items":{"type":"string"},"description":"Quick reply options as plain strings, e.g. [\"red\", \"blue\", \"green\"]"}},"required":["label","question"]},"description":"Array of questions to ask"},"timeout_secs":{"type":"integer","default":120,"description":"Timeout per question in seconds"}},"required":["channel_id","questions"]}),
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
        assert_eq!(tools.len(), 32);
    }

    #[test]
    fn test_init_default_tools_idempotent() {
        let (db, _dir) = test_db();
        init_default_tools(&db).unwrap();
        init_default_tools(&db).unwrap();
        let tools = list(&db).unwrap();
        assert_eq!(tools.len(), 32);
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
