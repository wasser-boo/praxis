use serde::{Deserialize, Serialize};

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
}
