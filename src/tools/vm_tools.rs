use crate::vm::VmManager;
use std::sync::Arc;
use tokio::sync::OnceCell;

static VM_MANAGER: OnceCell<Arc<VmManager>> = OnceCell::const_new();

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
        "vm_screenshot" => handle_vm_screenshot(&manager, args).await,
        "vm_file_transfer" => handle_vm_file_transfer(&manager, args).await,
        "vm_snapshot" => handle_vm_snapshot(&manager, args).await,
        "vm_shared_folder" => handle_vm_shared_folder(&manager, args).await,
        "vm_mouse" => handle_vm_mouse(&manager, args).await,
        "vm_look_screenshot" => handle_vm_look_screenshot(&manager, args).await,
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

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let vnc_offset = {
        let instances = manager.list_vms().await;
        instances.len() as u16 + 1
    };

    let mut config = crate::vm::VmConfig::default_for_name(name, &data_dir, vnc_offset, arch);
    config.cpu_cores = cpu_cores;
    config.ram_mb = ram_mb;
    config.disk_size = disk_size.to_string();
    config.iso_path = iso_path;

    // Auto-add shared folders from data directory
    config.shared_folders.push(crate::vm::SharedFolder {
        host_path: format!("{}/shared", data_dir),
        mount_tag: "praxis-shared".to_string(),
        mount_point: "/mnt/shared".to_string(),
        readonly: false,
    });

    match manager.start_vm(config).await {
        Ok(msg) => msg,
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

    if keys.is_empty() {
        return "Error: keys is required".to_string();
    }

    match manager.send_keys(name, keys).await {
        Ok(msg) => msg,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_vm_screenshot(manager: &VmManager, args: &serde_json::Value) -> String {
    let name = args["name"].as_str().unwrap_or("praxis-vm");

    match manager.screenshot(name).await {
        Ok(data_url) => format!("Screenshot captured: {}", data_url),
        Err(e) => format!("Error taking screenshot: {}", e),
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
pub async fn take_screenshot_for_context(vm_name: &str) -> Option<String> {
    let manager = get_vm_manager().await?;
    match manager.screenshot(vm_name).await {
        Ok(data_url) => {
            // Enforce 500 screenshot limit
            cleanup_screenshot_limit();
            Some(data_url)
        }
        Err(_) => None,
    }
}

/// Save screenshot to disk and return path (for vm_look_screenshot)
pub async fn save_screenshot_to_disk(vm_name: &str, data_dir: &str) -> Option<String> {
    let manager = get_vm_manager().await?;
    let screenshot_data = manager.screenshot(vm_name).await.ok()?;

    let ss_dir = format!("{}/vm/{}/screenshots", data_dir, vm_name);
    let _ = std::fs::create_dir_all(&ss_dir);

    // Enforce limit before saving
    cleanup_screenshot_limit_dir(&ss_dir);

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("{}/screenshot_{}.ppm", ss_dir, ts);

    if let Some(b64) = screenshot_data.strip_prefix("data:image/ppm;base64,") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            let _ = std::fs::write(&filename, bytes);
            return Some(filename);
        }
    }
    None
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
        .filter(|e| e.file_name().to_string_lossy().ends_with(".ppm"))
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
        Some(path) => match std::fs::read(&path) {
            Ok(data) => {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                format!(
                    "Screenshot: {}\nTotal screenshots: {}\ndata:image/ppm;base64,{}",
                    path,
                    files.len(),
                    b64
                )
            }
            Err(e) => format!("Error reading screenshot: {}", e),
        },
        None => "No screenshot found at that index.".to_string(),
    }
}

/// Enforce 500 screenshot limit globally
fn cleanup_screenshot_limit() {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let vm_dir = format!("{}/vm", data_dir);
    if let Ok(entries) = std::fs::read_dir(&vm_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let ss_dir = entry.path().join("screenshots");
                cleanup_screenshot_limit_dir(&ss_dir.to_string_lossy());
            }
        }
    }
}

/// Enforce 500 screenshot limit in a specific directory
fn cleanup_screenshot_limit_dir(ss_dir: &str) {
    const MAX_SCREENSHOTS: usize = 500;

    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(ss_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".ppm"))
        .filter_map(|e| {
            let time = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((e.path(), time))
        })
        .collect();

    if files.len() <= MAX_SCREENSHOTS {
        return;
    }

    // Sort oldest first
    files.sort_by_key(|(_, t)| *t);

    // Delete oldest files to get back to limit
    let to_delete = files.len() - MAX_SCREENSHOTS;
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
