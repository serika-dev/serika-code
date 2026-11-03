use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitFileEntry {
    pub path: String,
    pub index_status: char,
    pub worktree_status: char,
    pub staged: bool,
    pub untracked: bool,
}

impl GitFileEntry {
    pub fn badge(&self) -> &'static str {
        if self.untracked {
            "U"
        } else if self.index_status == 'A' || self.worktree_status == 'A' {
            "A"
        } else if self.index_status == 'D' || self.worktree_status == 'D' {
            "D"
        } else if self.index_status == 'R' || self.worktree_status == 'R' {
            "R"
        } else {
            "M"
        }
    }

    pub fn has_unstaged_changes(&self) -> bool {
        self.untracked || (self.worktree_status != ' ' && self.worktree_status != '?')
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitSummary {
    pub sha: String,
    pub author: String,
    pub relative_time: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: String,
    pub head: String,
    pub branch: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitRepoSnapshot {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<GitFileEntry>,
    pub branches: Vec<String>,
    pub worktrees: Vec<GitWorktree>,
    pub commits: Vec<GitCommitSummary>,
    pub unstaged_diff: String,
    pub staged_diff: String,
}

pub async fn status(root: &Path) -> Result<String> {
    capture(root, &["status", "--short", "--branch"]).await
}

pub async fn diff(root: &Path, path: Option<&str>) -> Result<String> {
    let mut args = vec!["diff", "--"];
    if let Some(path) = path {
        args.push(path);
    }
    capture(root, &args).await
}

pub async fn file_diff(root: &Path, path: &str, staged: bool) -> Result<String> {
    if staged {
        capture(root, &["diff", "--cached", "--", path]).await
    } else {
        capture(root, &["diff", "--", path]).await
    }
}

pub async fn log(root: &Path, limit: u32) -> Result<String> {
    let limit = limit.clamp(1, 50).to_string();
    capture(root, &["log", "-n", &limit, "--oneline", "--decorate"]).await
}

pub async fn branch(root: &Path) -> Result<String> {
    capture(root, &["rev-parse", "--abbrev-ref", "HEAD"]).await
}

pub async fn snapshot(root: &Path) -> Result<GitRepoSnapshot> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || snapshot_sync(&root))
        .await
        .map_err(|err| Error::msg(err.to_string()))?
}

pub fn snapshot_sync(root: &Path) -> Result<GitRepoSnapshot> {
    let probe = run_git_args(root, &["rev-parse", "--is-inside-work-tree"]).unwrap_or_default();
    if probe.trim() != "true" {
        return Ok(GitRepoSnapshot {
            is_repo: false,
            ..Default::default()
        });
    }

    let porcelain = run_git_args(root, &["status", "--porcelain=v1", "--branch"]).unwrap_or_default();
    let (mut branch, ahead, behind, entries) = parse_porcelain_status(&porcelain);
    if branch.is_empty() || branch == "HEAD" {
        let rev = run_git_args(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
        if !rev.is_empty() && !rev.starts_with("git exited") {
            branch = format!("HEAD@{rev}");
        }
    }

    let branches_raw = run_git_args(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )
    .unwrap_or_default();
    let branches = if branches_raw.starts_with("git exited") || branches_raw == "(no output)" {
        Vec::new()
    } else {
        branches_raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    };

    let worktrees_raw = run_git_args(root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let worktrees = parse_worktrees(&worktrees_raw);

    let log_raw = run_git_args(
        root,
        &["log", "-n", "15", "--pretty=format:%h\t%an\t%cr\t%s"],
    )
    .unwrap_or_default();
    let commits = if log_raw.starts_with("git exited") || log_raw == "(no output)" {
        Vec::new()
    } else {
        log_raw
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(4, '\t');
                Some(GitCommitSummary {
                    sha: parts.next()?.to_string(),
                    author: parts.next()?.to_string(),
                    relative_time: parts.next()?.to_string(),
                    subject: parts.next()?.to_string(),
                })
            })
            .collect()
    };

    let unstaged_raw = run_git_args(root, &["diff", "--"]).unwrap_or_default();
    let unstaged_diff = if unstaged_raw == "(no output)" || unstaged_raw.starts_with("git exited") {
        String::new()
    } else {
        unstaged_raw
    };

    let staged_raw = run_git_args(root, &["diff", "--cached", "--"]).unwrap_or_default();
    let staged_diff = if staged_raw == "(no output)" || staged_raw.starts_with("git exited") {
        String::new()
    } else {
        staged_raw
    };

    Ok(GitRepoSnapshot {
        is_repo: true,
        branch,
        ahead,
        behind,
        entries,
        branches,
        worktrees,
        commits,
        unstaged_diff,
        staged_diff,
    })
}

pub fn parse_porcelain_status(text: &str) -> (String, u32, u32, Vec<GitFileEntry>) {
    let mut branch = String::from("main");
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut entries = Vec::new();

    for line in text.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            let (branch_part, track_part) = match header.find('[') {
                Some(idx) => (&header[..idx], Some(&header[idx..])),
                None => (header, None),
            };
            let b = branch_part
                .split("...")
                .next()
                .unwrap_or("main")
                .trim();
            if let Some(stripped) = b.strip_prefix("No commits yet on ") {
                branch = stripped.trim().to_string();
            } else if let Some(stripped) = b.strip_prefix("Initial commit on ") {
                branch = stripped.trim().to_string();
            } else if !b.is_empty() {
                branch = b.to_string();
            }
            if let Some(track) = track_part {
                let inner = track.trim_matches(|c| c == '[' || c == ']' || c == ' ');
                for part in inner.split(',') {
                    let part = part.trim();
                    if let Some(num) = part.strip_prefix("ahead ") {
                        ahead = num.trim().parse().unwrap_or(0);
                    } else if let Some(num) = part.strip_prefix("behind ") {
                        behind = num.trim().parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let mut chars = line.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let _space = chars.next();
        let mut path = chars.as_str().trim().to_string();
        if let Some((_old, new_path)) = path.split_once(" -> ") {
            path = new_path.trim().to_string();
        }
        let path = path.trim_matches('"').to_string();
        let untracked = x == '?' && y == '?';
        let staged = !untracked && x != ' ';
        entries.push(GitFileEntry {
            path,
            index_status: x,
            worktree_status: y,
            staged,
            untracked,
        });
    }

    (branch, ahead, behind, entries)
}

fn parse_worktrees(text: &str) -> Vec<GitWorktree> {
    if text.starts_with("git exited") || text == "(no output)" {
        return Vec::new();
    }
    let mut list = Vec::new();
    let mut current_path = String::new();
    let mut current_head = String::new();
    let mut current_branch = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current_path.is_empty() {
                list.push(GitWorktree {
                    path: std::mem::take(&mut current_path),
                    head: std::mem::take(&mut current_head),
                    branch: std::mem::take(&mut current_branch),
                });
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = path.trim().to_string();
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current_head = head.trim().chars().take(7).collect();
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch
                .trim()
                .strip_prefix("refs/heads/")
                .unwrap_or(branch.trim())
                .to_string();
        } else if line == "detached" && current_branch.is_empty() {
            current_branch = "detached".to_string();
        }
    }
    if !current_path.is_empty() {
        list.push(GitWorktree {
            path: current_path,
            head: current_head,
            branch: current_branch,
        });
    }
    list
}

pub async fn stage_file(root: &Path, path: &str) -> Result<String> {
    capture(root, &["add", "--", path]).await
}

pub async fn unstage_file(root: &Path, path: &str) -> Result<String> {
    capture(root, &["reset", "HEAD", "--", path]).await
}

pub async fn stage_all(root: &Path) -> Result<String> {
    capture(root, &["add", "-A"]).await
}

pub async fn unstage_all(root: &Path) -> Result<String> {
    capture(root, &["reset", "HEAD"]).await
}

pub async fn discard_file(root: &Path, path: &str, untracked: bool) -> Result<String> {
    if untracked {
        capture(root, &["clean", "-f", "--", path]).await
    } else {
        capture(root, &["checkout", "--", path]).await
    }
}

pub async fn commit_staged(root: &Path, message: &str) -> Result<String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err(Error::msg("a commit message is required"));
    }
    capture(root, &["commit", "-m", msg]).await
}

pub async fn commit_all(root: &Path, message: &str) -> Result<String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err(Error::msg("a commit message is required"));
    }
    let root = root.to_path_buf();
    let msg = msg.to_string();
    tokio::task::spawn_blocking(move || {
        let added = run_git_args(&root, &["add", "-A"])?;
        if added.starts_with("git exited") {
            return Ok(added);
        }
        run_git_args(&root, &["commit", "-m", &msg])
    })
    .await
    .map_err(|err| Error::msg(err.to_string()))?
}

pub async fn checkout_branch(root: &Path, branch_name: &str, create: bool) -> Result<String> {
    let name = branch_name.trim();
    if name.is_empty() {
        return Err(Error::msg("a branch name is required"));
    }
    if create {
        capture(root, &["checkout", "-b", name]).await
    } else {
        capture(root, &["checkout", name]).await
    }
}

pub async fn create_worktree(
    root: &Path,
    worktrees_root: &Path,
    branch_name: &str,
) -> Result<PathBuf> {
    let clean_branch: String = branch_name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if clean_branch.is_empty() {
        return Err(Error::msg("worktree branch name cannot be empty"));
    }
    let repo_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let target = worktrees_root.join(format!("{repo_name}-{clean_branch}"));
    let root = root.to_path_buf();
    let target_clone = target.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = target_clone.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target_str = target_clone.to_string_lossy().to_string();
        let out = run_git_args(&root, &["worktree", "add", "-b", &clean_branch, &target_str])?;
        if out.starts_with("git exited") {
            // If branch already exists, try checking it out into the worktree without -b
            let retry = run_git_args(&root, &["worktree", "add", &target_str, &clean_branch])?;
            if retry.starts_with("git exited") {
                return Err(Error::msg(retry));
            }
        }
        Ok(target_clone)
    })
    .await
    .map_err(|err| Error::msg(err.to_string()))?
}

pub async fn push(root: &Path) -> Result<String> {
    capture(root, &["push"]).await
}

pub async fn pull(root: &Path) -> Result<String> {
    capture(root, &["pull", "--ff-only"]).await
}

pub async fn commit(root: &Path, message: &str, paths: &[String]) -> Result<String> {
    if message.trim().is_empty() {
        return Err(Error::msg("a commit message is required"));
    }
    if paths.is_empty() {
        return Err(Error::msg(
            "git_commit requires the paths to stage. It will not add the whole tree.",
        ));
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
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_git_args(&root, &arg_refs)
    })
    .await
    .map_err(|err| Error::msg(err.to_string()))?
}

fn run_git_args(root: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    for arg in args {
        command.arg(arg);
    }
    run_git(command)
}

fn run_git(mut command: Command) -> Result<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    match command.output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !err.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&err);
            }
            if text.trim().is_empty() {
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok("git is not installed".to_string())
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_branch_and_entries() {
        let raw = "## feat/ui...origin/feat/ui [ahead 2, behind 1]\nM  src/lib.rs\n M src/main.rs\n?? README.md\n";
        let (branch, ahead, behind, entries) = parse_porcelain_status(raw);
        assert_eq!(branch, "feat/ui");
        assert_eq!(ahead, 2);
        assert_eq!(behind, 1);
        assert_eq!(entries.len(), 3);
        assert!(entries[0].staged);
        assert!(!entries[0].has_unstaged_changes());
        assert_eq!(entries[0].path, "src/lib.rs");
        assert!(!entries[1].staged);
        assert!(entries[1].has_unstaged_changes());
        assert!(entries[2].untracked);
        assert_eq!(entries[2].badge(), "U");
    }

    #[tokio::test]
    async fn stages_commits_and_snapshots_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let init = run_git_args(root, &["init", "-b", "main"]).unwrap();
        if init == "git is not installed" {
            return;
        }
        run_git_args(root, &["config", "user.name", "Tester"]).unwrap();
        run_git_args(
            root,
            &["config", "user.email", "tester@example.com"],
        )
        .unwrap();
        std::fs::write(root.join("hello.txt"), "one\n").unwrap();
        stage_file(root, "hello.txt").await.unwrap();
        let snap = snapshot(root).await.unwrap();
        assert!(snap.is_repo);
        assert_eq!(snap.entries.len(), 1);
        assert!(snap.entries[0].staged);

        commit_staged(root, "feat: add hello").await.unwrap();
        let snap2 = snapshot(root).await.unwrap();
        assert!(snap2.entries.is_empty());
        assert_eq!(snap2.commits.len(), 1);
        assert_eq!(snap2.commits[0].subject, "feat: add hello");
    }
}
