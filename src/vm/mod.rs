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

        tracing::info!(name = %config.name, arch = ?config.arch, binary = %config.arch.qemu_binary(), "Starting VM");
        tracing::info!(
            "QEMU args: {} {}",
            config.arch.qemu_binary(),
            args.join(" ")
        );

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
        tracing::info!(name = %config.name, pid = ?pid, "QEMU process started");

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

    /// Execute a shell command in a VM
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

        let serial = instance
            .serial
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Serial not connected for VM '{}'", name))?;

        serial.execute(command, timeout_secs).await
    }

    /// Send raw keystrokes to a VM
    pub async fn send_keys(&self, name: &str, keys: &str) -> anyhow::Result<String> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

        if instance.status != VmStatus::Running {
            anyhow::bail!("VM '{}' is not running", name);
        }

        // Use serial for regular text, QMP for special keys
        if keys.starts_with("ctrl+")
            || keys.starts_with("alt+")
            || keys == "enter"
            || keys == "esc"
            || keys == "tab"
            || keys.starts_with("f1")
            || keys.starts_with("f2")
            || keys.starts_with("f3")
            || keys.starts_with("f4")
            || keys.starts_with("f5")
            || keys.starts_with("f6")
            || keys.starts_with("f7")
            || keys.starts_with("f8")
            || keys.starts_with("f9")
            || keys.starts_with("f10")
            || keys.starts_with("f11")
            || keys.starts_with("f12")
            || keys == "arrow_up"
            || keys == "arrow_down"
            || keys == "arrow_left"
            || keys == "arrow_right"
            || keys == "pageup"
            || keys == "pagedown"
            || keys == "home"
            || keys == "end"
            || keys == "insert"
            || keys == "delete"
            || keys == "backspace"
        {
            // Send via QMP input-send-event
            if let Some(ref mut qmp) = instance.qmp {
                let (keycode, down, up) = Self::keys_to_qmp(keys)?;
                qmp.send_key_event(&keycode, down, up).await?;
                return Ok(format!("Sent special key: {}", keys));
            }
            anyhow::bail!("QMP not connected for VM '{}'", name);
        } else {
            // Regular text — send via serial
            let serial = instance
                .serial
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Serial not connected for VM '{}'", name))?;
            serial.send_raw(keys).await?;
            return Ok(format!("Sent keys: {}", keys));
        }
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

        qmp.screendump(&screenshot_path).await?;

        // Read and convert PPM to base64 PNG if possible, otherwise return PPM base64
        let data = std::fs::read(&screenshot_path)?;
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

        // VNC
        let vnc_display = format!(":{}", config.vnc_port - 5900);
        args.extend([
            "-vnc".to_string(),
            vnc_display,
            "-display".to_string(),
            "none".to_string(),
        ]);

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

    fn keys_to_qmp(key: &str) -> anyhow::Result<(String, bool, bool)> {
        let keycode = match key {
            "enter" | "return" => "ret",
            "esc" | "escape" => "esc",
            "tab" => "tab",
            "backspace" => "backspace",
            "space" => "spc",
            "arrow_up" | "up" => "up",
            "arrow_down" | "down" => "down",
            "arrow_left" | "left" => "left",
            "arrow_right" | "right" => "right",
            "pageup" => "pgup",
            "pagedown" => "pgdn",
            "home" => "home",
            "end" => "end",
            "insert" => "insert",
            "delete" => "delete",
            "f1" => "f1",
            "f2" => "f2",
            "f3" => "f3",
            "f4" => "f4",
            "f5" => "f5",
            "f6" => "f6",
            "f7" => "f7",
            "f8" => "f8",
            "f9" => "f9",
            "f10" => "f10",
            "f11" => "f11",
            "f12" => "f12",
            "ctrl+c" => "ctrl-c",
            "ctrl+z" => "ctrl-z",
            "ctrl+d" => "ctrl-d",
            "ctrl+l" => "ctrl-l",
            "ctrl+a" => "ctrl-a",
            "ctrl+e" => "ctrl-e",
            "ctrl+x" => "ctrl-x",
            "ctrl+v" => "ctrl-v",
            "ctrl+w" => "ctrl-w",
            "alt+f4" => "alt-f4",
            "alt+tab" => "alt-tab",
            _ => anyhow::bail!("Unknown key: {}", key),
        };
        Ok((keycode.to_string(), true, true))
    }
}
