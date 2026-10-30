use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::events::{emit, AgentEvent, EventSender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    ReadOnly,
    Development,
    Destructive,
    Other,
}

pub fn classify_command(command: &str) -> CommandClass {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return CommandClass::Other;
    }
    let folded = folded(trimmed);
    if is_destructive(&folded) {
        return CommandClass::Destructive;
    }
    if is_read_only(&folded) {
        return CommandClass::ReadOnly;
    }
    if is_development(&folded) {
        return CommandClass::Development;
    }
    CommandClass::Other
}

fn folded(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

fn starts_any(command: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| command == *prefix || command.starts_with(&format!("{prefix} ")))
}

fn is_destructive(command: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "git push",
        "git reset",
        "git clean",
        "git rebase",
        "git filter-branch",
        "git checkout --",
        "git restore",
        "git commit",
        "git branch -d",
        "rm -rf",
        "rm -fr",
        "rmdir /s",
        "del /s",
        "del /f",
        "remove-item",
        "format c:",
        "format d:",
        "diskpart",
        "shutdown",
        "mkfs",
        "dd if=",
        "reg delete",
        "cipher /w",
        "sudo ",
        "runas ",
        "curl |",
        "wget |",
        "iex ",
        "iex(",
        "invoke-expression",
        "set-executionpolicy",
        "chmod -r 777",
        "chmod 777",
    ];
    NEEDLES.iter().any(|needle| command.contains(needle))
}

fn is_read_only(command: &str) -> bool {
    if command == "git remote" || command == "git remote -v" {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "git status",
        "git diff",
        "git log",
        "git show",
        "git branch",
        "git rev-parse",
    ];
    starts_any(command, PREFIXES) && !command.starts_with("git branch -d")
}

fn is_development(command: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "cargo test",
        "cargo build",
        "cargo check",
        "cargo fmt",
        "cargo clippy",
        "cargo metadata",
        "npm test",
        "npm run",
        "npm install",
        "npm ci",
        "bun test",
        "bun run",
        "bun install",
        "pnpm test",
        "pnpm run",
        "pnpm install",
        "yarn test",
        "yarn run",
        "pytest",
        "python -m pytest",
        "python -m unittest",
        "py -m pytest",
        "cmake",
        "ninja",
        "go test",
        "go build",
        "dotnet test",
        "dotnet build",
    ];
    starts_any(command, PREFIXES)
}

pub struct CommandOutput {
    pub exit_code: i32,
    pub output: String,
}

pub async fn run_command(
    command: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &CancellationToken,
    events: &EventSender,
    tool_id: &str,
) -> Result<CommandOutput> {
    if command.trim().is_empty() {
        return Err(Error::msg("refusing to run an empty command"));
    }
    let mut child = shell(command, cwd)?;
    let stdout = child.stdout.take().ok_or_else(|| Error::msg("stdout was not piped"))?;
    let stderr = child.stderr.take().ok_or_else(|| Error::msg("stderr was not piped"))?;
    let mut out_lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut stdout_open = true;
    let mut stderr_open = true;

    while stdout_open || stderr_open {
        tokio::select! {
            _ = cancel.cancelled() => {
                kill_child(&mut child).await;
                return Err(Error::Cancelled);
            }
            _ = tokio::time::sleep_until(deadline) => {
                kill_child(&mut child).await;
                return Err(Error::msg(format!("command timed out after {}s", timeout.as_secs())));
            }
            line = out_lines.next_line(), if stdout_open => {
                match line {
                    Ok(Some(line)) => push_line(&mut collected, tool_id, &line, events),
                    Ok(None) => stdout_open = false,
                    Err(err) => return Err(err.into()),
                }
            }
            line = err_lines.next_line(), if stderr_open => {
                match line {
                    Ok(Some(line)) => push_line(&mut collected, tool_id, &line, events),
                    Ok(None) => stderr_open = false,
                    Err(err) => return Err(err.into()),
                }
            }
        }
    }
    let status = child.wait().await?;
    let code = status.code().unwrap_or(-1);
    if collected.len() > 64_000 {
        collected.truncate(64_000);
        collected.push_str("\n… output truncated");
    }
    Ok(CommandOutput {
        exit_code: code,
        output: collected,
    })
}

fn push_line(collected: &mut String, tool_id: &str, line: &str, events: &EventSender) {
    if collected.len() < 64_000 {
        collected.push_str(line);
        collected.push('\n');
    }
    emit(events, AgentEvent::ToolOutput {
        id: tool_id.to_string(),
        stream: line.to_string(),
    });
}

fn shell(command: &str, cwd: &Path) -> Result<tokio::process::Child> {
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/D", "/S", "/C", command]);
        cmd.creation_flags(0x08000000);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        cmd.process_group(0);
        cmd
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("SERIKA_TOKEN")
        .env_remove("SERIKA_REFRESH_TOKEN");
    cmd.spawn().map_err(Error::from)
}

async fn kill_child(child: &mut tokio::process::Child) {
    let pid = child.id();
    let _ = child.start_kill();
    if let Some(pid) = pid {
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        #[cfg(not(windows))]
        {
            // Negative PID sends SIGTERM to the entire process group created by process_group(0).
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AgentEvent;

    #[test]
    fn classifies_commands() {
        assert_eq!(classify_command("git status --short"), CommandClass::ReadOnly);
        assert_eq!(classify_command("cargo test -q"), CommandClass::Development);
        assert_eq!(classify_command("git push origin main"), CommandClass::Destructive);
        assert_eq!(classify_command("npm test && git reset --hard"), CommandClass::Destructive);
        assert_eq!(classify_command("python script.py"), CommandClass::Other);
    }

    #[tokio::test]
    async fn runs_a_short_command() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let output = run_command(
            "echo serika-ok",
            dir.path(),
            Duration::from_secs(15),
            &CancellationToken::new(),
            &tx,
            "t",
        )
        .await
        .unwrap();
        drop(tx);
        assert_eq!(output.exit_code, 0);
        assert!(output.output.contains("serika-ok"));
        let mut saw = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AgentEvent::ToolOutput { .. }) {
                saw = true;
            }
        }
        assert!(saw);
    }
}
