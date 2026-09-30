//! Bounded, cancellable Codex CLI login. Relay output; never guess which line
//! contains the URL/code. Subsequent polls see output that arrived later.
use super::{commit, CodexAuth, GatewayState};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

static LOGIN: Mutex<Option<Job>> = Mutex::new(None);
const OUTPUT_LIMIT: usize = 16 * 1024;

struct Job {
    id: uuid::Uuid,
    output: Arc<Mutex<Vec<u8>>>,
    cancel: CancellationToken,
    error: Option<&'static str>,
}

pub(super) fn cancel() {
    if let Some(job) = LOGIN.lock().unwrap_or_else(|e| e.into_inner()).take() {
        job.cancel.cancel();
    }
}

pub(super) fn status() -> anyhow::Result<Option<String>> {
    let mut guard = LOGIN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(job) = guard.as_ref() {
        let instructions = instructions(&job.output);
        if let Some(error) = job.error {
            *guard = None;
            anyhow::bail!("{error}\n{instructions}\nRun /login codex to retry.");
        }
        return Ok(Some(instructions));
    }
    Ok(None)
}

fn instructions(output: &Mutex<Vec<u8>>) -> String {
    let bytes = output.lock().unwrap_or_else(|e| e.into_inner());
    let text = String::from_utf8_lossy(&bytes);
    // Strip terminal control sequences before displaying CLI output in a TUI.
    static ANSI: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\))").unwrap()
    });
    let text: String = ANSI
        .replace_all(&text, "")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    format!("Codex device login on the gateway. Follow the CLI instructions in any browser.\n{}\nPoll /login codex for updated output; /logout codex cancels.",
        if text.trim().is_empty() { "Waiting for CLI output…" } else { text.trim() })
}

fn reader(
    mut pipe: impl AsyncRead + Unpin + Send + 'static,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        while let Ok(n) = pipe.read(&mut buf).await {
            if n == 0 || tx.send(buf[..n].to_vec()).await.is_err() {
                break;
            }
        }
    })
}

fn append(output: &Mutex<Vec<u8>>, bytes: &[u8]) {
    let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
    // Keep the latest output so even a code printed after a noisy banner is
    // visible. Readers use bounded chunks and keep draining to avoid deadlock.
    out.extend_from_slice(bytes);
    let excess = out.len().saturating_sub(OUTPUT_LIMIT);
    if excess > 0 {
        out.drain(..excess);
    }
}

async fn run(
    mut child: tokio::process::Child,
    output: Arc<Mutex<Vec<u8>>>,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<(), &'static str> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        readers.push(reader(pipe, tx.clone()));
    }
    if let Some(pipe) = child.stderr.take() {
        readers.push(reader(pipe, tx.clone()));
    }
    drop(tx);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut pipes_open = true;
    let result = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Err("Codex device login cancelled"),
            _ = &mut deadline => break Err("Codex device login timed out"),
            bytes = rx.recv(), if pipes_open => match bytes {
                Some(bytes) => append(&output, &bytes),
                None => pipes_open = false,
            },
            status = child.wait() => break match status {
                Ok(status) if status.success() => Ok(()),
                _ => Err("Codex CLI login failed"),
            },
        }
    };
    if result.is_err() {
        #[cfg(unix)]
        if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
            // The npm `codex` launcher can spawn a Rust child. This process was
            // placed in its own group, so cancel the owned tree, not just Node.
            // SAFETY: a live, unreaped child's positive pid is its group id;
            // no pointers are passed and no unrelated process group is targeted.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = child.kill().await; // kill AND reap on cancellation/timeout
    }
    // A successful exit may precede the final pipe read. Drain for a bounded
    // interval even if an unexpected descendant keeps a pipe open.
    let _ = tokio::time::timeout(Duration::from_millis(200), async {
        while let Some(bytes) = rx.recv().await {
            append(&output, &bytes);
        }
    })
    .await;
    for task in readers {
        task.abort();
    }
    result
}

pub(super) async fn start(state: GatewayState, model: Option<String>) -> anyhow::Result<String> {
    {
        // Reserve the job before spawning; simultaneous /login calls share it.
        let mut guard = LOGIN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = guard.as_ref() {
            return Ok(instructions(&job.output));
        }
        let path = CodexAuth::cli_auth_path().ok_or_else(|| {
            anyhow::anyhow!("Set HOME or CODEX_HOME on the gateway for Codex login")
        })?;
        let mut command = tokio::process::Command::new("codex");
        #[cfg(unix)]
        command.process_group(0);
        let child = command.args(["login", "--device-auth"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .kill_on_drop(true).spawn()
            .map_err(|_| anyhow::anyhow!("Codex CLI unavailable on the gateway. Install @openai/codex or use /login codex --auth-json '<auth.json content>'"))?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();
        let id = uuid::Uuid::new_v4();
        *guard = Some(Job {
            id,
            output: output.clone(),
            cancel: cancel.clone(),
            error: None,
        });
        tokio::spawn(async move {
            let result = run(child, output, cancel, Duration::from_secs(900)).await;
            let result = result.and_then(|()| {
                let text = std::fs::read_to_string(path)
                    .map_err(|_| "Codex exited without a readable auth.json")?;
                CodexAuth::from_cli_file(&text)
                    .map_err(|_| "Codex auth.json contains no valid ChatGPT login")
            });
            let mut guard = LOGIN.lock().unwrap_or_else(|e| e.into_inner());
            let Some(job) = guard.as_mut().filter(|job| job.id == id) else {
                return;
            };
            // Hold the job lock through commit: /logout cannot race completion
            // and an old job cannot resurrect credentials after cancellation.
            match result {
                Ok(auth) => {
                    let mut secrets = crate::db::secrets::get_secrets();
                    auth.store(&mut secrets);
                    if let Some(model) = model {
                        secrets.custom.insert("codex_model".into(), model);
                    }
                    match commit(&state, "codex", secrets) {
                        Ok((persisted, _)) => {
                            tracing::info!(persisted, "Codex login completed");
                            *guard = None;
                        }
                        Err(_) => job.error = Some("Codex login could not be saved"),
                    }
                }
                Err(error) => job.error = Some(error),
            }
        });
    }
    // No dependency on CLI wording, line count, or when the code is printed.
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(status()?.unwrap_or_else(|| {
        "Codex login completed. Run /login codex for model setup instructions.".into()
    }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn child(script: &str) -> tokio::process::Child {
        tokio::process::Command::new("sh")
            .process_group(0)
            .args(["-c", script])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    #[tokio::test]
    async fn device_login_keeps_delayed_code_without_parsing_cli_wording() {
        let output = Arc::new(Mutex::new(Vec::new()));
        run(child("printf 'https://example.test/codex\\n'; sleep 0.05; printf '\\033[31mABCD-1234\\033[0m' >&2"),
            output.clone(), CancellationToken::new(), Duration::from_secs(2)).await.unwrap();
        let text = instructions(&output);
        assert!(text.contains("https://example.test/codex"));
        assert!(text.contains("ABCD-1234"));
        assert!(!text.contains('\x1b'));
    }

    #[tokio::test]
    async fn device_login_output_is_bounded_and_child_is_drained() {
        let output = Arc::new(Mutex::new(Vec::new()));
        run(
            child("head -c 65536 /dev/zero; head -c 65536 /dev/zero >&2"),
            output.clone(),
            CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(output.lock().unwrap().len(), OUTPUT_LIMIT);
        append(&output, b"late-code-9876");
        assert!(instructions(&output).contains("late-code-9876"));
    }

    #[tokio::test]
    async fn device_login_failure_timeout_and_cancel_are_reported() {
        assert!(run(
            child("exit 3"),
            Default::default(),
            CancellationToken::new(),
            Duration::from_secs(2)
        )
        .await
        .is_err());
        let start = std::time::Instant::now();
        assert!(run(
            child("exec sleep 5"),
            Default::default(),
            CancellationToken::new(),
            Duration::from_millis(20)
        )
        .await
        .unwrap_err()
        .contains("timed out"));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(run(
            child("exec sleep 5"),
            Default::default(),
            cancel,
            Duration::from_secs(2)
        )
        .await
        .unwrap_err()
        .contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
