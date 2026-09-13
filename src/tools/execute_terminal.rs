use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn execute_terminal(command: &str, cwd: Option<&str>) -> anyhow::Result<TerminalResult> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().await?;

    Ok(TerminalResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

// ── Detached background commands ─────────────────────────────────────────────
// Long-running commands (compiles, downloads, servers) can be started detached:
// the tool returns immediately with a job id, the agent keeps working, and the
// job's output can be polled later. When the job finishes it announces itself
// to the user's dashboard stream, so polling is optional.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::io::AsyncReadExt;

const MAX_CAPTURE: usize = 200_000; // bytes kept per stream

#[derive(Debug, Clone, Serialize)]
pub struct BackgroundJob {
    pub id: String,
    pub command: String,
    pub status: String, // running | done | failed
    pub exit_code: Option<i32>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub owner_user_id: Option<String>,
}

struct JobState {
    job: std::sync::Mutex<BackgroundJob>,
}

static JOBS: Lazy<DashMap<String, Arc<JobState>>> = Lazy::new(DashMap::new);
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

fn push_tail(existing: &mut String, chunk: &[u8]) {
    existing.push_str(&String::from_utf8_lossy(chunk));
    let len = existing.len();
    if len > MAX_CAPTURE {
        let cut = if existing.is_char_boundary(len - MAX_CAPTURE) {
            len - MAX_CAPTURE
        } else {
            existing[len - MAX_CAPTURE..]
                .char_indices()
                .next()
                .map(|(i, _)| len - MAX_CAPTURE + i)
                .unwrap_or(len)
        };
        *existing = existing[cut..].to_string();
    }
}

fn tail_str(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(2000);
    chars[start..].iter().collect()
}

/// Start a command detached; returns the job id immediately.
/// `owner_user_id` (optional) receives a completion announcement in the
/// dashboard/TUI stream when the job finishes, so polling is optional.
pub async fn start_background(
    command: &str,
    cwd: Option<&str>,
    owner_user_id: Option<&str>,
) -> anyhow::Result<String> {
    let seq = JOB_SEQ.fetch_add(1, Ordering::Relaxed);
    let job_id = format!("bg_{}", seq);
    let started_at = chrono::Utc::now().to_rfc3339();
    let state: Arc<JobState> = Arc::new(JobState {
        job: std::sync::Mutex::new(BackgroundJob {
            id: job_id.clone(),
            command: command.to_string(),
            status: "running".to_string(),
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            started_at,
            finished_at: None,
            owner_user_id: owner_user_id.map(|s| s.to_string()),
        }),
    });
    JOBS.insert(job_id.clone(), state.clone());

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    // stdout reader
    let st_out = state.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut j) = st_out.job.lock() {
                        push_tail(&mut j.stdout_tail, &buf[..n]);
                    }
                }
            }
        }
    });

    // stderr reader
    let st_err = state.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut j) = st_err.job.lock() {
                        push_tail(&mut j.stderr_tail, &buf[..n]);
                    }
                }
            }
        }
    });

    // completion watcher: updates status and announces the result
    let st_done = state.clone();
    let job_id_done = job_id.clone();
    let owner = owner_user_id.map(|s| s.to_string());
    tokio::spawn(async move {
        let status = child.wait().await;
        let (status_str, code) = match status {
            Ok(s) => (
                if s.success() { "done" } else { "failed" }.to_string(),
                s.code(),
            ),
            Err(_) => ("failed".to_string(), Some(-1)),
        };
        let snapshot = {
            let mut j = match st_done.job.lock() {
                Ok(j) => j,
                Err(p) => p.into_inner(),
            };
            j.status = status_str.clone();
            j.exit_code = code;
            j.finished_at = Some(chrono::Utc::now().to_rfc3339());
            j.clone()
        };
        tracing::info!(job_id = %job_id_done, command = %snapshot.command, status = %status_str, "background job finished");
        // Announce completion to the owner's dashboard/TUI stream (if any).
        if let Some(owner) = owner {
            let _ = crate::dashboard::stream::send(
                &owner,
                "background_job",
                &serde_json::json!({
                    "job_id": snapshot.id,
                    "command": snapshot.command,
                    "status": snapshot.status,
                    "exit_code": snapshot.exit_code,
                    "stdout_tail": tail_str(&snapshot.stdout_tail),
                    "stderr_tail": tail_str(&snapshot.stderr_tail),
                })
                .to_string(),
            );
        }
    });

    Ok(job_id)
}

/// Poll a background job's status and recent output.
pub fn job_status(job_id: &str) -> Option<BackgroundJob> {
    JOBS.get(job_id).map(|e| {
        let j = match e.job.lock() {
            Ok(j) => j,
            Err(p) => p.into_inner(),
        };
        j.clone()
    })
}

/// List all known background jobs (oldest first).
pub fn list_jobs() -> Vec<BackgroundJob> {
    let mut v: Vec<BackgroundJob> = JOBS
        .iter()
        .map(|e| {
            let j = match e.job.lock() {
                Ok(j) => j,
                Err(p) => p.into_inner(),
            };
            j.clone()
        })
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_terminal_echo() {
        let result = execute_terminal("echo hello", None).await.unwrap();
        assert!(result.stdout.contains("hello"));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_execute_terminal_exit_code() {
        let result = if cfg!(target_os = "windows") {
            execute_terminal("exit 1", None).await.unwrap()
        } else {
            execute_terminal("false", None).await.unwrap()
        };
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_background_job_completes_and_reports() {
        let id = start_background("echo detached-hello", None, None).await.unwrap();
        // wait for completion
        for _ in 0..50 {
            if let Some(j) = job_status(&id) {
                if j.status != "running" {
                    assert_eq!(j.status, "done");
                    assert!(j.stdout_tail.contains("detached-hello"));
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("background job did not finish in time");
    }
}