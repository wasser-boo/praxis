use crate::vm::VmManager;
use std::sync::Arc;
use tokio::sync::OnceCell;

static VM_MANAGER: OnceCell<Arc<VmManager>> = OnceCell::const_new();

/// Convert raw PPM (P6 binary) bytes to PNG bytes
fn ppm_to_png(ppm: &[u8]) -> anyhow::Result<Vec<u8>> {
    // Parse PPM header: P6\n<width> <height>\n<maxval>\n<pixels>
    if ppm.len() < 3 || ppm[0] != b'P' || ppm[1] != b'6' {
        anyhow::bail!("Not a P6 PPM file");
    }
    let mut pos = 2; // skip "P6"
                     // Skip whitespace and comments
    loop {
        while pos < ppm.len()
            && (ppm[pos] == b'\n' || ppm[pos] == b'\r' || ppm[pos] == b' ' || ppm[pos] == b'\t')
        {
            pos += 1;
        }
        if pos < ppm.len() && ppm[pos] == b'#' {
            while pos < ppm.len() && ppm[pos] != b'\n' {
                pos += 1;
            }
        } else {
            break;
        }
    }
    // Parse width
    let width_start = pos;
    while pos < ppm.len() && ppm[pos].is_ascii_digit() {
        pos += 1;
    }
    let width: u32 = std::str::from_utf8(&ppm[width_start..pos])?.parse()?;
    // Skip whitespace
    while pos < ppm.len() && (ppm[pos] == b' ' || ppm[pos] == b'\t') {
        pos += 1;
    }
    // Parse height
    let height_start = pos;
    while pos < ppm.len() && ppm[pos].is_ascii_digit() {
        pos += 1;
    }
    let height: u32 = std::str::from_utf8(&ppm[height_start..pos])?.parse()?;
    // Skip whitespace
    while pos < ppm.len()
        && (ppm[pos] == b' ' || ppm[pos] == b'\t' || ppm[pos] == b'\n' || ppm[pos] == b'\r')
    {
        pos += 1;
    }
    // Parse maxval
    let maxval_start = pos;
    while pos < ppm.len() && ppm[pos].is_ascii_digit() {
        pos += 1;
    }
    let _maxval: u32 = std::str::from_utf8(&ppm[maxval_start..pos])?.parse()?;
    // Skip single whitespace after maxval
    if pos < ppm.len() && (ppm[pos] == b'\n' || ppm[pos] == b'\r' || ppm[pos] == b' ') {
        pos += 1;
    }
    let pixel_data = &ppm[pos..];
    let expected = (width as usize) * (height as usize) * 3;
    if pixel_data.len() < expected {
        anyhow::bail!(
            "PPM pixel data too short: got {}, expected {}",
            pixel_data.len(),
            expected
        );
    }
    // Encode as PNG
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut buf), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixel_data[..expected])?;
    }
    Ok(buf)
}

pub async fn get_vm_manager() -> Option<Arc<VmManager>> {
    VM_MANAGER.get().cloned()
}

pub fn init_vm_manager(data_dir: &str) -> Arc<VmManager> {
    let manager = Arc::new(VmManager::new(data_dir));
    VM_MANAGER.set(manager.clone()).ok();
    manager
}

pub async fn dispatch_vm_tool(tool_name: &str, args: &serde_json::Value) -> Option<String> {
    let manager = match get_vm_manager().await {
        Some(m) => m,
        None => {
            // Lazy init
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let m = init_vm_manager(&data_dir);
            m
        }
    };

    let result = match tool_name {
        "vm_start" => handle_vm_start(&manager, args).await,
        "vm_stop" => handle_vm_stop(&manager, args).await,
        "vm_shell" => handle_vm_shell(&manager, args).await,
        "vm_keys" => handle_vm_keys(&manager, args).await,
        "vm_input" => handle_vm_input(&manager, args).await,
        "vm_screenshot" => handle_vm_screenshot(&manager, args).await,
        "vm_file_transfer" => handle_vm_file_transfer(&manager, args).await,
        "vm_snapshot" => handle_vm_snapshot(&manager, args).await,
        "vm_shared_folder" => handle_vm_shared_folder(&manager, args).await,
        "vm_mouse" => handle_vm_mouse(&manager, args).await,
        "vm_look_screenshot" => handle_vm_look_screenshot(&manager, args).await,
        "vm_process_list" => handle_vm_process_list(&manager, args).await,
        "vm_file_read" => handle_vm_file_read(&manager, args).await,
        "vm_network_test" => handle_vm_network_test(&manager, args).await,
        "vm_service_list" => handle_vm_service_list(&manager, args).await,
        "vm_package_install" => handle_vm_package_install(&manager, args).await,
        "vm_snapshot_list" => handle_vm_snapshot_list(&manager, args).await,
        "vm_snapshot_restore" => handle_vm_snapshot_restore(&manager, args).await,
        "vm_snapshot_delete" => handle_vm_snapshot_delete(&manager, args).await,
        "vm_cron_add" => handle_vm_cron_add(&manager, args).await,
        "vm_cron_list" => handle_vm_cron_list(&manager, args).await,
        "vm_cron_remove" => handle_vm_cron_remove(&manager, args).await,
        "vm_shortcut" => handle_vm_shortcut(&manager, args).await,
        "vm_key_combo" => handle_vm_key_combo(&manager, args).await,
        "vm_type_fast" => handle_vm_type_fast(&manager, args).await,
        "vm_wait_for_text" => handle_vm_wait_for_text(&manager, args).await,
        "vm_window_list" => handle_vm_window_list(&manager, args).await,
        "vm_window_focus" => handle_vm_window_focus(&manager, args).await,
        "vm_clipboard_set" => handle_vm_clipboard_set(&manager, args).await,
        "vm_clipboard_get" => handle_vm_clipboard_get(&manager, args).await,
        "vm_mouse_move" => handle_vm_mouse_move(&manager, args).await,
        "vm_mouse_click_at" => handle_vm_mouse_click_at(&manager, args).await,
        "vm_mouse_double_click_at" => handle_vm_mouse_double_click_at(&manager, args).await,
        "vm_mouse_drag_to" => handle_vm_mouse_drag_to(&manager, args).await,
        "vm_mouse_scroll_at" => handle_vm_mouse_scroll_at(&manager, args).await,
        "vm_install" => handle_vm_install(&manager, args).await,
        _ => return None,
    };

    Some(result)
}

async fn handle_vm_start(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let cpu_cores = args["cpu_cores"].as_u64().unwrap_or(2) as u32;
    let ram_mb = args["ram_mb"].as_u64().unwrap_or(4096) as u32;
    let disk_size = args["disk_size"].as_str().unwrap_or("40G");
    let iso_path = args["iso_path"].as_str().map(|s| s.to_string());
    let arch = args["arch"].as_str().unwrap_or("x86_64");
    
    // Get keyboard layout: from args, or from context settings, or default "us"
    let keyboard_layout = args["keyboard_layout"].as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Try to load from context settings (default user)
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            if let Ok(db) = crate::db::Database::new(std::path::Path::new(&data_dir)) {
                if let Ok(ctx) = db.load_context("default") {
                    let layout = ctx.settings.vm_keyboard_layout;
                    tracing::info!(layout = %layout, "Loaded keyboard layout from context for vm_start");
                    return layout;
                }
            }
            "us".to_string()
        });
    
    tracing::info!(layout = %keyboard_layout, "VM starting with keyboard layout");

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    // Use fixed VNC port to avoid port drift on restarts
    let vnc_offset: u16 = 1;

    let mut config = crate::vm::VmConfig::default_for_name(name, &data_dir, vnc_offset, arch);
    config.cpu_cores = cpu_cores;
    config.ram_mb = ram_mb;
    config.disk_size = disk_size.to_string();
    config.iso_path = iso_path;
    config.keyboard_layout = crate::vm::KeyboardLayout::from_str(&keyboard_layout);

    // Auto-add shared folders from data directory
    config.shared_folders.push(crate::vm::SharedFolder {
        host_path: format!("{}/shared", data_dir),
        mount_tag: "praxis-shared".to_string(),
        mount_point: "/mnt/shared".to_string(),
        readonly: false,
    });

    match manager.start_vm(config).await {
        Ok(msg) => {
            // Clear old screenshots from previous sessions
            let ss_dir = format!("{}/vm/{}/screenshots", data_dir, name);
            if let Ok(entries) = std::fs::read_dir(&ss_dir) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            let _ = std::fs::remove_file(format!("{}/vm/{}/screenshot.ppm", data_dir, name));
            msg
        }
        Err(e) => format!("Error starting VM: {}", e),
    }
}

async fn handle_vm_stop(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    match manager.stop_vm(name).await {
        Ok(msg) => msg,
        Err(e) => format!("Error stopping VM: {}", e),
    }
}

async fn handle_vm_shell(manager: &VmManager, args: &serde_json::Value) -> String {
    let command = args["command"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let timeout = args["timeout_secs"].as_u64().unwrap_or(30);

    if command.is_empty() {
        return "Error: command is required".to_string();
    }

    match manager.shell_exec(name, command, timeout).await {
        Ok(output) => output,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_vm_keys(manager: &VmManager, args: &serde_json::Value) -> String {
    let keys = args["keys"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // Get keyboard layout: from args, or from context settings, or default "us"
    let layout_override = args["keyboard_layout"].as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            // Try to load from context settings
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            match crate::db::Database::new(std::path::Path::new(&data_dir)) {
                Ok(db) => match db.load_context("default") {
                    Ok(ctx) => {
                        let layout = ctx.settings.vm_keyboard_layout;
                        tracing::info!(layout = %layout, "Loaded keyboard layout from context for vm_keys");
                        Some(layout)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to load context for keyboard layout");
                        None
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to open database for keyboard layout");
                    None
                }
            }
        });
    
    tracing::info!(keys = %keys, layout = ?layout_override, "vm_keys called");

    if keys.is_empty() {
        return "Error: keys is required".to_string();
    }

    match manager.send_keys_with_layout(name, keys, layout_override.as_deref()).await {
        Ok(msg) => msg,
        Err(e) => format!("Error: {}", e),
    }
}

/// High-level input tool for VM navigation
/// Supports vim-like commands and intelligent menu navigation
async fn handle_vm_input(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let action = args["action"].as_str().unwrap_or("");
    let text = args["text"].as_str().unwrap_or("");
    let count = args["count"].as_u64().unwrap_or(1) as usize;
    
    // Get keyboard layout: from args, or from context settings, or default "us"
    let layout_override = args["keyboard_layout"].as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            // Try to load from context settings
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            match crate::db::Database::new(std::path::Path::new(&data_dir)) {
                Ok(db) => match db.load_context("default") {
                    Ok(ctx) => {
                        let layout = ctx.settings.vm_keyboard_layout;
                        tracing::info!(layout = %layout, "Loaded keyboard layout from context");
                        Some(layout)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to load context for keyboard layout");
                        None
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to open database for keyboard layout");
                    None
                }
            }
        });
    
    tracing::info!(action = %action, text = %text, layout = ?layout_override, "vm_input called");

    match action {
        // Navigation
        "up" | "k" => {
            match manager.send_keys_with_layout(name, &format!("(arrowup){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Moved up {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "down" | "j" => {
            match manager.send_keys_with_layout(name, &format!("(arrowdown){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Moved down {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "left" | "h" => {
            match manager.send_keys_with_layout(name, &format!("(arrowleft){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Moved left {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "right" | "l" => {
            match manager.send_keys_with_layout(name, &format!("(arrowright){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Moved right {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Page navigation
        "pageup" | "pgup" => {
            match manager.send_keys_with_layout(name, &format!("(pageup){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Page up {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "pagedown" | "pgdn" => {
            match manager.send_keys_with_layout(name, &format!("(pagedown){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Page down {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "home" => {
            match manager.send_keys_with_layout(name, "home", layout_override.as_deref()).await {
                Ok(_) => "Pressed Home".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "end" => {
            match manager.send_keys_with_layout(name, "end", layout_override.as_deref()).await {
                Ok(_) => "Pressed End".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Tab navigation (common in installers)
        "tab" => {
            match manager.send_keys_with_layout(name, &format!("(tab){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Tab {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "shifttab" | "backtab" => {
            match manager.send_keys_with_layout(name, &format!("shift+tab{}", if count > 1 { format!("(shift+tab){}", count - 1) } else { String::new() }), layout_override.as_deref()).await {
                Ok(_) => format!("Shift+Tab {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Common actions
        "enter" | "confirm" | "select" => {
            match manager.send_keys_with_layout(name, &format!("(enter){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Enter {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "esc" | "escape" | "cancel" | "back" => {
            match manager.send_keys_with_layout(name, &format!("(esc){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Escape {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "space" | "toggle" => {
            match manager.send_keys_with_layout(name, &format!("(space){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Space {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        "backspace" | "delete" => {
            match manager.send_keys_with_layout(name, &format!("(backspace){}", count), layout_override.as_deref()).await {
                Ok(_) => format!("Backspace {} time(s)", count),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Type text (with optional enter)
        "type" => {
            if text.is_empty() {
                return "Error: text is required for type action".to_string();
            }
            let enter_after = args["enter"].as_bool().unwrap_or(false);
            let keys = if enter_after {
                format!("{}\n", text)
            } else {
                text.to_string()
            };
            match manager.send_keys_with_layout(name, &keys, layout_override.as_deref()).await {
                Ok(_) => {
                    if enter_after {
                        format!("Typed: {} (+ enter)", text)
                    } else {
                        format!("Typed: {}", text)
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Type and press enter (shorthand)
        "typeenter" => {
            if text.is_empty() {
                return "Error: text is required for typeenter action".to_string();
            }
            match manager.send_keys_with_layout(name, &format!("{}\n", text), layout_override.as_deref()).await {
                Ok(_) => format!("Typed: {} + enter", text),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Number shortcuts (for menu selection)
        "num" | "number" => {
            if text.is_empty() {
                return "Error: text (the number) is required for num action".to_string();
            }
            match manager.send_keys_with_layout(name, &format!("{}\n", text), layout_override.as_deref()).await {
                Ok(_) => format!("Selected option: {}", text),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Function keys
        "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => {
            match manager.send_keys_with_layout(name, &format!("({}){}", action, count), layout_override.as_deref()).await {
                Ok(_) => format!("Pressed {} {} time(s)", action, count),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Ctrl combinations
        "ctrl_c" | "interrupt" => {
            match manager.send_keys_with_layout(name, "ctrl+c", layout_override.as_deref()).await {
                Ok(_) => "Sent Ctrl+C".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "ctrl_z" => {
            match manager.send_keys_with_layout(name, "ctrl+z", layout_override.as_deref()).await {
                Ok(_) => "Sent Ctrl+Z".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "ctrl_a" => {
            match manager.send_keys_with_layout(name, "ctrl+a", layout_override.as_deref()).await {
                Ok(_) => "Sent Ctrl+A".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "ctrl_l" | "clear" => {
            match manager.send_keys_with_layout(name, "ctrl+l", layout_override.as_deref()).await {
                Ok(_) => "Sent Ctrl+L (clear)".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Wait (useful between actions)
        "wait" => {
            let ms = args["ms"].as_u64().unwrap_or(1000);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            format!("Waited {}ms", ms)
        }
        
        // Bash/Shell mode - execute command directly via serial (no keyboard simulation)
        "bash" | "shell" | "exec" => {
            let command = args["command"].as_str().unwrap_or(text);
            if command.is_empty() {
                return "Error: command is required for bash action".to_string();
            }
            let timeout = args["timeout_secs"].as_u64().unwrap_or(30);
            match manager.shell_exec(name, command, timeout).await {
                Ok(output) => format!("Output:\n{}", output),
                Err(e) => format!("Error: {}", e),
            }
        }
        
        // Write file directly to VM
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            if path.is_empty() {
                return "Error: path is required for write_file action".to_string();
            }
            // Write to shared folder first
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let filename = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let transfer_path = format!("{}/shared/{}", data_dir, filename);
            if let Err(e) = std::fs::write(&transfer_path, content) {
                return format!("Error writing file to shared folder: {}", e);
            }
            // Copy from shared folder to target path in VM
            match manager.shell_exec(name, &format!("cp /mnt/shared/{} {}", filename, path), 10).await {
                Ok(_) => format!("File written to VM: {} ({} bytes)", path, content.len()),
                Err(e) => format!("Error copying file in VM: {}", e),
            }
        }
        
        // System status checker
        "system_status" | "status" => {
            let checks = vec![
                ("keymap", "cat /etc/vconsole.conf 2>/dev/null || echo 'not set'"),
                ("locale", "locale 2>/dev/null | head -5 || echo 'not available'"),
                ("hostname", "hostname"),
                ("kernel", "uname -r"),
                ("uptime", "uptime"),
                ("disk", "df -h / | tail -1"),
                ("memory", "free -h | grep Mem"),
                ("network", "ip addr show 2>/dev/null | grep -E 'inet.*scope global' || echo 'no network'"),
                ("users", "who 2>/dev/null || echo 'no users logged in'"),
                ("services", "ls /var/service/ 2>/dev/null || echo 'no services'"),
            ];
            let mut status = String::new();
            for (name, cmd) in checks {
                match manager.shell_exec(name, cmd, 5).await {
                    Ok(output) => {
                        status.push_str(&format!("{}: {}\n", name, output.trim()));
                    }
                    Err(_) => {
                        status.push_str(&format!("{}: unavailable\n", name));
                    }
                }
            }
            status
        }
        
        // Restart a service (Void Linux runit)
        "restart_service" => {
            let service = args["service"].as_str().unwrap_or("");
            if service.is_empty() {
                return "Error: service name is required for restart_service action".to_string();
            }
            match manager.shell_exec(name, &format!("sv restart {}", service), 10).await {
                Ok(output) => format!("Service '{}' restarted: {}", service, output.trim()),
                Err(e) => format!("Error restarting service '{}': {}", service, e),
            }
        }
        
        _ => {
            format!("Unknown action: {}. Valid actions: up/down/left/right, tab/shifttab, enter/confirm, esc/cancel, space/toggle, type, typeenter, num, f1-f12, ctrl_c/ctrl_z/ctrl_a/ctrl_l, wait, bash, write_file, system_status, restart_service", action)
        }
    }
}

async fn handle_vm_screenshot(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    match save_screenshot_to_disk(name, &data_dir).await {
        Some(path) => format!("Screenshot saved to: {}", path),
        None => "Error: Failed to capture screenshot. Is the VM running?".to_string(),
    }
}

async fn handle_vm_file_transfer(manager: &VmManager, args: &serde_json::Value) -> String {
    let path = args["path"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let direction = args["direction"].as_str().unwrap_or("to_vm");

    if path.is_empty() {
        return "Error: path is required".to_string();
    }

    match direction {
        "to_vm" => {
            let content = args["content"].as_str().unwrap_or("");
            // Write to shared folder, then copy in VM
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let transfer_path = format!(
                "{}/shared/{}",
                data_dir,
                std::path::Path::new(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            if let Err(e) = std::fs::write(&transfer_path, content) {
                return format!("Error writing file to shared folder: {}", e);
            }
            // Copy from shared folder to target path in VM
            let filename = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            match manager.shell_exec(name, &format!("cp /mnt/shared/{} {}", filename, path), 10).await {
                Ok(_) => format!("File transferred to VM: {}", path),
                Err(e) => format!("File written to shared folder but copy in VM failed: {}. You can manually copy from /mnt/shared/", e),
            }
        }
        "from_vm" => {
            // Copy from VM path to shared folder, then read
            let filename = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            match manager
                .shell_exec(name, &format!("cp {} /mnt/shared/{}", path, filename), 10)
                .await
            {
                Ok(_) => {
                    let data_dir =
                        std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
                    let shared_path = format!("{}/shared/{}", data_dir, filename);
                    match std::fs::read_to_string(&shared_path) {
                        Ok(content) => format!("File content from VM:\n{}", content),
                        Err(e) => format!("File copied to shared folder but read failed: {}", e),
                    }
                }
                Err(e) => format!("Error copying file from VM: {}", e),
            }
        }
        _ => "Error: direction must be 'to_vm' or 'from_vm'".to_string(),
    }
}

async fn handle_vm_snapshot(manager: &VmManager, args: &serde_json::Value) -> String {
    let snapshot_name = args["snapshot_name"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");

    if snapshot_name.is_empty() {
        return "Error: snapshot_name is required".to_string();
    }

    match manager.create_snapshot(name, snapshot_name).await {
        Ok(msg) => msg,
        Err(e) => format!("Error creating snapshot: {}", e),
    }
}

async fn handle_vm_shared_folder(manager: &VmManager, args: &serde_json::Value) -> String {
    let host_path = args["host_path"].as_str().unwrap_or("");
    let mount_point = args["mount_point"].as_str().unwrap_or("/mnt/shared");
    let readonly = args["readonly"].as_bool().unwrap_or(false);
    let name = args["name"].as_str().unwrap_or("praxis-vm");

    if host_path.is_empty() {
        return "Error: host_path is required".to_string();
    }

    let tag = format!(
        "shared-{}",
        std::path::Path::new(host_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );

    let folder = crate::vm::SharedFolder {
        host_path: host_path.to_string(),
        mount_tag: tag,
        mount_point: mount_point.to_string(),
        readonly,
    };

    match manager.add_shared_folder(name, folder).await {
        Ok(msg) => msg,
        Err(e) => format!("Error adding shared folder: {}", e),
    }
}

async fn handle_vm_mouse(manager: &VmManager, args: &serde_json::Value) -> String {
    let action = args["action"].as_str().unwrap_or("click");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let x = args["x"].as_i64().map(|v| v as i32);
    let y = args["y"].as_i64().map(|v| v as i32);
    let dx = args["dx"].as_i64().map(|v| v as i32);
    let dy = args["dy"].as_i64().map(|v| v as i32);
    let button = args["button"].as_i64().map(|v| v as i32);
    let scroll_vertical = args["scroll_vertical"].as_i64().map(|v| v as i32);
    let scroll_horizontal = args["scroll_horizontal"].as_i64().map(|v| v as i32);

    match manager
        .send_mouse(
            name,
            action,
            x,
            y,
            dx,
            dy,
            button,
            scroll_vertical,
            scroll_horizontal,
        )
        .await
    {
        Ok(msg) => msg,
        Err(e) => format!("Error: {}", e),
    }
}

/// Take a screenshot and return just the base64 data URL (for agent loop injection)
pub async fn take_screenshot_for_context(vm_name: &str, enabled: bool, limit: usize) -> Option<String> {
    if !enabled {
        return None;
    }
    let manager = get_vm_manager().await?;
    match manager.screenshot(vm_name).await {
        Ok(data_url) => {
            // Enforce screenshot limit
            cleanup_screenshot_limit(limit);
            Some(data_url)
        }
        Err(_) => None,
    }
}

/// Save screenshot to disk and return path (for vm_look_screenshot)
pub async fn save_screenshot_to_disk(vm_name: &str, data_dir: &str) -> Option<String> {
    let manager = get_vm_manager().await?;
    let screenshot_data = manager.screenshot(vm_name).await.ok()?;
    tracing::info!(vm = %vm_name, data_len = screenshot_data.len(), "Screenshot data received from QMP");

    let ss_dir = format!("{}/vm/{}/screenshots", data_dir, vm_name);
    let _ = std::fs::create_dir_all(&ss_dir);

    cleanup_screenshot_limit_dir(&ss_dir, 5000);

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("{}/screenshot_{}.png", ss_dir, ts);

    if let Some(b64) = screenshot_data.strip_prefix("data:image/ppm;base64,") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            match ppm_to_png(&bytes) {
                Ok(png_bytes) => {
                    let _ = std::fs::write(&filename, &png_bytes);
                    // Clean up the temporary PPM file
                    let _ =
                        std::fs::remove_file(format!("{}/vm/{}/screenshot.ppm", data_dir, vm_name));
                    return Some(filename);
                }
                Err(e) => {
                    tracing::warn!("PPM to PNG conversion failed: {}, falling back to PPM", e);
                }
            }
            let ppm_filename = format!("{}/screenshot_{}.ppm", ss_dir, ts);
            let _ = std::fs::write(&ppm_filename, bytes);
            let _ = std::fs::remove_file(format!("{}/vm/{}/screenshot.ppm", data_dir, vm_name));
            return Some(ppm_filename);
        }
    }
    None
}

/// Read a screenshot file and return a base64 data URL
pub fn screenshot_to_data_url(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let mime = if path.ends_with(".png") {
        "image/png"
    } else {
        "image/ppm"
    };
    Some(format!("data:{};base64,{}", mime, b64))
}

async fn handle_vm_look_screenshot(_manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let index = args["index"].as_i64().map(|v| v as i32); // -1 = latest, 0 = oldest, N = specific
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let ss_dir = format!("{}/vm/{}/screenshots", data_dir, name);

    if !std::path::Path::new(&ss_dir).exists() {
        return "No screenshots found. Screenshots are saved when vm_auto_screenshot is enabled."
            .to_string();
    }

    // List all screenshots sorted by name (which includes timestamp)
    let mut files: Vec<String> = std::fs::read_dir(&ss_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".png"))
        .map(|e| e.path().to_string_lossy().to_string())
        .collect();

    files.sort();

    if files.is_empty() {
        return "No screenshots found.".to_string();
    }

    let file_path = match index {
        Some(-1) | None => files.last().cloned(), // latest
        Some(0) => files.first().cloned(),        // oldest
        Some(n) => {
            let idx = n as usize;
            if idx < files.len() {
                files.get(idx).cloned()
            } else {
                return format!(
                    "Screenshot index {} out of range (have {} screenshots)",
                    n,
                    files.len()
                );
            }
        }
    };

    match file_path {
        Some(path) => {
            format!("Screenshot: {}\nTotal screenshots: {}", path, files.len())
        }
        None => "No screenshot found at that index.".to_string(),
    }
}

/// Enforce screenshot limit globally
fn cleanup_screenshot_limit(limit: usize) {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let vm_dir = format!("{}/vm", data_dir);
    if let Ok(entries) = std::fs::read_dir(&vm_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let ss_dir = entry.path().join("screenshots");
                cleanup_screenshot_limit_dir(&ss_dir.to_string_lossy(), limit);
            }
        }
    }
}

/// Enforce screenshot limit in a specific directory
fn cleanup_screenshot_limit_dir(ss_dir: &str, max_screenshots: usize) {
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(ss_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".png"))
        .filter_map(|e| {
            let time = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((e.path(), time))
        })
        .collect();

    if files.len() <= max_screenshots {
        return;
    }

    // Sort oldest first
    files.sort_by_key(|(_, t)| *t);

    // Delete oldest files to get back to limit
    let to_delete = files.len() - max_screenshots;
    for (path, _) in files.iter().take(to_delete) {
        let _ = std::fs::remove_file(path);
    }
    tracing::info!("Cleaned {} old screenshots from {}", to_delete, ss_dir);
}

async fn handle_vm_install(manager: &VmManager, args: &serde_json::Value) -> String {
    let iso_name = args["iso_name"].as_str().unwrap_or("");
    let iso_path_arg = args["iso_path"].as_str();
    let vm_name = args["vm_name"].as_str().unwrap_or("praxis-vm");
    let cpu_cores = args["cpu_cores"].as_u64().unwrap_or(2) as u32;
    let ram_mb = args["ram_mb"].as_u64().unwrap_or(4096) as u32;
    let disk_size = args["disk_size"].as_str().unwrap_or("40G");

    // Resolve ISO path: check iso_name against installation_disks, or use iso_path directly
    let iso_path = if let Some(path) = iso_path_arg {
        path.to_string()
    } else if !iso_name.is_empty() {
        // Search in installation_disks
        let isos = manager.list_isos();
        let found = isos.iter().find(|iso| {
            iso.get("name")
                .and_then(|v| v.as_str())
                .map(|n| n.to_lowercase().contains(&iso_name.to_lowercase()))
                .unwrap_or(false)
                || iso
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|p| p.to_lowercase().contains(&iso_name.to_lowercase()))
                    .unwrap_or(false)
        });
        match found {
            Some(iso) => iso
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            None => {
                // Also check the iso_dir for a file matching the name
                let iso_dir = manager.iso_dir();
                let candidates: Vec<String> = std::fs::read_dir(&iso_dir)
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let fname = e.file_name().to_string_lossy().to_lowercase();
                        fname.contains(&iso_name.to_lowercase())
                            && (fname.ends_with(".iso") || fname.ends_with(".img"))
                    })
                    .map(|e| e.path().to_string_lossy().to_string())
                    .collect();
                if let Some(path) = candidates.first() {
                    path.clone()
                } else {
                    return format!(
                        "ISO '{}' not found in installation_disks or {}. Available ISOs: {:?}",
                        iso_name,
                        iso_dir,
                        isos.iter()
                            .filter_map(|i| i.get("name").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    } else {
        return "Error: either iso_name or iso_path is required".to_string();
    };

    // Verify the ISO exists
    if !std::path::Path::new(&iso_path).exists() {
        return format!("ISO not found at: {}", iso_path);
    }

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let arch = std::env::var("VM_ARCH").unwrap_or_else(|_| "x86_64".to_string());
    let vnc_offset = manager.list_vms().await.len() as u16 + 1;

    let mut config = crate::vm::VmConfig::default_for_name(vm_name, &data_dir, vnc_offset, &arch);
    config.cpu_cores = cpu_cores;
    config.ram_mb = ram_mb;
    config.disk_size = disk_size.to_string();
    config.iso_path = Some(iso_path.clone());

    // Add shared folder
    config.shared_folders.push(crate::vm::SharedFolder {
        host_path: format!("{}/shared", data_dir),
        mount_tag: "praxis-shared".to_string(),
        mount_point: "/mnt/shared".to_string(),
        readonly: false,
    });

    match manager.start_vm(config).await {
        Ok(msg) => format!("VM '{}' started with ISO '{}'. The installer should boot.\n{}\nUse vm_keys and vm_screenshot to interact with the installer.", vm_name, iso_path, msg),
        Err(e) => format!("Error starting VM with ISO: {}", e),
    }
}

// ── New VM Tools ────────────────────────────────────────────────────────────

async fn handle_vm_process_list(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    match manager.shell_exec(name, "ps aux --sort=-%cpu | head -30", 10).await {
        Ok(output) => format!("Running processes:\n{}", output),
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_vm_file_read(manager: &VmManager, args: &serde_json::Value) -> String {
    let path = args["path"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if path.is_empty() {
        return "Error: path is required".to_string();
    }
    
    match manager.shell_exec(name, &format!("cat '{}'", path), 10).await {
        Ok(output) => format!("Content of {}:\n{}", path, output),
        Err(e) => format!("Error reading file '{}': {}", path, e),
    }
}

async fn handle_vm_network_test(manager: &VmManager, args: &serde_json::Value) -> String {
    let action = args["action"].as_str().unwrap_or("interfaces");
    let target = args["target"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    let cmd = match action {
        "ping" => {
            if target.is_empty() {
                return "Error: target host is required for ping".to_string();
            }
            format!("ping -c 3 {}", target)
        }
        "curl" => {
            if target.is_empty() {
                return "Error: URL is required for curl".to_string();
            }
            format!("curl -sI {} | head -10", target)
        }
        "dns" => {
            if target.is_empty() {
                return "Error: domain is required for dns".to_string();
            }
            format!("nslookup {} 2>&1 || host {} 2>&1 || dig {} 2>&1", target, target, target)
        }
        "interfaces" => "ip addr show 2>/dev/null || ifconfig".to_string(),
        "routes" => "ip route show 2>/dev/null || route -n".to_string(),
        _ => return format!("Unknown action: {}. Use: ping, curl, dns, interfaces, routes", action),
    };
    
    match manager.shell_exec(name, &cmd, 15).await {
        Ok(output) => format!("Network {}:\n{}", action, output),
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_vm_service_list(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // List services and their status (Void Linux runit)
    let cmd = "for svc in /var/service/*; do name=$(basename $svc); if [ -f $svc/supervise/ok ]; then status='running'; else status='stopped'; fi; echo \"$name: $status\"; done 2>/dev/null || ls -la /var/service/ 2>/dev/null";
    
    match manager.shell_exec(name, cmd, 10).await {
        Ok(output) => format!("Services:\n{}", output),
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_vm_package_install(manager: &VmManager, args: &serde_json::Value) -> String {
    let packages = args["packages"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if packages.is_empty() {
        return "Error: packages is required".to_string();
    }
    
    // Update package list and install
    let cmd = format!("xbps-install -Sy {}", packages);
    
    match manager.shell_exec(name, &cmd, 120).await {
        Ok(output) => format!("Package install result:\n{}", output),
        Err(e) => format!("Error installing packages: {}", e),
    }
}

// ── Snapshot Tools ──────────────────────────────────────────────────────────

async fn handle_vm_snapshot_list(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    match manager.shell_exec(name, "qemu-img snapshot -l /data/vm/*/disk.qcow2 2>/dev/null || echo 'No snapshots found'", 10).await {
        Ok(output) => format!("Snapshots:\n{}", output),
        Err(e) => format!("Error listing snapshots: {}", e),
    }
}

async fn handle_vm_snapshot_restore(manager: &VmManager, args: &serde_json::Value) -> String {
    let snapshot_name = args["snapshot_name"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if snapshot_name.is_empty() {
        return "Error: snapshot_name is required".to_string();
    }
    
    // This requires VM to be stopped first, then restore, then start
    match manager.stop_vm(name).await {
        Ok(_) => {
            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let disk_path = format!("{}/vm/{}/disk.qcow2", data_dir, name);
            match manager.shell_exec(name, &format!("qemu-img snapshot -a {} {}", snapshot_name, disk_path), 30).await {
                Ok(_) => {
                    match manager.start_vm(crate::vm::VmConfig::default_for_name(name, &data_dir, 1, "x86_64")).await {
                        Ok(msg) => format!("Snapshot '{}' restored. VM restarted.\n{}", snapshot_name, msg),
                        Err(e) => format!("Snapshot restored but VM restart failed: {}", e),
                    }
                }
                Err(e) => format!("Error restoring snapshot: {}", e),
            }
        }
        Err(e) => format!("Error stopping VM for snapshot restore: {}", e),
    }
}

async fn handle_vm_snapshot_delete(manager: &VmManager, args: &serde_json::Value) -> String {
    let snapshot_name = args["snapshot_name"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if snapshot_name.is_empty() {
        return "Error: snapshot_name is required".to_string();
    }
    
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let disk_path = format!("{}/vm/{}/disk.qcow2", data_dir, name);
    match manager.shell_exec(name, &format!("qemu-img snapshot -d {} {}", snapshot_name, disk_path), 10).await {
        Ok(output) => format!("Snapshot '{}' deleted: {}", snapshot_name, output),
        Err(e) => format!("Error deleting snapshot: {}", e),
    }
}

// ── Cron Job Tools ──────────────────────────────────────────────────────────

async fn handle_vm_cron_add(manager: &VmManager, args: &serde_json::Value) -> String {
    let schedule = args["schedule"].as_str().unwrap_or("");
    let command = args["command"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if schedule.is_empty() || command.is_empty() {
        return "Error: schedule and command are required".to_string();
    }
    
    // Add to crontab
    let cron_line = format!("{} {}", schedule, command);
    let cmd = format!("(crontab -l 2>/dev/null; echo '{}') | crontab -", cron_line);
    
    match manager.shell_exec(name, &cmd, 10).await {
        Ok(_) => format!("Cron job added: {}", cron_line),
        Err(e) => format!("Error adding cron job: {}", e),
    }
}

async fn handle_vm_cron_list(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    match manager.shell_exec(name, "crontab -l 2>/dev/null || echo 'No cron jobs'", 10).await {
        Ok(output) => format!("Cron jobs:\n{}", output),
        Err(e) => format!("Error listing cron jobs: {}", e),
    }
}

async fn handle_vm_cron_remove(manager: &VmManager, args: &serde_json::Value) -> String {
    let pattern = args["pattern"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if pattern.is_empty() {
        return "Error: pattern is required".to_string();
    }
    
    // Remove cron jobs matching pattern
    let cmd = format!("crontab -l 2>/dev/null | grep -v '{}' | crontab -", pattern);
    
    match manager.shell_exec(name, &cmd, 10).await {
        Ok(_) => format!("Cron jobs matching '{}' removed", pattern),
        Err(e) => format!("Error removing cron jobs: {}", e),
    }
}

// ── Keyboard Shortcut Tools ─────────────────────────────────────────────────

async fn handle_vm_shortcut(manager: &VmManager, args: &serde_json::Value) -> String {
    let shortcut = args["shortcut"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if shortcut.is_empty() {
        return "Error: shortcut is required".to_string();
    }
    
    let keys = match shortcut.to_lowercase().as_str() {
        "copy" => "ctrl+c",
        "paste" => "ctrl+v",
        "cut" => "ctrl+x",
        "select_all" => "ctrl+a",
        "undo" => "ctrl+z",
        "redo" => "ctrl+y",
        "save" => "ctrl+s",
        "open" => "ctrl+o",
        "new" => "ctrl+n",
        "close" => "ctrl+w",
        "quit" => "ctrl+q",
        "find" => "ctrl+f",
        "replace" => "ctrl+h",
        "print" => "ctrl+p",
        "alt_tab" => "alt+tab",
        "alt_f4" => "alt+f4",
        "ctrl_alt_delete" => "ctrl+alt+delete",
        "minimize" => "super+down",
        "maximize" => "super+up",
        "fullscreen" => "f11",
        "volume_up" => "XF86AudioRaiseVolume",
        "volume_down" => "XF86AudioLowerVolume",
        "mute" => "XF86AudioMute",
        "brightness_up" => "XF86MonBrightnessUp",
        "brightness_down" => "XF86MonBrightnessDown",
        _ => return format!("Unknown shortcut: {}. Available: copy, paste, cut, select_all, undo, redo, save, open, new, close, quit, find, replace, print, alt_tab, alt_f4, ctrl_alt_delete, minimize, maximize, fullscreen, volume_up, volume_down, mute, brightness_up, brightness_down", shortcut),
    };
    
    match manager.send_keys_with_layout(name, keys, None).await {
        Ok(_) => format!("Sent shortcut: {} ({})", shortcut, keys),
        Err(e) => format!("Error sending shortcut: {}", e),
    }
}

async fn handle_vm_key_combo(manager: &VmManager, args: &serde_json::Value) -> String {
    let combo = args["combo"].as_str().unwrap_or("");
    let repeat = args["repeat"].as_u64().unwrap_or(1) as usize;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if combo.is_empty() {
        return "Error: combo is required".to_string();
    }
    
    let mut result = String::new();
    for i in 0..repeat {
        match manager.send_keys_with_layout(name, combo, None).await {
            Ok(_) => {
                if repeat > 1 {
                    result.push_str(&format!("Sent combo {}/{}: {}\n", i + 1, repeat, combo));
                } else {
                    result.push_str(&format!("Sent combo: {}", combo));
                }
            }
            Err(e) => return format!("Error sending combo: {}", e),
        }
    }
    result
}

async fn handle_vm_type_fast(manager: &VmManager, args: &serde_json::Value) -> String {
    let text = args["text"].as_str().unwrap_or("");
    let enter = args["enter"].as_bool().unwrap_or(false);
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    let layout = args["keyboard_layout"].as_str().map(|s| s.to_string());
    
    if text.is_empty() {
        return "Error: text is required".to_string();
    }
    
    let keys = if enter {
        format!("{}\n", text)
    } else {
        text.to_string()
    };
    
    // Use faster delays for type_fast
    match manager.send_keys_with_layout(name, &keys, layout.as_deref()).await {
        Ok(_) => {
            if enter {
                format!("Typed fast: {} + Enter", text)
            } else {
                format!("Typed fast: {}", text)
            }
        }
        Err(e) => format!("Error typing: {}", e),
    }
}

async fn handle_vm_wait_for_text(manager: &VmManager, args: &serde_json::Value) -> String {
    let text = args["text"].as_str().unwrap_or("");
    let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(60);
    let interval_secs = args["interval_secs"].as_u64().unwrap_or(5);
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if text.is_empty() {
        return "Error: text is required".to_string();
    }
    
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    
    loop {
        if start.elapsed() > timeout {
            return format!("Timeout waiting for '{}' after {}s", text, timeout_secs);
        }
        
        // Take screenshot and check for text
        match manager.screenshot(name).await {
            Ok(_) => {
                // For now, just wait and check periodically
                // In a real implementation, we'd use OCR or check the serial output
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
                
                // Check if text appears in serial output
                match manager.shell_exec(name, &format!("grep -q '{}' /tmp/screen_buffer 2>/dev/null && echo 'FOUND' || echo 'NOT_FOUND'", text), 5).await {
                    Ok(output) => {
                        if output.contains("FOUND") {
                            return format!("Text '{}' found on screen", text);
                        }
                    }
                    Err(_) => continue,
                }
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            }
        }
    }
}

// ── Window Management Tools ─────────────────────────────────────────────────

async fn handle_vm_window_list(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // Try wmctrl first, then xdotool
    let cmd = "wmctrl -l 2>/dev/null || xdotool search --name '' getwindowname 2>/dev/null || echo 'No window manager detected'";
    
    match manager.shell_exec(name, cmd, 10).await {
        Ok(output) => format!("Windows:\n{}", output),
        Err(e) => format!("Error listing windows: {}", e),
    }
}

async fn handle_vm_window_focus(manager: &VmManager, args: &serde_json::Value) -> String {
    let window = args["window"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if window.is_empty() {
        return "Error: window is required".to_string();
    }
    
    // Try wmctrl first, then xdotool
    let cmd = format!("wmctrl -a '{}' 2>/dev/null || xdotool search --name '{}' windowactivate 2>/dev/null || echo 'Could not focus window'", window, window);
    
    match manager.shell_exec(name, &cmd, 10).await {
        Ok(output) => format!("Window focus result: {}", output),
        Err(e) => format!("Error focusing window: {}", e),
    }
}

// ── Clipboard Tools ─────────────────────────────────────────────────────────

async fn handle_vm_clipboard_set(manager: &VmManager, args: &serde_json::Value) -> String {
    let content = args["content"].as_str().unwrap_or("");
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    if content.is_empty() {
        return "Error: content is required".to_string();
    }
    
    // Write content to temp file, then use xclip
    let cmd = format!("echo '{}' > /tmp/clipboard_content && xclip -selection clipboard < /tmp/clipboard_content 2>/dev/null || xsel --clipboard < /tmp/clipboard_content 2>/dev/null || echo 'Clipboard tool not available'", content);
    
    match manager.shell_exec(name, &cmd, 10).await {
        Ok(output) => {
            if output.contains("not available") {
                "Error: xclip or xsel not installed. Install with: xbps-install xclip".to_string()
            } else {
                format!("Clipboard set to: {}", content)
            }
        }
        Err(e) => format!("Error setting clipboard: {}", e),
    }
}

async fn handle_vm_clipboard_get(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    let cmd = "xclip -selection clipboard -o 2>/dev/null || xsel --clipboard 2>/dev/null || echo 'Clipboard tool not available'";
    
    match manager.shell_exec(name, cmd, 10).await {
        Ok(output) => {
            if output.contains("not available") {
                "Error: xclip or xsel not installed. Install with: xbps-install xclip".to_string()
            } else {
                format!("Clipboard content:\n{}", output)
            }
        }
        Err(e) => format!("Error getting clipboard: {}", e),
    }
}

// ── Mouse Tools ─────────────────────────────────────────────────────────────

async fn handle_vm_mouse_move(manager: &VmManager, args: &serde_json::Value) -> String {
    let x = args["x"].as_i64().unwrap_or(0) as i32;
    let y = args["y"].as_i64().unwrap_or(0) as i32;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    match manager.send_mouse(name, "move_absolute", Some(x), Some(y), None, None, None, None, None).await {
        Ok(_) => format!("Mouse moved to ({}, {})", x, y),
        Err(e) => format!("Error moving mouse: {}", e),
    }
}

async fn handle_vm_mouse_click_at(manager: &VmManager, args: &serde_json::Value) -> String {
    let x = args["x"].as_i64().unwrap_or(0) as i32;
    let y = args["y"].as_i64().unwrap_or(0) as i32;
    let button = args["button"].as_i64().unwrap_or(0) as i32;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // Move to position first, then click
    match manager.send_mouse(name, "move_absolute", Some(x), Some(y), None, None, None, None, None).await {
        Ok(_) => {
            match manager.send_mouse(name, "click", Some(x), Some(y), None, None, Some(button), None, None).await {
                Ok(_) => format!("Clicked at ({}, {}) button={}", x, y, button),
                Err(e) => format!("Error clicking: {}", e),
            }
        }
        Err(e) => format!("Error moving mouse: {}", e),
    }
}

async fn handle_vm_mouse_double_click_at(manager: &VmManager, args: &serde_json::Value) -> String {
    let x = args["x"].as_i64().unwrap_or(0) as i32;
    let y = args["y"].as_i64().unwrap_or(0) as i32;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // Move to position first, then double click
    match manager.send_mouse(name, "move_absolute", Some(x), Some(y), None, None, None, None, None).await {
        Ok(_) => {
            match manager.send_mouse(name, "double_click", Some(x), Some(y), None, None, None, None, None).await {
                Ok(_) => format!("Double-clicked at ({}, {})", x, y),
                Err(e) => format!("Error double-clicking: {}", e),
            }
        }
        Err(e) => format!("Error moving mouse: {}", e),
    }
}

async fn handle_vm_mouse_drag_to(manager: &VmManager, args: &serde_json::Value) -> String {
    let from_x = args["from_x"].as_i64().unwrap_or(0) as i32;
    let from_y = args["from_y"].as_i64().unwrap_or(0) as i32;
    let to_x = args["to_x"].as_i64().unwrap_or(0) as i32;
    let to_y = args["to_y"].as_i64().unwrap_or(0) as i32;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    match manager.send_mouse(name, "drag", Some(from_x), Some(from_y), Some(to_x), Some(to_y), None, None, None).await {
        Ok(_) => format!("Dragged from ({},{}) to ({},{})", from_x, from_y, to_x, to_y),
        Err(e) => format!("Error dragging: {}", e),
    }
}

async fn handle_vm_mouse_scroll_at(manager: &VmManager, args: &serde_json::Value) -> String {
    let x = args["x"].as_i64().unwrap_or(0) as i32;
    let y = args["y"].as_i64().unwrap_or(0) as i32;
    let direction = args["direction"].as_str().unwrap_or("down");
    let amount = args["amount"].as_i64().unwrap_or(3) as i32;
    let name = args["name"].as_str().unwrap_or("praxis-vm");
    
    // Move to position first
    let _ = manager.send_mouse(name, "move_absolute", Some(x), Some(y), None, None, None, None, None).await;
    
    // Calculate scroll values based on direction
    let (vertical, horizontal) = match direction {
        "up" => (amount, 0),
        "down" => (-amount, 0),
        "left" => (0, -amount),
        "right" => (0, amount),
        _ => return format!("Unknown direction: {}. Use: up, down, left, right", direction),
    };
    
    match manager.send_mouse(name, "scroll", Some(x), Some(y), None, None, None, Some(vertical), Some(horizontal)).await {
        Ok(_) => format!("Scrolled {} at ({}, {})", direction, x, y),
        Err(e) => format!("Error scrolling: {}", e),
    }
}
