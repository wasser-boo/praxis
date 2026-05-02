use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub struct SerialShell {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl SerialShell {
    pub async fn connect(socket_path: &str) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket_path).await.map_err(|e| {
            anyhow::anyhow!("Cannot connect to serial socket {}: {}", socket_path, e)
        })?;
        Ok(Self {
            stream,
            buffer: Vec::with_capacity(8192),
        })
    }

    /// Execute a command and return its output
    /// Uses marker-based output detection to reliably capture command output
    pub async fn execute(&mut self, command: &str, timeout_secs: u64) -> anyhow::Result<String> {
        // Use a unique marker to detect command boundaries
        let marker_start = "<<<PRAXIS_START>>>";
        let marker_end = "<<<PRAXIS_END>>>";
        let marker_exit = "<<<PRAXIS_EXIT>>>";
        let marker_cmd = format!(
            "echo '{}'; {}; echo '{}:{}'; echo '{}'",
            marker_start, command, marker_exit, "$?", marker_end
        );

        // Clear any pending data
        self.drain_buffer().await;

        // Send the wrapped command
        self.send_line(&marker_cmd).await?;

        // Read until we see the end marker
        let output = self
            .read_until_markers(marker_start, marker_end, timeout_secs)
            .await?;

        // Parse exit code from output
        let exit_code = self.extract_exit_code(&output, marker_exit);

        // Clean the output: remove markers and command echo
        let cleaned =
            self.clean_output(&output, marker_start, marker_end, marker_exit, &marker_cmd);

        Ok(format!("{}\nexit_code: {}", cleaned.trim(), exit_code))
    }

    /// Send raw text (no newline appended) — for vm_keys tool
    pub async fn send_raw(&mut self, text: &str) -> anyhow::Result<()> {
        self.stream.write_all(text.as_bytes()).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Send a line (with newline)
    pub async fn send_line(&mut self, line: &str) -> anyhow::Result<()> {
        self.stream.write_all(line.as_bytes()).await?;
        self.stream.write_all(b"\n").await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Read raw output from serial (for vm_keys where we want to see what happened)
    pub async fn read_output(&mut self, timeout_secs: u64) -> anyhow::Result<String> {
        let mut output = Vec::new();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                break;
            }

            let remaining = timeout - start.elapsed();
            match tokio::time::timeout(remaining, self.stream.read_u8()).await {
                Ok(Ok(byte)) => {
                    output.push(byte);
                    // Check for shell prompt patterns
                    if output.ends_with(b"$ ") || output.ends_with(b"# ") || output.ends_with(b"> ")
                    {
                        break;
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => break, // Timeout
            }
        }

        Ok(String::from_utf8_lossy(&output).to_string())
    }

    async fn drain_buffer(&mut self) {
        let timeout = std::time::Duration::from_millis(200);
        loop {
            match tokio::time::timeout(timeout, self.stream.read_u8()).await {
                Ok(Ok(_)) => continue,
                _ => break,
            }
        }
    }

    async fn read_until_markers(
        &mut self,
        _start_marker: &str,
        end_marker: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<String> {
        let mut output = Vec::new();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                anyhow::bail!(
                    "Command timed out after {}s. Partial output: {}",
                    timeout_secs,
                    String::from_utf8_lossy(&output)
                );
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            match tokio::time::timeout(remaining, self.stream.read_u8()).await {
                Ok(Ok(byte)) => {
                    output.push(byte);
                    let output_str = String::from_utf8_lossy(&output);
                    if output_str.contains(end_marker) {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    if output.is_empty() {
                        anyhow::bail!("Serial read error: {}", e);
                    }
                    break;
                }
                Err(_) => {
                    if output.is_empty() {
                        anyhow::bail!("Command timed out after {}s", timeout_secs);
                    }
                    break;
                }
            }
        }

        Ok(String::from_utf8_lossy(&output).to_string())
    }

    fn extract_exit_code(&self, output: &str, marker_exit: &str) -> i32 {
        for line in output.lines() {
            if let Some(rest) = line.strip_prefix(marker_exit) {
                if let Some(code_str) = rest.strip_prefix(':') {
                    if let Ok(code) = code_str.trim().parse::<i32>() {
                        return code;
                    }
                }
            }
        }
        -1
    }

    fn clean_output(
        &self,
        output: &str,
        start_marker: &str,
        end_marker: &str,
        exit_marker: &str,
        cmd_line: &str,
    ) -> String {
        let mut result = String::new();
        let mut in_output = false;

        for line in output.lines() {
            let trimmed = line.trim();

            // Skip the command echo
            if trimmed.contains(cmd_line) {
                continue;
            }

            if trimmed.contains(start_marker) {
                in_output = true;
                // Check if there's content on the same line after the marker
                if let Some(rest) = trimmed.split_once(start_marker) {
                    let after = rest.1.trim();
                    if !after.is_empty() && !after.contains(end_marker) {
                        result.push_str(after);
                        result.push('\n');
                    }
                }
                continue;
            }

            if trimmed.contains(end_marker) {
                // Check for exit code on same line
                if let Some(idx) = trimmed.find(exit_marker) {
                    let before = &trimmed[..idx].trim();
                    if !before.is_empty() && in_output {
                        result.push_str(before);
                        result.push('\n');
                    }
                }
                in_output = false;
                continue;
            }

            if trimmed.contains(exit_marker) {
                continue;
            }

            if in_output {
                result.push_str(line);
                result.push('\n');
            }
        }

        result
    }
}
