use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub struct QmpClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

#[derive(Debug, Serialize, Deserialize)]
struct QmpCommand {
    execute: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

impl QmpClient {
    pub async fn connect(socket_path: &str) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(|e| anyhow::anyhow!("Cannot connect to QMP socket {}: {}", socket_path, e))?;
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
    }

    async fn read_line(&mut self) -> anyhow::Result<String> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        Ok(line)
    }

    async fn write_cmd(&mut self, cmd: &QmpCommand) -> anyhow::Result<()> {
        let cmd_json = serde_json::to_string(cmd)? + "\n";
        self.writer.write_all(cmd_json.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Negotiate QMP capabilities (must be called first)
    pub async fn negotiate(&mut self) -> anyhow::Result<()> {
        // Read greeting
        let _greeting = self.read_line().await?;

        // Send qmp_capabilities
        self.write_cmd(&QmpCommand {
            execute: "qmp_capabilities".to_string(),
            arguments: None,
        })
        .await?;

        // Read response, skip events
        let mut response = self.read_line().await?;
        while response.contains("\"event\"") {
            response = self.read_line().await?;
        }

        if response.contains("\"error\"") {
            anyhow::bail!("QMP negotiation failed: {}", response);
        }

        Ok(())
    }

    /// Send a generic QMP command and read response
    async fn execute(&mut self, cmd: QmpCommand) -> anyhow::Result<serde_json::Value> {
        self.write_cmd(&cmd).await?;

        let mut line = self.read_line().await?;

        // Skip events
        while line.contains("\"event\"") {
            line = self.read_line().await?;
        }

        let response: serde_json::Value = serde_json::from_str(&line)?;

        if let Some(err) = response.get("error") {
            anyhow::bail!("QMP error: {}", err);
        }

        Ok(response
            .get("return")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Graceful system powerdown
    pub async fn system_powerdown(&mut self) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "system_powerdown".to_string(),
            arguments: None,
        })
        .await?;
        Ok(())
    }

    /// Force system reset
    pub async fn system_reset(&mut self) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "system_reset".to_string(),
            arguments: None,
        })
        .await?;
        Ok(())
    }

    /// Pause the VM
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "stop".to_string(),
            arguments: None,
        })
        .await?;
        Ok(())
    }

    /// Resume the VM
    pub async fn cont(&mut self) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "cont".to_string(),
            arguments: None,
        })
        .await?;
        Ok(())
    }

    /// Save a screenshot of the display
    pub async fn screendump(&mut self, path: &str) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "screendump".to_string(),
            arguments: Some(serde_json::json!({
                "filename": path,
            })),
        })
        .await?;
        Ok(())
    }

    /// Send a key event
    pub async fn send_key_event(&mut self, key: &str, down: bool, up: bool) -> anyhow::Result<()> {
        let mut keys = Vec::new();
        if down {
            keys.push(serde_json::json!({
                "type": "qcode",
                "data": key,
            }));
        }

        self.execute(QmpCommand {
            execute: "input-send-event".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio-keyboard",
                "head": 0,
                "events": keys,
            })),
        })
        .await?;

        if up {
            self.execute(QmpCommand {
                execute: "input-send-event".to_string(),
                arguments: Some(serde_json::json!({
                    "device": "virtio-keyboard",
                    "head": 0,
                    "events": [{
                        "type": "qcode",
                        "data": key,
                    }],
                })),
            })
            .await?;
        }

        Ok(())
    }

    /// Move the mouse cursor to absolute position
    pub async fn mouse_move_absolute(&mut self, x: i32, y: i32) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "input-send-event".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio-mouse",
                "head": 0,
                "events": [{
                    "type": "abs",
                    "data": {
                        "axis": "x",
                        "value": x
                    }
                }, {
                    "type": "abs",
                    "data": {
                        "axis": "y",
                        "value": y
                    }
                }]
            })),
        })
        .await?;
        Ok(())
    }

    /// Move the mouse cursor relatively
    pub async fn mouse_move_relative(&mut self, dx: i32, dy: i32) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "input-send-event".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio-mouse",
                "head": 0,
                "events": [{
                    "type": "rel",
                    "data": {
                        "axis": "x",
                        "value": dx
                    }
                }, {
                    "type": "rel",
                    "data": {
                        "axis": "y",
                        "value": dy
                    }
                }]
            })),
        })
        .await?;
        Ok(())
    }

    /// Press or release a mouse button
    /// button: 0=left, 1=middle, 2=right
    pub async fn mouse_button(&mut self, button: i32, down: bool) -> anyhow::Result<()> {
        self.execute(QmpCommand {
            execute: "input-send-event".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio-mouse",
                "head": 0,
                "events": [{
                    "type": "btn",
                    "data": {
                        "button": match button {
                            0 => "left",
                            1 => "middle",
                            2 => "right",
                            _ => "left",
                        },
                        "down": down
                    }
                }]
            })),
        })
        .await?;
        Ok(())
    }

    /// Scroll the mouse wheel
    pub async fn mouse_scroll(&mut self, vertical: i32, horizontal: i32) -> anyhow::Result<()> {
        let mut events = Vec::new();
        if vertical != 0 {
            events.push(serde_json::json!({
                "type": "btn",
                "data": {
                    "button": if vertical > 0 { "wheel-up" } else { "wheel-down" },
                    "down": true
                }
            }));
        }
        if horizontal != 0 {
            events.push(serde_json::json!({
                "type": "btn",
                "data": {
                    "button": if horizontal > 0 { "wheel-left" } else { "wheel-right" },
                    "down": true
                }
            }));
        }
        self.execute(QmpCommand {
            execute: "input-send-event".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio-mouse",
                "head": 0,
                "events": events
            })),
        })
        .await?;
        Ok(())
    }

    /// Create a block device snapshot
    pub async fn blockdev_snapshotsync(&mut self, snapshot_name: &str) -> anyhow::Result<()> {
        let _ = self.stop().await;

        self.execute(QmpCommand {
            execute: "blockdev-snapshot-sync".to_string(),
            arguments: Some(serde_json::json!({
                "device": "virtio0",
                "snapshot-file": format!("{}.snap", snapshot_name),
                "format": "qcow2",
            })),
        })
        .await?;

        let _ = self.cont().await;
        Ok(())
    }

    /// Execute a guest command via QEMU Guest Agent
    pub async fn guest_exec(&mut self, command: &str, args: &[&str]) -> anyhow::Result<i64> {
        let result = self
            .execute(QmpCommand {
                execute: "guest-exec".to_string(),
                arguments: Some(serde_json::json!({
                    "path": command,
                    "arg": args,
                    "capture-output": true,
                })),
            })
            .await?;

        result
            .get("pid")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("No PID returned from guest-exec"))
    }

    /// Read guest-exec output
    pub async fn guest_exec_status(&mut self, pid: i64) -> anyhow::Result<serde_json::Value> {
        self.execute(QmpCommand {
            execute: "guest-exec-status".to_string(),
            arguments: Some(serde_json::json!({
                "pid": pid,
            })),
        })
        .await
    }

    /// Write a file in the guest via guest-file-write
    pub async fn guest_file_write(
        &mut self,
        path: &str,
        content_b64: &str,
        append: bool,
    ) -> anyhow::Result<()> {
        let open_result = self
            .execute(QmpCommand {
                execute: "guest-file-open".to_string(),
                arguments: Some(serde_json::json!({
                    "path": path,
                    "mode": if append { "a" } else { "w" },
                })),
            })
            .await?;

        let handle = open_result
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("No file handle returned"))?;

        self.execute(QmpCommand {
            execute: "guest-file-write".to_string(),
            arguments: Some(serde_json::json!({
                "handle": handle,
                "buf-b64": content_b64,
            })),
        })
        .await?;

        self.execute(QmpCommand {
            execute: "guest-file-close".to_string(),
            arguments: Some(serde_json::json!({
                "handle": handle,
            })),
        })
        .await?;

        Ok(())
    }
}
