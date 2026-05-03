pub mod qmp;
pub mod secrets_inject;
pub mod serial;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VmStatus {
    Stopped,
    Running,
    Paused,
    Installing,
    Error,
}

impl std::fmt::Display for VmStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmStatus::Stopped => write!(f, "stopped"),
            VmStatus::Running => write!(f, "running"),
            VmStatus::Paused => write!(f, "paused"),
            VmStatus::Installing => write!(f, "installing"),
            VmStatus::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VmArch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl VmArch {
    pub fn qemu_binary(&self) -> &str {
        match self {
            VmArch::X86_64 => "qemu-system-x86_64",
            VmArch::Aarch64 => "qemu-system-aarch64",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "aarch64" => VmArch::Aarch64,
            _ => VmArch::X86_64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFolder {
    pub host_path: String,
    pub mount_tag: String,
    pub mount_point: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub name: String,
    pub arch: VmArch,
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub disk_path: String,
    pub disk_size: String,
    pub iso_path: Option<String>,
    pub vnc_port: u16,
    pub qmp_port: u16,
    pub serial_port: u16,
    pub qmp_socket_path: Option<String>, // Unix socket path (only when socket_mode=unix)
    pub serial_socket_path: Option<String>, // Unix socket path (only when socket_mode=unix)
    pub socket_mode: String,             // "unix" or "tcp"
    pub shared_folders: Vec<SharedFolder>,
    pub network_mode: String,
    pub audio_enabled: bool,
}

/// Detect the best acceleration method for the current OS
fn detect_acceleration() -> &'static str {
    if cfg!(target_os = "linux") {
        // Check if KVM is available
        if std::path::Path::new("/dev/kvm").exists() {
            return "kvm";
        }
    } else if cfg!(target_os = "windows") {
        // WHPX (Windows Hypervisor Platform) or HAXM
        return "whpx";
    } else if cfg!(target_os = "macos") {
        return "hvf";
    }
    "tcg" // fallback: software emulation (slow but works everywhere)
}

/// Find OVMF firmware for UEFI boot
fn find_ovmf_firmware() -> Option<String> {
    let paths = if cfg!(target_os = "windows") {
        vec![
            r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
            r"C:\Program Files\qemu\share\OVMF_CODE.fd",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "/opt/homebrew/share/qemu/edk2-x86_64-code.fd",
            "/usr/local/share/qemu/edk2-x86_64-code.fd",
        ]
    } else {
        vec![
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/qemu/OVMF_CODE.fd",
        ]
    };
    for p in paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

impl VmConfig {
    pub fn default_for_name(name: &str, data_dir: &str, vnc_offset: u16, arch: &str) -> Self {
        let vm_dir = format!("{}/vm/{}", data_dir, name);
        let base_port = 44400u16 + (vnc_offset * 10);
        let socket_mode = std::env::var("VM_SOCKET_MODE").unwrap_or_else(|_| {
            if cfg!(target_os = "linux") {
                "unix".to_string()
            } else {
                "tcp".to_string()
            }
        });

        let (qmp_socket_path, serial_socket_path) = if socket_mode == "unix" {
            (
                Some(format!("{}/qmp.sock", vm_dir)),
                Some(format!("{}/serial.sock", vm_dir)),
            )
        } else {
            (None, None)
        };

        Self {
            name: name.to_string(),
            arch: VmArch::from_str(arch),
            cpu_cores: 2,
            ram_mb: 4096,
            disk_path: format!("{}/disk.qcow2", vm_dir),
            disk_size: "40G".to_string(),
            iso_path: None,
            vnc_port: 5900 + vnc_offset,
            qmp_port: base_port,
            serial_port: base_port + 1,
            qmp_socket_path,
            serial_socket_path,
            socket_mode,
            shared_folders: Vec::new(),
            network_mode: "user".to_string(),
            audio_enabled: false,
        }
    }

    pub fn qmp_connect_addr(&self) -> String {
        if self.socket_mode == "unix" {
            self.qmp_socket_path
                .clone()
                .unwrap_or_else(|| format!("{}.sock", self.name))
        } else {
            format!("127.0.0.1:{}", self.qmp_port)
        }
    }

    pub fn serial_connect_addr(&self) -> String {
        if self.socket_mode == "unix" {
            self.serial_socket_path
                .clone()
                .unwrap_or_else(|| format!("{}.serial.sock", self.name))
        } else {
            format!("127.0.0.1:{}", self.serial_port)
        }
    }
}

/// Runtime state of a running VM
pub struct VmInstance {
    pub config: VmConfig,
    pub status: VmStatus,
    pub process: Option<tokio::process::Child>,
    pub qmp: Option<qmp::QmpClient>,
    pub serial: Option<serial::SerialShell>,
    pub pid: Option<u32>,
}

/// VM Manager — manages all VM instances
#[derive(Clone)]
pub struct VmManager {
    instances: Arc<RwLock<HashMap<String, VmInstance>>>,
    data_dir: String,
    next_vnc: Arc<RwLock<u16>>,
}

impl VmManager {
    pub fn new(data_dir: &str) -> Self {
        // Create VM folder structure
        let vm_base = format!("{}/vm", data_dir);
        let _ = std::fs::create_dir_all(&vm_base);
        let _ = std::fs::create_dir_all(format!("{}/isos", vm_base));
        let _ = std::fs::create_dir_all(format!("{}/disks", vm_base));
        let _ = std::fs::create_dir_all(format!("{}/shared", data_dir));

        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            data_dir: data_dir.to_string(),
            next_vnc: Arc::new(RwLock::new(1)),
        }
    }

    /// Get the base VM storage directory
    pub fn vm_dir(&self) -> String {
        format!("{}/vm", self.data_dir)
    }

    /// Get the ISO directory
    pub fn iso_dir(&self) -> String {
        format!("{}/vm/isos", self.data_dir)
    }

    /// List available ISOs in the iso directory
    pub fn list_isos(&self) -> Vec<serde_json::Value> {
        let iso_dir = self.iso_dir();
        let mut isos = Vec::new();

        // Scan the default iso dir
        Self::scan_iso_dir(&iso_dir, &mut isos);

        // Also scan context-configured paths
        let installation_disks = self.get_installation_disks();
        for disk in &installation_disks {
            if let Some(path) = disk.get("path").and_then(|v| v.as_str()) {
                let name = disk
                    .get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        std::path::Path::new(path)
                            .file_stem()
                            .and_then(|n| n.to_str())
                    })
                    .unwrap_or("unknown");
                let exists = std::path::Path::new(path).exists();
                isos.push(serde_json::json!({
                    "name": name,
                    "path": path,
                    "exists": exists,
                    "source": "context"
                }));
            }
        }

        isos
    }

    fn scan_iso_dir(dir: &str, isos: &mut Vec<serde_json::Value>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                let ext = path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if ext == "iso" || ext == "img" || ext == "qcow2" {
                    let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                    isos.push(serde_json::json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                        "size_bytes": size,
                        "exists": true,
                        "source": "iso_dir"
                    }));
                }
            }
        }
    }

    /// Get installation_disks from context (stored in a file for persistence)
    fn get_installation_disks(&self) -> Vec<serde_json::Value> {
        let config_path = format!("{}/vm/installation_disks.json", self.data_dir);
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(disks) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                return disks;
            }
        }
        Vec::new()
    }

    /// Save installation_disks to context file
    pub fn set_installation_disks(&self, disks: &[serde_json::Value]) -> anyhow::Result<()> {
        let config_path = format!("{}/vm/installation_disks.json", self.data_dir);
        let content = serde_json::to_string_pretty(disks)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// Add an ISO path to the installation disks list
    pub fn add_installation_disk(&self, name: &str, path: &str) -> anyhow::Result<()> {
        let mut disks = self.get_installation_disks();
        // Check if already exists
        if disks
            .iter()
            .any(|d| d.get("path").and_then(|v| v.as_str()) == Some(path))
        {
            return Ok(());
        }
        disks.push(serde_json::json!({ "name": name, "path": path }));
        self.set_installation_disks(&disks)
    }

    /// Remove an ISO from the installation disks list
    pub fn remove_installation_disk(&self, name_or_path: &str) -> anyhow::Result<()> {
        let mut disks = self.get_installation_disks();
        disks.retain(|d| {
            d.get("name").and_then(|v| v.as_str()) != Some(name_or_path)
                && d.get("path").and_then(|v| v.as_str()) != Some(name_or_path)
        });
        self.set_installation_disks(&disks)
    }

    /// Start a VM with the given config
    pub async fn start_vm(&self, config: VmConfig) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        if let Some(inst) = instances.get(&config.name) {
            if inst.status == VmStatus::Running {
                return Ok(format!("VM '{}' is already running", config.name));
            }
        }

        // Kill any orphaned QEMU processes for this VM (from previous app sessions)
        let vm_dir = format!("{}/vm/{}", self.data_dir, config.name);
        let qmp_sock = format!("{}/qmp.sock", vm_dir);
        let serial_sock = format!("{}/serial.sock", vm_dir);
        // Find and kill QEMU processes that reference this VM's sockets or name
        if let Ok(output) = std::process::Command::new("pgrep")
            .args(["-f", &format!("qemu.*{}", config.name)])
            .output()
        {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid_str in pids.lines() {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    tracing::warn!(vm = %config.name, pid = pid, "Killing orphaned QEMU process");
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output();
                }
            }
        }
        // Clean up stale sockets
        let _ = std::fs::remove_file(&qmp_sock);
        let _ = std::fs::remove_file(&serial_sock);

        // Resolve all paths to absolute (QEMU needs absolute paths for sockets)
        let abs_data_dir = std::fs::canonicalize(&self.data_dir)
            .unwrap_or_else(|_| std::path::PathBuf::from(&self.data_dir));
        let abs_data_dir_str = abs_data_dir.to_string_lossy().to_string();
        let vm_dir = format!("{}/vm/{}", abs_data_dir_str, config.name);
        std::fs::create_dir_all(&vm_dir)?;

        // Fix relative paths in config
        let mut config = config.clone();
        if !std::path::Path::new(&config.disk_path).is_absolute() {
            config.disk_path = format!("{}/disk.qcow2", vm_dir);
        }
        if let Some(ref path) = config.qmp_socket_path {
            if !std::path::Path::new(path).is_absolute() {
                config.qmp_socket_path = Some(format!("{}/qmp.sock", vm_dir));
            }
        }
        if let Some(ref path) = config.serial_socket_path {
            if !std::path::Path::new(path).is_absolute() {
                config.serial_socket_path = Some(format!("{}/serial.sock", vm_dir));
            }
        }

        // Create disk if it doesn't exist
        if !std::path::Path::new(&config.disk_path).exists() {
            tracing::info!(disk = %config.disk_path, size = %config.disk_size, "Creating VM disk");
            let output = tokio::process::Command::new("qemu-img")
                .args([
                    "create",
                    "-f",
                    "qcow2",
                    &config.disk_path,
                    &config.disk_size,
                ])
                .output()
                .await?;
            if !output.status.success() {
                anyhow::bail!(
                    "Failed to create disk: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            tracing::info!("Disk created: {}", config.disk_path);
        }

        // Build QEMU command
        let args = self.build_qemu_args(&config);

        let vnc_port = config.vnc_port;
        let vnc_display_num = vnc_port - 5900;

        tracing::info!("=== VM START SEQUENCE ===");
        tracing::info!(vm = %config.name, arch = ?config.arch, binary = %config.arch.qemu_binary());
        tracing::info!("VNC: display :{} (port {})", vnc_display_num, vnc_port);
        tracing::info!("QMP socket: {:?}", config.qmp_socket_path);
        tracing::info!("Serial socket: {:?}", config.serial_socket_path);
        tracing::info!("Disk: {}", config.disk_path);
        tracing::info!("ISO: {:?}", config.iso_path);
        tracing::info!(
            "QEMU command: {} {}",
            config.arch.qemu_binary(),
            args.join(" ")
        );
        tracing::info!("========================");

        // Start QEMU process
        let mut cmd = tokio::process::Command::new(config.arch.qemu_binary());
        cmd.args(&args);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to start QEMU ({}): {}. Is qemu-system-x86_64 installed?",
                config.arch.qemu_binary(),
                e
            )
        })?;

        let pid = child.id();
        tracing::info!(vm = %config.name, pid = ?pid, "QEMU process started (PID: {:?})", pid);
        tracing::info!(
            "VNC available at: localhost:{} (display :{})",
            vnc_port,
            vnc_display_num
        );
        tracing::info!("VNC viewer: http://localhost:1337/vnc");

        // Check if QEMU died immediately (within 500ms)
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        match child.try_wait() {
            Ok(Some(status)) => {
                // QEMU exited already — capture stderr
                let stderr = child.stderr.take();
                let mut stderr_output = String::new();
                if let Some(mut stderr) = stderr {
                    use tokio::io::AsyncReadExt;
                    let _ = stderr.read_to_string(&mut stderr_output).await;
                }
                anyhow::bail!(
                    "QEMU exited immediately with status {}. Command: {} {}\nStderr: {}",
                    status,
                    config.arch.qemu_binary(),
                    args.join(" "),
                    stderr_output
                );
            }
            Ok(None) => {
                tracing::info!("QEMU process is running (PID: {:?})", pid);
            }
            Err(e) => {
                tracing::warn!("Could not check QEMU status: {}", e);
            }
        }

        // Wait for QEMU to start listening, with retries
        let qmp_addr = config.qmp_connect_addr();
        let serial_addr = config.serial_connect_addr();
        let mut qmp = None;
        let mut serial = None;

        for attempt in 1..=5 {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

            // Check if QEMU is still alive
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stderr = child.stderr.take();
                    let mut stderr_output = String::new();
                    if let Some(mut stderr) = stderr {
                        use tokio::io::AsyncReadExt;
                        let _ = stderr.read_to_string(&mut stderr_output).await;
                    }
                    anyhow::bail!(
                        "QEMU died during startup (exit {}). Stderr:\n{}",
                        status,
                        stderr_output
                    );
                }
                _ => {}
            }

            tracing::info!(attempt = attempt, qmp = %qmp_addr, "Trying QMP connection...");
            if qmp.is_none() {
                match qmp::QmpClient::connect(&qmp_addr).await {
                    Ok(mut client) => {
                        let _ = client.negotiate().await;
                        tracing::info!("QMP connected for VM '{}' at {}", config.name, qmp_addr);
                        qmp = Some(client);
                    }
                    Err(e) => {
                        tracing::debug!("QMP attempt {} failed: {}", attempt, e);
                    }
                }
            }

            if serial.is_none() {
                tracing::info!(attempt = attempt, serial = %serial_addr, "Trying Serial connection...");
                match serial::SerialShell::connect(&serial_addr).await {
                    Ok(shell) => {
                        tracing::info!(
                            "Serial connected for VM '{}' at {}",
                            config.name,
                            serial_addr
                        );
                        serial = Some(shell);
                    }
                    Err(e) => {
                        tracing::debug!("Serial attempt {} failed: {}", attempt, e);
                    }
                }
            }

            if qmp.is_some() && serial.is_some() {
                break;
            }
        }

        // Inject secrets via 9p if configured
        if let Err(e) = secrets_inject::inject_secrets(&config.name, &abs_data_dir_str).await {
            tracing::warn!("Secret injection failed (non-fatal): {}", e);
        }

        let instance = VmInstance {
            config: config.clone(),
            status: VmStatus::Running,
            process: Some(child),
            qmp,
            serial,
            pid,
        };

        instances.insert(config.name.clone(), instance);

        Ok(format!(
            "VM '{}' started (PID: {:?}, VNC: {}, QMP: {}, Serial: {})",
            config.name,
            pid,
            config.vnc_port,
            config.qmp_connect_addr(),
            config.serial_connect_addr()
        ))
    }

    /// Stop a VM gracefully
    pub async fn stop_vm(&self, name: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            return Ok(format!("VM '{}' is not running", name));
        }

        // Try QMP powerdown first (graceful)
        if let Some(ref mut qmp) = instance.qmp {
            let _ = qmp.system_powerdown().await;
            tracing::info!("Sent system_powerdown to VM '{}'", name);

            // Wait up to 10s for graceful shutdown
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Some(ref mut proc) = instance.process {
                    match proc.try_wait() {
                        Ok(Some(_)) => {
                            instance.status = VmStatus::Stopped;
                            instance.process = None;
                            instance.qmp = None;
                            instance.serial = None;
                            return Ok(format!("VM '{}' stopped gracefully", name));
                        }
                        _ => continue,
                    }
                }
            }
        }

        // Force kill if graceful failed
        if let Some(ref mut proc) = instance.process {
            proc.kill().await?;
            instance.status = VmStatus::Stopped;
            instance.process = None;
            instance.qmp = None;
            instance.serial = None;
            return Ok(format!("VM '{}' force-stopped", name));
        }

        Ok(format!("VM '{}' was not running", name))
    }

    /// Execute a shell command in a VM via QMP keystrokes (visible in VNC)
    pub async fn shell_exec(
        &self,
        name: &str,
        command: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        // Send command via QMP keystrokes (visible in VNC)
        if let Some(ref mut qmp) = instance.qmp {
            // Type each character via QMP key events
            for ch in command.chars() {
                let events = Self::char_to_qmp_events(ch)?;
                for (qcode, down) in events {
                    qmp.send_key_event(&qcode, down).await?;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            // Send enter
            qmp.send_key_event("ret", true).await?;
            qmp.send_key_event("ret", false).await?;

            // Wait for command to execute, then take screenshot
            tokio::time::sleep(std::time::Duration::from_millis(timeout_secs.min(5) * 1000)).await;

            let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let screenshot_path = format!("{}/vm/{}/screenshot.ppm", data_dir, name);
            let _ = qmp.screendump(&screenshot_path).await;

            return Ok(format!(
                "Command '{}' sent via QMP keystrokes. Output visible in VNC / screenshot.",
                command
            ));
        }

        // Fallback: use serial (not visible in VNC)
        let serial = instance
            .serial
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Neither QMP nor Serial connected for VM '{}'", name))?;

        serial.execute(command, timeout_secs).await
    }

    /// Send raw keystrokes to a VM (all input via QMP so it's visible in VNC)
    pub async fn send_keys(&self, name: &str, keys: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        let qmp = instance
            .qmp
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QMP not connected for VM '{}'", name))?;

        // Check if it's a single special key name
        let key_events = if Self::is_special_key(keys) {
            Self::keys_to_qmp(keys)?
        } else {
            // It's a string - send each character individually
            Self::string_to_qmp(keys)?
        };
        
        tracing::info!(keys = keys, events = ?key_events, "Sending QMP key events");
        for (qcode, down) in key_events {
            qmp.send_key_event(&qcode, down).await?;
            // Small delay between characters for reliability
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Ok(format!("Sent keys: {}", keys))
    }

    /// Check if the input is a known special key name
    fn is_special_key(key: &str) -> bool {
        // Check for modifier combinations like "ctrl+a", "alt+tab", etc.
        if key.contains('+') {
            return true;
        }
        matches!(key, 
            "enter" | "return" | "esc" | "escape" | "tab" | "backspace" | "space" |
            "capslock" | "caps_lock" | "numlock" | "num_lock" | "scrolllock" | "scroll_lock" |
            "print" | "print_screen" | "prtsc" | "sysrq" |
            "arrow_up" | "up" | "arrow_down" | "down" | "arrow_left" | "left" | "arrow_right" | "right" |
            "pageup" | "page_up" | "pagedown" | "page_down" | "home" | "end" | "insert" | "delete" | "del" |
            "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" |
            "f13" | "f14" | "f15" | "f16" | "f17" | "f18" | "f19" | "f20" | "f21" | "f22" | "f23" | "f24" |
            "kp0" | "numpad0" | "kp1" | "numpad1" | "kp2" | "numpad2" | "kp3" | "numpad3" |
            "kp4" | "numpad4" | "kp5" | "numpad5" | "kp6" | "numpad6" | "kp7" | "numpad7" |
            "kp8" | "numpad8" | "kp9" | "numpad9" | "kp_enter" | "numpad_enter" |
            "kp_plus" | "numpad_plus" | "kp_minus" | "numpad_minus" |
            "kp_multiply" | "numpad_multiply" | "kp_divide" | "numpad_divide" |
            "kp_dot" | "numpad_dot" |
            "ctrl" | "left_ctrl" | "right_ctrl" | "alt" | "left_alt" | "right_alt" |
            "shift" | "left_shift" | "right_shift" | "super" | "meta" | "win" | "left_meta" | "right_meta"
        )
    }

    /// Convert a string to QMP key events, handling special characters and modifier combos
    fn string_to_qmp(s: &str) -> anyhow::Result<Vec<(String, bool)>> {
        let mut events = Vec::new();
        
        // Check for modifier combinations like "ctrl+a", "alt+tab", "ctrl+alt+delete"
        if s.contains('+') {
            let parts: Vec<&str> = s.split('+').collect();
            let mut modifiers = Vec::new();
            let mut main_key = None;
            
            for part in &parts {
                let part_lower = part.to_lowercase();
                match part_lower.as_str() {
                    "ctrl" | "control" => modifiers.push("ctrl"),
                    "alt" => modifiers.push("alt"),
                    "shift" => modifiers.push("shift"),
                    "super" | "meta" | "win" => modifiers.push("meta_l"),
                    _ => main_key = Some(part_lower),
                }
            }
            
            // Press all modifiers down
            for mod_key in &modifiers {
                events.push((mod_key.to_string(), true));
            }
            
            // Press the main key
            if let Some(key) = main_key {
                let key_events = Self::keys_to_qmp(&key)?;
                events.extend(key_events);
            }
            
            // Release all modifiers (in reverse order)
            for mod_key in modifiers.iter().rev() {
                events.push((mod_key.to_string(), false));
            }
            
            return Ok(events);
        }
        
        // Regular string - send each character
        for c in s.chars() {
            let char_events = match c {
                '\n' | '\r' => Self::keys_to_qmp("enter")?,
                '\t' => Self::keys_to_qmp("tab")?,
                ' ' => Self::keys_to_qmp("space")?,
                '\x08' => Self::keys_to_qmp("backspace")?,
                '\x1b' => Self::keys_to_qmp("esc")?,
                _ => {
                    let key = c.to_string();
                    Self::keys_to_qmp(&key)?
                }
            };
            events.extend(char_events);
        }
        Ok(events)
    }

    /// Send mouse input to the VM
    pub async fn send_mouse(
        &self,
        name: &str,
        action: &str,
        x: Option<i32>,
        y: Option<i32>,
        dx: Option<i32>,
        dy: Option<i32>,
        button: Option<i32>,
        scroll_vertical: Option<i32>,
        scroll_horizontal: Option<i32>,
    ) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        let qmp = instance
            .qmp
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QMP not connected for VM '{}'", name))?;

        match action {
            "move_absolute" => {
                let mx = x.unwrap_or(0);
                let my = y.unwrap_or(0);
                qmp.mouse_move_absolute(mx, my).await?;
                Ok(format!("Mouse moved to ({}, {})", mx, my))
            }
            "move_relative" => {
                let mdx = dx.unwrap_or(0);
                let mdy = dy.unwrap_or(0);
                qmp.mouse_move_relative(mdx, mdy).await?;
                Ok(format!("Mouse moved by ({}, {})", mdx, mdy))
            }
            "click" => {
                let btn = button.unwrap_or(0);
                qmp.mouse_button(btn, true).await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                qmp.mouse_button(btn, false).await?;
                let btn_name = match btn { 0 => "left", 1 => "middle", 2 => "right", _ => "left" };
                Ok(format!("Mouse {} clicked at ({}, {})", btn_name, x.unwrap_or(-1), y.unwrap_or(-1)))
            }
            "double_click" => {
                let btn = button.unwrap_or(0);
                for _ in 0..2 {
                    qmp.mouse_button(btn, true).await?;
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                    qmp.mouse_button(btn, false).await?;
                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                }
                Ok(format!("Mouse double-clicked"))
            }
            "drag" => {
                // Move to start, press, move to end, release
                let sx = x.unwrap_or(0);
                let sy = y.unwrap_or(0);
                let ex = dx.unwrap_or(0);
                let ey = dy.unwrap_or(0);
                let btn = button.unwrap_or(0);
                qmp.mouse_move_absolute(sx, sy).await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                qmp.mouse_button(btn, true).await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                qmp.mouse_move_absolute(ex, ey).await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                qmp.mouse_button(btn, false).await?;
                Ok(format!("Mouse dragged from ({},{}) to ({},{})", sx, sy, ex, ey))
            }
            "scroll" => {
                let sv = scroll_vertical.unwrap_or(0);
                let sh = scroll_horizontal.unwrap_or(0);
                qmp.mouse_scroll(sv, sh).await?;
                Ok(format!("Mouse scrolled (vertical: {}, horizontal: {})", sv, sh))
            }
            _ => anyhow::bail!(
                "Unknown mouse action: {}. Use: move_absolute, move_relative, click, double_click, drag, scroll",
                action
            ),
        }
    }

    /// Take a screenshot of the VM display
    pub async fn screenshot(&self, name: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        let qmp = instance
            .qmp
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QMP not connected for VM '{}'", name))?;

        let vm_dir = format!("{}/vm/{}", self.data_dir, name);
        let screenshot_path = format!("{}/screenshot.ppm", vm_dir);

        tracing::debug!(vm = %name, path = %screenshot_path, "Taking screenshot via QMP screendump");

        // Small delay to let the display framebuffer update after input events
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        qmp.screendump(&screenshot_path).await?;

        let data = std::fs::read(&screenshot_path)?;
        tracing::debug!(vm = %name, size = data.len(), "Screenshot captured");

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

        Ok(format!("data:image/ppm;base64,{}", b64))
    }

    /// Get VM status info
    pub async fn get_vm_info(&self, name: &str) -> anyhow::Result<serde_json::Value> {
        let instances = self.instances.read().await;
        let instance = instances
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        Ok(serde_json::json!({
            "name": instance.config.name,
            "status": instance.status.to_string(),
            "arch": instance.config.arch.qemu_binary(),
            "cpu_cores": instance.config.cpu_cores,
            "ram_mb": instance.config.ram_mb,
            "disk_path": instance.config.disk_path,
            "disk_size": instance.config.disk_size,
            "vnc_port": instance.config.vnc_port,
            "qmp_port": instance.config.qmp_port,
            "serial_port": instance.config.serial_port,
            "pid": instance.pid,
            "shared_folders": instance.config.shared_folders,
            "network_mode": instance.config.network_mode,
            "audio_enabled": instance.config.audio_enabled,
        }))
    }

    /// List all VMs
    pub async fn list_vms(&self) -> Vec<serde_json::Value> {
        let instances = self.instances.read().await;
        instances
            .values()
            .map(|inst| {
                serde_json::json!({
                    "name": inst.config.name,
                    "status": inst.status.to_string(),
                    "pid": inst.pid,
                    "vnc_port": inst.config.vnc_port,
                })
            })
            .collect()
    }

    /// Create a snapshot
    pub async fn create_snapshot(&self, name: &str, snapshot_name: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        let qmp = instance
            .qmp
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QMP not connected"))?;

        qmp.blockdev_snapshotsync(snapshot_name).await?;
        Ok(format!(
            "Snapshot '{}' created for VM '{}'",
            snapshot_name, name
        ))
    }

    /// Insert or eject a CD/ISO in the VM
    pub async fn change_cd(&self, name: &str, iso_path: Option<&str>) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        let qmp = instance
            .qmp
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QMP not connected for VM '{}'", name))?;

        match iso_path {
            Some(path) => {
                if !std::path::Path::new(path).exists() {
                    anyhow::bail!("ISO file not found: {}", path);
                }
                qmp.blockdev_change_medium("cd0", path).await?;
                Ok(format!("CD '{}' inserted into VM '{}'", path, name))
            }
            None => {
                qmp.eject("cd0").await?;
                Ok(format!("CD ejected from VM '{}'", name))
            }
        }
    }

    /// Add a shared folder (requires VM restart)
    pub async fn add_shared_folder(
        &self,
        name: &str,
        folder: SharedFolder,
    ) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        instance.config.shared_folders.push(folder.clone());
        Ok(format!(
            "Shared folder added: {} -> {}. Restart VM to apply.",
            folder.host_path, folder.mount_point
        ))
    }

    /// Reconnect QMP/serial if VM is running but connections were lost
    pub async fn reconnect(&self, name: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        if instance.qmp.is_none() {
            let qmp_addr = instance.config.qmp_connect_addr();
            match qmp::QmpClient::connect(&qmp_addr).await {
                Ok(mut client) => {
                    let _ = client.negotiate().await;
                    instance.qmp = Some(client);
                    tracing::info!("QMP reconnected for VM '{}'", name);
                }
                Err(e) => {
                    tracing::warn!("QMP reconnect failed: {}", e);
                }
            }
        }

        if instance.serial.is_none() {
            let serial_addr = instance.config.serial_connect_addr();
            match serial::SerialShell::connect(&serial_addr).await {
                Ok(shell) => {
                    instance.serial = Some(shell);
                    tracing::info!("Serial reconnected for VM '{}'", name);
                }
                Err(e) => {
                    tracing::warn!("Serial reconnect failed: {}", e);
                }
            }
        }

        Ok(format!("Reconnection attempted for VM '{}'", name))
    }

    fn build_qemu_args(&self, config: &VmConfig) -> Vec<String> {
        let accel = detect_acceleration();
        let cpu = if accel == "kvm" { "host" } else { "max" };

        let mut args = vec![
            "-name".to_string(),
            config.name.clone(),
            "-machine".to_string(),
            format!("q35,accel={}", accel),
            "-cpu".to_string(),
            cpu.to_string(),
            "-smp".to_string(),
            config.cpu_cores.to_string(),
            "-m".to_string(),
            config.ram_mb.to_string(),
        ];

        // Disk
        args.extend([
            "-drive".to_string(),
            format!("file={},format=qcow2,if=virtio", config.disk_path),
        ]);

        // CD-ROM device (always present for hot-plug support)
        if let Some(ref iso) = config.iso_path {
            args.extend([
                "-drive".to_string(),
                format!("file={},readonly=on,media=cdrom,if=none,id=cd0", iso),
            ]);
        } else {
            args.extend([
                "-drive".to_string(),
                "if=none,id=cd0,media=cdrom".to_string(),
            ]);
        }
        args.extend(["-device".to_string(), "ide-cd,drive=cd0".to_string()]);
        if config.iso_path.is_some() {
            args.extend(["-boot".to_string(), "d".to_string()]);
        }

        // VNC with explicit VGA device
        let vnc_display = format!(":{}", config.vnc_port - 5900);
        args.extend(["-vnc".to_string(), vnc_display]);

        // QMP: Unix socket or TCP based on config
        if config.socket_mode == "unix" {
            let qmp_path = config.qmp_socket_path.as_deref().unwrap_or("/tmp/qmp.sock");
            args.extend([
                "-qmp".to_string(),
                format!("unix:{},server,nowait", qmp_path),
            ]);
        } else {
            args.extend([
                "-qmp".to_string(),
                format!("tcp:127.0.0.1:{},server,nowait", config.qmp_port),
            ]);
        }

        // Serial: Unix socket or TCP based on config
        if config.socket_mode == "unix" {
            let serial_path = config
                .serial_socket_path
                .as_deref()
                .unwrap_or("/tmp/serial.sock");
            args.extend([
                "-chardev".to_string(),
                format!("socket,id=serial0,path={},server=on,wait=off", serial_path),
                "-serial".to_string(),
                "chardev:serial0".to_string(),
            ]);
        } else {
            args.extend([
                "-chardev".to_string(),
                format!(
                    "socket,id=serial0,host=127.0.0.1,port={},server=on,wait=off",
                    config.serial_port
                ),
                "-serial".to_string(),
                "chardev:serial0".to_string(),
            ]);
        }

        // Shared folders: 9p on Linux, SMB on Windows/macOS
        if cfg!(target_os = "linux") {
            for folder in &config.shared_folders {
                let security = if folder.readonly {
                    "passthrough,readonly"
                } else {
                    "passthrough"
                };
                let abs_host_path = std::fs::canonicalize(&folder.host_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| folder.host_path.clone());
                args.extend([
                    "-virtfs".to_string(),
                    format!(
                        "local,path={},mount_tag={},security_model={}",
                        abs_host_path, folder.mount_tag, security
                    ),
                ]);
            }
            // Secrets 9p mount — absolute path
            let abs_data = std::fs::canonicalize(&self.data_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| self.data_dir.clone());
            let secrets_dir = format!("{}/vm/{}/secrets", abs_data, config.name);
            let _ = std::fs::create_dir_all(&secrets_dir);
            args.extend([
                "-virtfs".to_string(),
                format!(
                    "local,path={},mount_tag=praxis-secrets,security_model=none,readonly",
                    secrets_dir
                ),
            ]);
        } else {
            // On Windows/macOS: use QEMU's built-in SMB server for shared folders
            // -smb <dir> exposes the directory as //10.0.2.4/qemu
            if !config.shared_folders.is_empty() {
                // Use the first shared folder as the SMB share
                args.extend([
                    "-smb".to_string(),
                    config.shared_folders[0].host_path.clone(),
                ]);
            }
        }

        // Network
        match config.network_mode.as_str() {
            "none" => {
                args.extend(["-net".to_string(), "none".to_string()]);
            }
            "bridge" => {
                args.extend([
                    "-device".to_string(),
                    "virtio-net,netdev=net0".to_string(),
                    "-netdev".to_string(),
                    "bridge,id=net0,br=br0".to_string(),
                ]);
            }
            _ => {
                args.extend([
                    "-device".to_string(),
                    "virtio-net,netdev=net0".to_string(),
                    "-netdev".to_string(),
                    "user,id=net0,hostfwd=tcp::2222-:22".to_string(),
                ]);
            }
        }

        // Audio: platform-aware
        if config.audio_enabled {
            if cfg!(target_os = "linux") {
                args.extend(["-audiodev".to_string(), "pa,id=audio0".to_string()]);
            } else if cfg!(target_os = "windows") {
                args.extend(["-audiodev".to_string(), "dsound,id=audio0".to_string()]);
            } else {
                args.extend(["-audiodev".to_string(), "sdl,id=audio0".to_string()]);
            }
            args.extend(["-device".to_string(), "AC97,audiodev=audio0".to_string()]);
        }

        // UEFI firmware (OS-specific paths)
        if let Some(fw) = find_ovmf_firmware() {
            args.extend(["-bios".to_string(), fw]);
        }

        args
    }

    fn keys_to_qmp(key: &str) -> anyhow::Result<Vec<(String, bool)>> {
        // QEMU QCode strings from qemu-qmp-ref.html
        let press = |k: &str| vec![(k.to_string(), true), (k.to_string(), false)];

        match key {
            // Basic keys
            "enter" | "return" => Ok(press("ret")),
            "esc" | "escape" => Ok(press("esc")),
            "tab" => Ok(press("tab")),
            "backspace" => Ok(press("backspace")),
            "space" => Ok(press("spc")),
            "capslock" | "caps_lock" => Ok(press("caps_lock")),
            "numlock" | "num_lock" => Ok(press("num_lock")),
            "scrolllock" | "scroll_lock" => Ok(press("scroll_lock")),
            "print" | "print_screen" | "prtsc" | "sysrq" => Ok(press("print")),

            // Arrow keys
            "arrow_up" | "up" => Ok(press("up")),
            "arrow_down" | "down" => Ok(press("down")),
            "arrow_left" | "left" => Ok(press("left")),
            "arrow_right" | "right" => Ok(press("right")),

            // Navigation
            "pageup" | "page_up" => Ok(press("pgup")),
            "pagedown" | "page_down" => Ok(press("pgdn")),
            "home" => Ok(press("home")),
            "end" => Ok(press("end")),
            "insert" => Ok(press("insert")),
            "delete" | "del" => Ok(press("delete")),

            // Function keys
            "f1" => Ok(press("f1")),
            "f2" => Ok(press("f2")),
            "f3" => Ok(press("f3")),
            "f4" => Ok(press("f4")),
            "f5" => Ok(press("f5")),
            "f6" => Ok(press("f6")),
            "f7" => Ok(press("f7")),
            "f8" => Ok(press("f8")),
            "f9" => Ok(press("f9")),
            "f10" => Ok(press("f10")),
            "f11" => Ok(press("f11")),
            "f12" => Ok(press("f12")),
            "f13" => Ok(press("f13")),
            "f14" => Ok(press("f14")),
            "f15" => Ok(press("f15")),
            "f16" => Ok(press("f16")),
            "f17" => Ok(press("f17")),
            "f18" => Ok(press("f18")),
            "f19" => Ok(press("f19")),
            "f20" => Ok(press("f20")),
            "f21" => Ok(press("f21")),
            "f22" => Ok(press("f22")),
            "f23" => Ok(press("f23")),
            "f24" => Ok(press("f24")),

            // Numpad
            "kp0" | "numpad0" => Ok(press("kp_0")),
            "kp1" | "numpad1" => Ok(press("kp_1")),
            "kp2" | "numpad2" => Ok(press("kp_2")),
            "kp3" | "numpad3" => Ok(press("kp_3")),
            "kp4" | "numpad4" => Ok(press("kp_4")),
            "kp5" | "numpad5" => Ok(press("kp_5")),
            "kp6" | "numpad6" => Ok(press("kp_6")),
            "kp7" | "numpad7" => Ok(press("kp_7")),
            "kp8" | "numpad8" => Ok(press("kp_8")),
            "kp9" | "numpad9" => Ok(press("kp_9")),
            "kp_enter" | "numpad_enter" => Ok(press("kp_enter")),
            "kp_plus" | "numpad_plus" => Ok(press("kp_add")),
            "kp_minus" | "numpad_minus" => Ok(press("kp_subtract")),
            "kp_multiply" | "numpad_multiply" => Ok(press("kp_multiply")),
            "kp_divide" | "numpad_divide" => Ok(press("kp_divide")),
            "kp_dot" | "numpad_dot" => Ok(press("kp_decimal")),

            // Modifier keys (standalone press)
            "ctrl" | "left_ctrl" => Ok(press("ctrl")),
            "right_ctrl" => Ok(press("ctrl_r")),
            "alt" | "left_alt" => Ok(press("alt")),
            "right_alt" => Ok(press("alt_r")),
            "shift" | "left_shift" => Ok(press("shift")),
            "right_shift" => Ok(press("shift_r")),
            "super" | "meta" | "win" | "left_meta" => Ok(press("meta_l")),
            "right_meta" | "right_super" => Ok(press("meta_r")),

            // Single characters (a-z)
            "a" => Ok(press("a")),
            "b" => Ok(press("b")),
            "c" => Ok(press("c")),
            "d" => Ok(press("d")),
            "e" => Ok(press("e")),
            "f" => Ok(press("f")),
            "g" => Ok(press("g")),
            "h" => Ok(press("h")),
            "i" => Ok(press("i")),
            "j" => Ok(press("j")),
            "k" => Ok(press("k")),
            "l" => Ok(press("l")),
            "m" => Ok(press("m")),
            "n" => Ok(press("n")),
            "o" => Ok(press("o")),
            "p" => Ok(press("p")),
            "q" => Ok(press("q")),
            "r" => Ok(press("r")),
            "s" => Ok(press("s")),
            "t" => Ok(press("t")),
            "u" => Ok(press("u")),
            "v" => Ok(press("v")),
            "w" => Ok(press("w")),
            "x" => Ok(press("x")),
            "y" => Ok(press("y")),
            "z" => Ok(press("z")),

            // Digits
            "0" => Ok(press("0")),
            "1" => Ok(press("1")),
            "2" => Ok(press("2")),
            "3" => Ok(press("3")),
            "4" => Ok(press("4")),
            "5" => Ok(press("5")),
            "6" => Ok(press("6")),
            "7" => Ok(press("7")),
            "8" => Ok(press("8")),
            "9" => Ok(press("9")),

            // Symbol keys (unshifted)
            "-" => Ok(press("minus")),
            "=" => Ok(press("equal")),
            "[" => Ok(press("bracket_left")),
            "]" => Ok(press("bracket_right")),
            "\\" => Ok(press("backslash")),
            ";" => Ok(press("semicolon")),
            "'" => Ok(press("apostrophe")),
            "`" => Ok(press("grave_accent")),
            "," => Ok(press("comma")),
            "." => Ok(press("dot")),
            "/" => Ok(press("slash")),

            // Shifted symbol keys (shift + base key)
            "!" => Ok(vec![
                ("shift".into(), true),
                ("1".into(), true),
                ("1".into(), false),
                ("shift".into(), false),
            ]),
            "@" => Ok(vec![
                ("shift".into(), true),
                ("2".into(), true),
                ("2".into(), false),
                ("shift".into(), false),
            ]),
            "#" => Ok(vec![
                ("shift".into(), true),
                ("3".into(), true),
                ("3".into(), false),
                ("shift".into(), false),
            ]),
            "$" => Ok(vec![
                ("shift".into(), true),
                ("4".into(), true),
                ("4".into(), false),
                ("shift".into(), false),
            ]),
            "%" => Ok(vec![
                ("shift".into(), true),
                ("5".into(), true),
                ("5".into(), false),
                ("shift".into(), false),
            ]),
            "^" => Ok(vec![
                ("shift".into(), true),
                ("6".into(), true),
                ("6".into(), false),
                ("shift".into(), false),
            ]),
            "&" => Ok(vec![
                ("shift".into(), true),
                ("7".into(), true),
                ("7".into(), false),
                ("shift".into(), false),
            ]),
            "*" => Ok(vec![
                ("shift".into(), true),
                ("8".into(), true),
                ("8".into(), false),
                ("shift".into(), false),
            ]),
            "(" => Ok(vec![
                ("shift".into(), true),
                ("9".into(), true),
                ("9".into(), false),
                ("shift".into(), false),
            ]),
            ")" => Ok(vec![
                ("shift".into(), true),
                ("0".into(), true),
                ("0".into(), false),
                ("shift".into(), false),
            ]),
            "_" => Ok(vec![
                ("shift".into(), true),
                ("minus".into(), true),
                ("minus".into(), false),
                ("shift".into(), false),
            ]),
            "+" => Ok(vec![
                ("shift".into(), true),
                ("equal".into(), true),
                ("equal".into(), false),
                ("shift".into(), false),
            ]),
            "{" => Ok(vec![
                ("shift".into(), true),
                ("bracket_left".into(), true),
                ("bracket_left".into(), false),
                ("shift".into(), false),
            ]),
            "}" => Ok(vec![
                ("shift".into(), true),
                ("bracket_right".into(), true),
                ("bracket_right".into(), false),
                ("shift".into(), false),
            ]),
            "|" => Ok(vec![
                ("shift".into(), true),
                ("backslash".into(), true),
                ("backslash".into(), false),
                ("shift".into(), false),
            ]),
            ":" => Ok(vec![
                ("shift".into(), true),
                ("semicolon".into(), true),
                ("semicolon".into(), false),
                ("shift".into(), false),
            ]),
            "\"" => Ok(vec![
                ("shift".into(), true),
                ("apostrophe".into(), true),
                ("apostrophe".into(), false),
                ("shift".into(), false),
            ]),
            "~" => Ok(vec![
                ("shift".into(), true),
                ("grave_accent".into(), true),
                ("grave_accent".into(), false),
                ("shift".into(), false),
            ]),
            "<" => Ok(vec![
                ("shift".into(), true),
                ("comma".into(), true),
                ("comma".into(), false),
                ("shift".into(), false),
            ]),
            ">" => Ok(vec![
                ("shift".into(), true),
                ("dot".into(), true),
                ("dot".into(), false),
                ("shift".into(), false),
            ]),
            "?" => Ok(vec![
                ("shift".into(), true),
                ("slash".into(), true),
                ("slash".into(), false),
                ("shift".into(), false),
            ]),

            // Modifier combos
            "ctrl+a" => Ok(vec![
                ("ctrl".into(), true),
                ("a".into(), true),
                ("a".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+b" => Ok(vec![
                ("ctrl".into(), true),
                ("b".into(), true),
                ("b".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+c" => Ok(vec![
                ("ctrl".into(), true),
                ("c".into(), true),
                ("c".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+d" => Ok(vec![
                ("ctrl".into(), true),
                ("d".into(), true),
                ("d".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+e" => Ok(vec![
                ("ctrl".into(), true),
                ("e".into(), true),
                ("e".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+f" => Ok(vec![
                ("ctrl".into(), true),
                ("f".into(), true),
                ("f".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+g" => Ok(vec![
                ("ctrl".into(), true),
                ("g".into(), true),
                ("g".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+h" => Ok(vec![
                ("ctrl".into(), true),
                ("h".into(), true),
                ("h".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+i" => Ok(vec![
                ("ctrl".into(), true),
                ("i".into(), true),
                ("i".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+j" => Ok(vec![
                ("ctrl".into(), true),
                ("j".into(), true),
                ("j".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+k" => Ok(vec![
                ("ctrl".into(), true),
                ("k".into(), true),
                ("k".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+l" => Ok(vec![
                ("ctrl".into(), true),
                ("l".into(), true),
                ("l".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+m" => Ok(vec![
                ("ctrl".into(), true),
                ("m".into(), true),
                ("m".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+n" => Ok(vec![
                ("ctrl".into(), true),
                ("n".into(), true),
                ("n".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+o" => Ok(vec![
                ("ctrl".into(), true),
                ("o".into(), true),
                ("o".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+p" => Ok(vec![
                ("ctrl".into(), true),
                ("p".into(), true),
                ("p".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+q" => Ok(vec![
                ("ctrl".into(), true),
                ("q".into(), true),
                ("q".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+r" => Ok(vec![
                ("ctrl".into(), true),
                ("r".into(), true),
                ("r".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+s" => Ok(vec![
                ("ctrl".into(), true),
                ("s".into(), true),
                ("s".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+t" => Ok(vec![
                ("ctrl".into(), true),
                ("t".into(), true),
                ("t".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+u" => Ok(vec![
                ("ctrl".into(), true),
                ("u".into(), true),
                ("u".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+v" => Ok(vec![
                ("ctrl".into(), true),
                ("v".into(), true),
                ("v".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+w" => Ok(vec![
                ("ctrl".into(), true),
                ("w".into(), true),
                ("w".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+x" => Ok(vec![
                ("ctrl".into(), true),
                ("x".into(), true),
                ("x".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+y" => Ok(vec![
                ("ctrl".into(), true),
                ("y".into(), true),
                ("y".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+z" => Ok(vec![
                ("ctrl".into(), true),
                ("z".into(), true),
                ("z".into(), false),
                ("ctrl".into(), false),
            ]),
            "alt+f1" => Ok(vec![
                ("alt".into(), true),
                ("f1".into(), true),
                ("f1".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f2" => Ok(vec![
                ("alt".into(), true),
                ("f2".into(), true),
                ("f2".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f3" => Ok(vec![
                ("alt".into(), true),
                ("f3".into(), true),
                ("f3".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f4" => Ok(vec![
                ("alt".into(), true),
                ("f4".into(), true),
                ("f4".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f5" => Ok(vec![
                ("alt".into(), true),
                ("f5".into(), true),
                ("f5".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f6" => Ok(vec![
                ("alt".into(), true),
                ("f6".into(), true),
                ("f6".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f7" => Ok(vec![
                ("alt".into(), true),
                ("f7".into(), true),
                ("f7".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f8" => Ok(vec![
                ("alt".into(), true),
                ("f8".into(), true),
                ("f8".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f9" => Ok(vec![
                ("alt".into(), true),
                ("f9".into(), true),
                ("f9".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f10" => Ok(vec![
                ("alt".into(), true),
                ("f10".into(), true),
                ("f10".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f11" => Ok(vec![
                ("alt".into(), true),
                ("f11".into(), true),
                ("f11".into(), false),
                ("alt".into(), false),
            ]),
            "alt+f12" => Ok(vec![
                ("alt".into(), true),
                ("f12".into(), true),
                ("f12".into(), false),
                ("alt".into(), false),
            ]),
            "alt+tab" => Ok(vec![
                ("alt".into(), true),
                ("tab".into(), true),
                ("tab".into(), false),
                ("alt".into(), false),
            ]),
            "alt+enter" => Ok(vec![
                ("alt".into(), true),
                ("ret".into(), true),
                ("ret".into(), false),
                ("alt".into(), false),
            ]),
            "ctrl+alt+delete" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("delete".into(), true),
                ("delete".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f1" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f1".into(), true),
                ("f1".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f2" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f2".into(), true),
                ("f2".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f3" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f3".into(), true),
                ("f3".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f4" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f4".into(), true),
                ("f4".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f5" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f5".into(), true),
                ("f5".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            "ctrl+alt+f6" => Ok(vec![
                ("ctrl".into(), true),
                ("alt".into(), true),
                ("f6".into(), true),
                ("f6".into(), false),
                ("alt".into(), false),
                ("ctrl".into(), false),
            ]),
            _ => anyhow::bail!("Unknown key: {}", key),
        }
    }

    /// Convert a single character to QMP key events (for typing text via QMP)
    fn char_to_qmp_events(ch: char) -> anyhow::Result<Vec<(String, bool)>> {
        let press = |k: &str| vec![(k.to_string(), true), (k.to_string(), false)];

        match ch {
            'a'..='z' => Ok(press(&ch.to_string())),
            '0'..='9' => Ok(press(&ch.to_string())),
            ' ' => Ok(press("spc")),
            '\n' => Ok(press("ret")),
            '\t' => Ok(press("tab")),
            '-' => Ok(press("minus")),
            '=' => Ok(press("equal")),
            '[' => Ok(press("bracket_left")),
            ']' => Ok(press("bracket_right")),
            '\\' => Ok(press("backslash")),
            ';' => Ok(press("semicolon")),
            '\'' => Ok(press("apostrophe")),
            '`' => Ok(press("grave_accent")),
            ',' => Ok(press("comma")),
            '.' => Ok(press("dot")),
            '/' => Ok(press("slash")),

            // Shifted characters
            'A'..='Z' => {
                let lower = ch.to_lowercase().to_string();
                Ok(vec![
                    ("shift".into(), true),
                    (lower.clone(), true),
                    (lower, false),
                    ("shift".into(), false),
                ])
            }
            '!' => Ok(vec![
                ("shift".into(), true),
                ("1".into(), true),
                ("1".into(), false),
                ("shift".into(), false),
            ]),
            '@' => Ok(vec![
                ("shift".into(), true),
                ("2".into(), true),
                ("2".into(), false),
                ("shift".into(), false),
            ]),
            '#' => Ok(vec![
                ("shift".into(), true),
                ("3".into(), true),
                ("3".into(), false),
                ("shift".into(), false),
            ]),
            '$' => Ok(vec![
                ("shift".into(), true),
                ("4".into(), true),
                ("4".into(), false),
                ("shift".into(), false),
            ]),
            '%' => Ok(vec![
                ("shift".into(), true),
                ("5".into(), true),
                ("5".into(), false),
                ("shift".into(), false),
            ]),
            '^' => Ok(vec![
                ("shift".into(), true),
                ("6".into(), true),
                ("6".into(), false),
                ("shift".into(), false),
            ]),
            '&' => Ok(vec![
                ("shift".into(), true),
                ("7".into(), true),
                ("7".into(), false),
                ("shift".into(), false),
            ]),
            '*' => Ok(vec![
                ("shift".into(), true),
                ("8".into(), true),
                ("8".into(), false),
                ("shift".into(), false),
            ]),
            '(' => Ok(vec![
                ("shift".into(), true),
                ("9".into(), true),
                ("9".into(), false),
                ("shift".into(), false),
            ]),
            ')' => Ok(vec![
                ("shift".into(), true),
                ("0".into(), true),
                ("0".into(), false),
                ("shift".into(), false),
            ]),
            '_' => Ok(vec![
                ("shift".into(), true),
                ("minus".into(), true),
                ("minus".into(), false),
                ("shift".into(), false),
            ]),
            '+' => Ok(vec![
                ("shift".into(), true),
                ("equal".into(), true),
                ("equal".into(), false),
                ("shift".into(), false),
            ]),
            '{' => Ok(vec![
                ("shift".into(), true),
                ("bracket_left".into(), true),
                ("bracket_left".into(), false),
                ("shift".into(), false),
            ]),
            '}' => Ok(vec![
                ("shift".into(), true),
                ("bracket_right".into(), true),
                ("bracket_right".into(), false),
                ("shift".into(), false),
            ]),
            '|' => Ok(vec![
                ("shift".into(), true),
                ("backslash".into(), true),
                ("backslash".into(), false),
                ("shift".into(), false),
            ]),
            ':' => Ok(vec![
                ("shift".into(), true),
                ("semicolon".into(), true),
                ("semicolon".into(), false),
                ("shift".into(), false),
            ]),
            '"' => Ok(vec![
                ("shift".into(), true),
                ("apostrophe".into(), true),
                ("apostrophe".into(), false),
                ("shift".into(), false),
            ]),
            '~' => Ok(vec![
                ("shift".into(), true),
                ("grave_accent".into(), true),
                ("grave_accent".into(), false),
                ("shift".into(), false),
            ]),
            '<' => Ok(vec![
                ("shift".into(), true),
                ("comma".into(), true),
                ("comma".into(), false),
                ("shift".into(), false),
            ]),
            '>' => Ok(vec![
                ("shift".into(), true),
                ("dot".into(), true),
                ("dot".into(), false),
                ("shift".into(), false),
            ]),
            '?' => Ok(vec![
                ("shift".into(), true),
                ("slash".into(), true),
                ("slash".into(), false),
                ("shift".into(), false),
            ]),

            _ => {
                tracing::warn!(char = %ch, "Unsupported character for QMP, skipping");
                Ok(vec![])
            }
        }
    }
}
