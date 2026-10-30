use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

pub async fn status(root: &Path) -> Result<String> {
    capture(root, &["status", "--short", "--branch"]).await
}

pub async fn diff(root: &Path, path: Option<&str>) -> Result<String> {
    let mut args = vec!["diff", "--",];
    if let Some(path) = path {
        args.push(path);
    }
    capture(root, &args).await
}

pub async fn log(root: &Path, limit: u32) -> Result<String> {
    let limit = limit.clamp(1, 50).to_string();
    capture(root, &["log", "-n", &limit, "--oneline", "--decorate"]).await
}

pub async fn branch(root: &Path) -> Result<String> {
    capture(root, &["rev-parse", "--abbrev-ref", "HEAD"]).await
}

pub async fn commit(root: &Path, message: &str, paths: &[String]) -> Result<String> {
    if message.trim().is_empty() {
        return Err(Error::msg("a commit message is required"));
    }
    if paths.is_empty() {
        return Err(Error::msg("git_commit requires the paths to stage. It will not add the whole tree."));
    }
    let root = root.to_path_buf();
    let message = message.to_string();
    let paths = paths.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut add = Command::new("git");
        add.arg("-C").arg(&root).arg("add").arg("--");
        for path in &paths {
            add.arg(path);
        }
        let added = run_git(add)?;
        if added.starts_with("git exited") {
            return Ok(added);
        }
        let mut commit = Command::new("git");
        commit.arg("-C").arg(&root).arg("commit").arg("-m").arg(&message);
        run_git(commit)
    })
    .await
    .map_err(|err| Error::msg(err.to_string()))?
}

async fn capture(root: &Path, args: &[&str]) -> Result<String> {
    let root = root.to_path_buf();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.arg("-C").arg(&root);
        for arg in &args {
            command.arg(arg);
        }
        run_git(command)
    })
    .await
    .map_err(|err| Error::msg(err.to_string()))?
}

fn run_git(mut command: Command) -> Result<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    match command.output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !err.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&err);
            }
            if text.is_empty() {
                text = if output.status.success() {
                    "(no output)".to_string()
                } else {
                    format!("git exited {}", output.status.code().unwrap_or(-1))
                };
            } else if !output.status.success() {
                text = format!("git exited {}\n{text}", output.status.code().unwrap_or(-1));
            }
            Ok(text)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok("git is not installed".to_string()),
        Err(err) => Err(err.into()),
    }
}
