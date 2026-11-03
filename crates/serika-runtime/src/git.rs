//! Read-mostly git view of a project for the desktop Git panel: branch and upstream,
//! incoming and outgoing commits, working changes, history, and stashes (many teams keep one
//! stash per ticket, so stashes are listed with their message and files and can be shown,
//! applied, popped, or dropped). Everything goes through the `git` command line.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(30);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// Large diffs are cut so the panel stays responsive.
const MAX_DIFF: usize = 400_000;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Change {
    pub path: String,
    /// Two-letter porcelain code, e.g. "M.", ".M", "A.", "??".
    pub code: String,
    pub staged: bool,
    pub untracked: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stash {
    pub index: usize,
    /// `stash@{n}`
    pub name: String,
    pub message: String,
    pub branch: String,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Overview {
    pub repo: bool,
    pub root: String,
    pub branch: String,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Unix seconds of the last fetch (FETCH_HEAD), if any.
    pub fetched_at: Option<i64>,
    pub changes: Vec<Change>,
    pub stashes: Vec<Stash>,
    pub incoming: Vec<Commit>,
    pub outgoing: Vec<Commit>,
    pub history: Vec<Commit>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FileDiff {
    pub path: String,
    pub diff: String,
}

fn command(project: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(project)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Run git and return stdout; a non-zero exit is an error carrying git's own message.
async fn git(project: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let mut cmd = command(project);
    cmd.args(["-c", "core.quotepath=off", "-c", "color.ui=never"]).args(args);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| Error::msg(format!("git {} took too long", args.first().unwrap_or(&""))))?
        .map_err(|err| Error::msg(format!("could not run git: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        return Err(Error::msg(if message.is_empty() {
            format!("git {} failed", args.first().unwrap_or(&""))
        } else {
            message
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// One operation per repository at a time, so git's index.lock never collides.
fn lock_for(project: &Path) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let key = project.display().to_string().to_ascii_lowercase();
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_default()
        .clone()
}

const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%s%x1f%an%x1f%ct%x1e";

fn parse_commits(text: &str) -> Vec<Commit> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim_matches(['\n', '\r']);
            let mut parts = record.split('\u{1f}');
            let hash = parts.next()?.trim().to_string();
            if hash.is_empty() {
                return None;
            }
            Some(Commit {
                hash,
                short: parts.next().unwrap_or("").to_string(),
                subject: parts.next().unwrap_or("").to_string(),
                author: parts.next().unwrap_or("").to_string(),
                time: parts.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

/// `git status --porcelain=v2 --branch` into branch info and changes.
fn parse_status(text: &str, overview: &mut Overview) {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            overview.detached = rest == "(detached)";
            overview.branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            overview.upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    overview.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    overview.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("? ") {
            overview.changes.push(Change { path: rest.to_string(), code: "??".into(), staged: false, untracked: true });
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            // "1 XY sub mH mI mW hH hI path" / "2 XY ... score path\torig" / "u XY ... path"
            let fields: usize = match line.as_bytes()[0] {
                b'1' => 8,
                b'2' => 9,
                _ => 10,
            };
            let mut parts = line.splitn(fields + 1, ' ');
            let _kind = parts.next();
            let code = parts.next().unwrap_or("..").to_string();
            let path = parts.nth(fields - 2).unwrap_or("").split('\t').next().unwrap_or("").to_string();
            let staged = code.chars().next().map(|c| c != '.').unwrap_or(false);
            overview.changes.push(Change { path, code, staged, untracked: false });
        }
    }
}

/// "On main: PROJ-123 fix login" / "WIP on main: abc123 subject" -> (branch, message)
fn split_stash_subject(subject: &str) -> (String, String) {
    for prefix in ["On ", "WIP on "] {
        if let Some(rest) = subject.strip_prefix(prefix) {
            if let Some((branch, message)) = rest.split_once(": ") {
                return (branch.to_string(), message.to_string());
            }
        }
    }
    (String::new(), subject.to_string())
}

fn parse_stashes(text: &str) -> Vec<Stash> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim_matches(['\n', '\r']);
            let mut parts = record.split('\u{1f}');
            let name = parts.next()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let subject = parts.next().unwrap_or("");
            let time = parts.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
            let index = name
                .trim_start_matches("stash@{")
                .trim_end_matches('}')
                .parse()
                .unwrap_or(0);
            let (branch, message) = split_stash_subject(subject);
            Some(Stash { index, name, message, branch, time })
        })
        .collect()
}

/// Everything the panel shows, in one call. A folder that is not a repository says so.
pub async fn overview(project: &Path) -> Result<Overview> {
    let mut overview = Overview::default();
    let root = match git(project, &["rev-parse", "--show-toplevel"], TIMEOUT).await {
        Ok(root) => root.trim().to_string(),
        Err(_) => return Ok(overview),
    };
    overview.repo = true;
    overview.root = root;
    let status = git(project, &["status", "--porcelain=v2", "--branch", "--untracked-files=all"], TIMEOUT);
    let stashes = git(project, &["stash", "list", "--format=%gd%x1f%gs%x1f%ct%x1e"], TIMEOUT);
    let history = git(project, &["log", "-n", "40", LOG_FORMAT], TIMEOUT);
    let fetch_head = git(project, &["rev-parse", "--git-path", "FETCH_HEAD"], TIMEOUT);
    let (status, stashes, history, fetch_head) = tokio::join!(status, stashes, history, fetch_head);
    parse_status(&status?, &mut overview);
    overview.stashes = parse_stashes(&stashes.unwrap_or_default());
    overview.history = parse_commits(&history.unwrap_or_default());
    if let Ok(path) = fetch_head {
        let path = Path::new(path.trim());
        let path = if path.is_absolute() { path.to_path_buf() } else { project.join(path) };
        overview.fetched_at = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
    }
    if overview.upstream.is_some() {
        let incoming = git(project, &["log", "-n", "100", LOG_FORMAT, "HEAD..@{upstream}"], TIMEOUT);
        let outgoing = git(project, &["log", "-n", "100", LOG_FORMAT, "@{upstream}..HEAD"], TIMEOUT);
        let (incoming, outgoing) = tokio::join!(incoming, outgoing);
        overview.incoming = parse_commits(&incoming.unwrap_or_default());
        overview.outgoing = parse_commits(&outgoing.unwrap_or_default());
    }
    Ok(overview)
}

/// Split a multi-file unified diff into one entry per file.
pub fn split_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // "a/path b/path" (paths may contain spaces; take the b/ side)
            let rest = rest.trim_end();
            let path = rest
                .rsplit_once(" b/")
                .map(|(_, b)| b.to_string())
                .unwrap_or_else(|| rest.trim_start_matches("a/").to_string());
            files.push(FileDiff { path, diff: String::new() });
        }
        if let Some(file) = files.last_mut() {
            file.diff.push_str(line);
        }
    }
    files
}

fn cap(mut text: String) -> String {
    if text.len() > MAX_DIFF {
        let mut end = MAX_DIFF;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n… (diff cut here)\n");
    }
    text
}

fn stash_ref(index: usize) -> String {
    format!("stash@{{{index}}}")
}

/// The files a stash changes, including the untracked files it saved.
pub async fn stash_diff(project: &Path, index: usize) -> Result<Vec<FileDiff>> {
    let name = stash_ref(index);
    let tracked = git(project, &["stash", "show", "-p", "--no-ext-diff", &name], TIMEOUT).await?;
    let mut files = split_diff(&cap(tracked));
    // Untracked files live in the stash's third parent, when there is one.
    let untracked_ref = format!("{name}^3");
    if git(project, &["rev-parse", "--verify", "--quiet", &untracked_ref], TIMEOUT).await.is_ok() {
        if let Ok(extra) = git(
            project,
            &["show", "--no-ext-diff", "--format=", &untracked_ref],
            TIMEOUT,
        )
        .await
        {
            files.extend(split_diff(&cap(extra)));
        }
    }
    Ok(files)
}

pub async fn commit_diff(project: &Path, hash: &str) -> Result<Vec<FileDiff>> {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::msg("not a commit id"));
    }
    let text = git(project, &["show", "--no-ext-diff", "--format=", hash], TIMEOUT).await?;
    Ok(split_diff(&cap(text)))
}

/// The working tree diff for one file (staged and unstaged together, against HEAD).
pub async fn file_diff(project: &Path, path: &str, untracked: bool) -> Result<String> {
    if untracked {
        let full = project.join(path);
        let text = std::fs::read_to_string(&full).unwrap_or_default();
        let mut diff = format!("diff --git a/{path} b/{path}\nnew file\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n", text.lines().count());
        for line in text.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        return Ok(cap(diff));
    }
    let text = git(project, &["diff", "--no-ext-diff", "HEAD", "--", path], TIMEOUT).await?;
    Ok(cap(text))
}

/// Stash actions and network operations. Returns git's own message for the panel.
pub async fn act(project: &Path, action: &str, index: Option<usize>, message: &str) -> Result<String> {
    let lock = lock_for(project);
    let _guard = lock.lock().await;
    let name = index.map(stash_ref);
    // Git's own output is kept for errors; success gets a short line (stash pop, for one,
    // prints a whole `git status`).
    let done = match (action, name.as_deref()) {
        ("stash.apply", Some(name)) => {
            git(project, &["stash", "apply", name], TIMEOUT).await?;
            format!("Applied {name}. The stash is still there.")
        }
        ("stash.pop", Some(name)) => {
            git(project, &["stash", "pop", name], TIMEOUT).await?;
            format!("Popped {name} into your working tree.")
        }
        ("stash.drop", Some(name)) => {
            git(project, &["stash", "drop", name], TIMEOUT).await?;
            format!("Dropped {name}.")
        }
        ("stash.push", _) => {
            let message = message.trim();
            if message.is_empty() {
                git(project, &["stash", "push", "--include-untracked"], TIMEOUT).await?;
            } else {
                git(project, &["stash", "push", "--include-untracked", "-m", message], TIMEOUT).await?;
            }
            "Stashed your changes.".to_string()
        }
        ("fetch", _) => {
            git(project, &["fetch", "--prune"], NETWORK_TIMEOUT).await?;
            "Fetched.".to_string()
        }
        ("pull", _) => {
            let out = git(project, &["pull", "--ff-only"], NETWORK_TIMEOUT).await?;
            out.lines().find(|l| l.contains("..") || l.contains("Already")).unwrap_or("Pulled.").trim().to_string()
        }
        ("push", _) => {
            git(project, &["push"], NETWORK_TIMEOUT).await?;
            "Pushed.".to_string()
        }
        _ => return Err(Error::msg(format!("unknown git action {action}"))),
    };
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_stashes() {
        let mut o = Overview::default();
        parse_status(
            "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -5\n\
             1 .M N... 100644 100644 100644 aaa bbb src/lib.rs\n\
             1 A. N... 000000 100644 100644 000 ccc new file.rs\n\
             2 R. N... 100644 100644 100644 aaa bbb R100 to.rs\tfrom.rs\n\
             ? notes.txt\n",
            &mut o,
        );
        assert_eq!(o.branch, "main");
        assert_eq!(o.upstream.as_deref(), Some("origin/main"));
        assert_eq!((o.ahead, o.behind), (2, 5));
        let paths: Vec<&str> = o.changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["src/lib.rs", "new file.rs", "to.rs", "notes.txt"]);
        assert!(!o.changes[0].staged && o.changes[1].staged && o.changes[3].untracked);

        let stashes = parse_stashes("stash@{0}\u{1f}On main: 4821 login timeout\u{1f}1700000000\u{1e}\nstash@{1}\u{1f}WIP on dev: abc fix\u{1f}1690000000\u{1e}");
        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].message, "4821 login timeout");
        assert_eq!(stashes[0].branch, "main");
        assert_eq!(stashes[1].index, 1);
    }

    #[test]
    fn splits_multi_file_diffs() {
        let files = split_diff("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/dir/b c.rs b/dir/b c.rs\n+z\n");
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].path, "dir/b c.rs");
        assert!(files[0].diff.ends_with("+y\n"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reads_a_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        if git(p, &["init", "-q", "-b", "main"], TIMEOUT).await.is_err() {
            return; // no git here
        }
        let _ = git(p, &["config", "user.email", "t@example.com"], TIMEOUT).await;
        let _ = git(p, &["config", "user.name", "T"], TIMEOUT).await;
        std::fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "."], TIMEOUT).await.unwrap();
        git(p, &["commit", "-q", "-m", "first"], TIMEOUT).await.unwrap();
        std::fs::write(p.join("a.txt"), "two\n").unwrap();
        std::fs::write(p.join("new.txt"), "fresh\n").unwrap();
        act(p, "stash.push", None, "4821 ticket work").await.unwrap();
        let o = overview(p).await.unwrap();
        assert!(o.repo);
        assert_eq!(o.branch, "main");
        assert_eq!(o.stashes.len(), 1);
        assert_eq!(o.stashes[0].message, "4821 ticket work");
        assert!(o.changes.is_empty());
        let files = stash_diff(p, 0).await.unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.txt") && paths.contains(&"new.txt"), "{paths:?}");
        act(p, "stash.pop", Some(0), "").await.unwrap();
        let o = overview(p).await.unwrap();
        assert!(o.stashes.is_empty());
        assert_eq!(o.changes.len(), 2);
        assert_eq!(o.history[0].subject, "first");
    }
}
