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
    #[serde(rename = "fetchedAt")]
    pub fetched_at: Option<i64>,
    pub changes: Vec<Change>,
    pub stashes: Vec<Stash>,
    pub incoming: Vec<Commit>,
    pub outgoing: Vec<Commit>,
    pub history: Vec<Commit>,
    pub identity: Identity,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Branch {
    pub name: String,
    pub current: bool,
    /// A remote branch with no local branch yet (switching to it creates one that tracks it).
    pub remote: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub time: i64,
    pub subject: String,
}

/// Local branches (current first, then newest), then remote branches you have no local copy of.
pub async fn branches(project: &Path) -> Result<Vec<Branch>> {
    let text = git(
        project,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%1f%(HEAD)%1f%(upstream:short)%1f%(upstream:track)%1f%(committerdate:unix)%1f%(subject)%1e",
            "refs/heads",
            "refs/remotes",
        ],
        TIMEOUT,
    )
    .await?;
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for record in text.split('\u{1e}') {
        let record = record.trim_matches(['\n', '\r']);
        let parts: Vec<&str> = record.split('\u{1f}').collect();
        if parts.len() < 6 {
            continue;
        }
        let (refname, head, upstream, track, time, subject) = (parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]);
        let mut branch = Branch {
            current: head == "*",
            upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
            time: time.trim().parse().unwrap_or(0),
            subject: subject.to_string(),
            ..Default::default()
        };
        // "[ahead 2, behind 3]"
        for part in track.trim_matches(['[', ']']).split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                branch.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                branch.behind = n.parse().unwrap_or(0);
            }
        }
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            branch.name = name.to_string();
            local.push(branch);
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            branch.name = name.to_string();
            branch.remote = true;
            remote.push(branch);
        }
    }
    // A remote branch that a local branch already tracks is the same work; list it once.
    let tracked: std::collections::HashSet<String> = local.iter().filter_map(|b| b.upstream.clone()).collect();
    remote.retain(|b| !tracked.contains(&b.name));
    local.sort_by_key(|b| !b.current);
    local.extend(remote);
    Ok(local)
}

/// A branch name git accepts, and not one that could be read as an option.
async fn valid_branch(project: &Path, name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_whitespace) {
        return Err(Error::msg(format!("\"{name}\" is not a valid branch name")));
    }
    git(project, &["check-ref-format", "--branch", name], TIMEOUT)
        .await
        .map(|_| ())
        .map_err(|_| Error::msg(format!("\"{name}\" is not a valid branch name")))
}

/// A git identity you can switch between: who commits, and which stored login and SSH key
/// git uses. No passwords or tokens: the credential helper keeps those, picked by `username`.
#[derive(Debug, Clone, Serialize, serde::Deserialize, Default, PartialEq)]
pub struct Account {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub name: String,
    pub email: String,
    /// Login for HTTPS remotes (`credential.username`), so the credential manager picks
    /// the right stored account.
    #[serde(default)]
    pub username: String,
    /// Private key for SSH remotes (`core.sshCommand`).
    #[serde(default, rename = "sshKey")]
    pub ssh_key: String,
    /// Where it came from when Serika found it for you: "gh" or "gcm" (empty when you added it).
    #[serde(default)]
    pub source: String,
    /// Profile picture URL (GitHub accounts).
    #[serde(default)]
    pub avatar: String,
}

/// Accounts plus the default account per project (keyed by project path).
#[derive(Debug, Clone, Serialize, serde::Deserialize, Default)]
pub struct AccountBook {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub defaults: std::collections::BTreeMap<String, String>,
    /// Logins of found accounts you removed, so they are not added again.
    #[serde(default)]
    pub dismissed: Vec<String>,
}

fn book_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("git-accounts.json")
}

pub fn load_book(data_dir: &Path) -> AccountBook {
    std::fs::read_to_string(book_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_book(data_dir: &Path, book: &AccountBook) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let text = serde_json::to_string_pretty(book).map_err(|err| Error::msg(err.to_string()))?;
    std::fs::write(book_path(data_dir), text)?;
    Ok(())
}

/// Add or replace an account (a new one gets an id). Name and email are required.
pub fn save_account(data_dir: &Path, mut account: Account) -> Result<Account> {
    account.name = account.name.trim().to_string();
    account.email = account.email.trim().to_string();
    account.username = account.username.trim().to_string();
    account.ssh_key = account.ssh_key.trim().trim_matches('"').to_string();
    account.label = account.label.trim().to_string();
    if account.name.is_empty() || !account.email.contains('@') {
        return Err(Error::msg("a git account needs a name and an email address"));
    }
    if account.label.is_empty() {
        account.label = account.email.clone();
    }
    let mut book = load_book(data_dir);
    if account.id.is_empty() {
        account.id = crate::sessions::new_id();
    }
    match book.accounts.iter_mut().find(|a| a.id == account.id) {
        Some(existing) => {
            if account.avatar.is_empty() {
                account.avatar = existing.avatar.clone();
            }
            if account.source.is_empty() {
                account.source = existing.source.clone();
            }
            *existing = account.clone()
        }
        None => book.accounts.push(account.clone()),
    }
    save_book(data_dir, &book)?;
    Ok(account)
}

pub fn remove_account(data_dir: &Path, id: &str) -> Result<()> {
    let mut book = load_book(data_dir);
    for account in book.accounts.iter().filter(|a| a.id == id && !a.source.is_empty()) {
        book.dismissed.push(account.username.to_ascii_lowercase());
    }
    book.accounts.retain(|a| a.id != id);
    book.defaults.retain(|_, account| account != id);
    save_book(data_dir, &book)
}

/// Remember (or forget, with an empty id) the account a project uses by default.
pub fn set_default(data_dir: &Path, project_key: &str, id: &str) -> Result<()> {
    let mut book = load_book(data_dir);
    if id.is_empty() {
        book.defaults.remove(project_key);
    } else {
        book.defaults.insert(project_key.to_string(), id.to_string());
    }
    save_book(data_dir, &book)
}

/// GitHub logins on this computer: from the GitHub CLI (`gh auth status`, every account it
/// holds) and from Git Credential Manager. Each becomes an account that commits under the
/// profile's display name and GitHub's private noreply address, and signs in as that login.
pub async fn detect_accounts() -> Vec<Account> {
    use std::sync::{Mutex, OnceLock};
    // Asking GitHub for profile names costs a request per login; reuse answers for a while.
    static CACHE: OnceLock<Mutex<Option<(std::time::Instant, Vec<Account>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, list)) = cache.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return list.clone();
        }
    }
    let quiet = |program: &Path| {
        let mut cmd = Command::new(program);
        cmd.stdin(std::process::Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        cmd
    };
    let mut logins: Vec<(String, String, &'static str)> = Vec::new(); // (host, login, source)
    let gh = crate::agents::find_cli("gh", None);
    if let Some(gh) = &gh {
        let mut cmd = quiet(gh);
        cmd.args(["auth", "status", "--json", "hosts"]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await {
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            if let Some(hosts) = value.get("hosts").and_then(|h| h.as_object()) {
                for (host, list) in hosts {
                    for entry in list.as_array().into_iter().flatten() {
                        if let Some(login) = entry.get("login").and_then(|l| l.as_str()) {
                            logins.push((host.clone(), login.to_string(), "gh"));
                        }
                    }
                }
            }
        }
    }
    {
        let mut cmd = Command::new("git");
        cmd.args(["credential-manager", "github", "list"]).stdin(std::process::Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await {
            if out.status.success() {
                for login in String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains(' ')) {
                    if !logins.iter().any(|(_, l, _)| l.eq_ignore_ascii_case(login)) {
                        logins.push(("github.com".into(), login.to_string(), "gcm"));
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    for (host, login, source) in logins {
        // Public profile: display name and numeric id (for the noreply address).
        let mut name = login.clone();
        let mut id: Option<u64> = None;
        let mut avatar = String::new();
        if let Some(gh) = &gh {
            let mut cmd = quiet(gh);
            cmd.args(["api", &format!("users/{login}"), "--hostname", &host]);
            if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await {
                let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
                if let Some(n) = value.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()) {
                    name = n.trim().to_string();
                }
                id = value.get("id").and_then(|i| i.as_u64());
                avatar = value.get("avatar_url").and_then(|a| a.as_str()).unwrap_or("").to_string();
            }
        }
        let domain = if host == "github.com" { "users.noreply.github.com".to_string() } else { format!("users.noreply.{host}") };
        let email = match id {
            Some(id) => format!("{id}+{login}@{domain}"),
            None => format!("{login}@{domain}"),
        };
        let label = if host == "github.com" { format!("{login} (GitHub)") } else { format!("{login} ({host})") };
        found.push(Account { id: String::new(), label, name, email, username: login, ssh_key: String::new(), source: source.into(), avatar });
    }
    *cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), found.clone()));
    found
}

/// Add the GitHub logins found on this computer that are not accounts yet (and that you have
/// not removed before). Returns how many accounts were added or updated.
pub async fn import_detected(data_dir: &Path) -> usize {
    let found = detect_accounts().await;
    let mut book = load_book(data_dir);
    let mut added = 0;
    let mut changed = false;
    for mut account in found {
        let login = account.username.to_ascii_lowercase();
        // A known login gets its profile picture if it has none yet.
        if let Some(existing) = book.accounts.iter_mut().find(|a| a.username.eq_ignore_ascii_case(&login)) {
            if existing.avatar.is_empty() && !account.avatar.is_empty() {
                existing.avatar = account.avatar.clone();
                changed = true;
            }
            continue;
        }
        if book.dismissed.contains(&login) {
            continue;
        }
        account.id = crate::sessions::new_id();
        book.accounts.push(account);
        added += 1;
    }
    if added > 0 || changed {
        let _ = save_book(data_dir, &book);
    }
    added + usize::from(changed)
}

/// How a project reaches its remote: the login it signs in with, and the GitHub CLI logins
/// that could be used instead.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RemoteAuth {
    pub remote: String,
    pub url: String,
    pub host: String,
    pub login: String,
    pub logins: Vec<String>,
}

pub async fn remote_auth(project: &Path) -> RemoteAuth {
    let mut out = RemoteAuth::default();
    // Two git calls side by side: the upstream, and every remote URL plus the login.
    let upstream = git(project, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"], TIMEOUT);
    let config = git(project, &["config", "--get-regexp", r"^(remote\..*\.url|credential\.username)$"], TIMEOUT);
    let (upstream, config) = tokio::join!(upstream, config);
    let mut urls: Vec<(String, String)> = Vec::new();
    for line in config.unwrap_or_default().lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key.eq_ignore_ascii_case("credential.username") {
            out.login = value.trim().to_string();
        } else if let Some(name) = key.strip_prefix("remote.").and_then(|k| k.strip_suffix(".url")) {
            urls.push((name.to_string(), value.trim().to_string()));
        }
    }
    let wanted = upstream.ok().and_then(|u| u.trim().split_once('/').map(|(r, _)| r.to_string()));
    let chosen = wanted
        .and_then(|w| urls.iter().find(|(n, _)| *n == w).cloned())
        .or_else(|| urls.iter().find(|(n, _)| n == "origin").cloned())
        .or_else(|| urls.first().cloned());
    if let Some((remote, url)) = chosen {
        out.host = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .map(|h| h.rsplit('@').next().unwrap_or(h).to_string())
            .unwrap_or_default();
        out.remote = remote;
        out.url = url;
    }
    if !out.host.is_empty() {
        out.logins = gh_logins(&out.host).await;
    }
    out
}

/// Ask the GitHub CLI once in the background, so the first git panel or project settings
/// do not wait for it.
pub fn warm_up(data_dir: std::path::PathBuf) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = gh_logins("github.com").await;
            let _ = import_detected(&data_dir).await;
        });
    }
}

/// Sign in to the remote as `login` from now on ("" = whatever git picks by default).
pub async fn set_login(project: &Path, login: &str) -> Result<()> {
    let lock = lock_for(project);
    let _guard = lock.lock().await;
    if login.trim().is_empty() {
        let _ = git(project, &["config", "--local", "--unset-all", "credential.username"], TIMEOUT).await;
    } else {
        git(project, &["config", "--local", "credential.username", login.trim()], TIMEOUT).await?;
    }
    Ok(())
}

/// Who commits in this repository, and which Serika account set it (if one did).
#[derive(Debug, Clone, Serialize, Default)]
pub struct Identity {
    pub name: String,
    pub email: String,
    pub username: String,
    #[serde(rename = "sshCommand")]
    pub ssh_command: String,
    /// Id of the Serika account applied to this repository, if any.
    pub account: String,
}

async fn config_get(project: &Path, key: &str) -> String {
    git(project, &["config", "--get", key], TIMEOUT).await.map(|v| v.trim().to_string()).unwrap_or_default()
}

/// One `git config` call for all five settings (each git start costs tens of milliseconds).
pub async fn identity(project: &Path) -> Identity {
    let text = git(
        project,
        &["config", "--get-regexp", r"^(user\.name|user\.email|credential\.username|core\.sshcommand|serika\.account)$"],
        TIMEOUT,
    )
    .await
    .unwrap_or_default();
    let mut who = Identity::default();
    // Later lines win, as in git: local settings come after global ones.
    for line in text.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let value = value.to_string();
        match key.to_ascii_lowercase().as_str() {
            "user.name" => who.name = value,
            "user.email" => who.email = value,
            "credential.username" => who.username = value,
            "core.sshcommand" => who.ssh_command = value,
            "serika.account" => who.account = value,
            _ => {}
        }
    }
    who
}

/// Make this repository commit and authenticate as `account` (local config only), or go back
/// to your global git settings with None. Settings Serika wrote for an earlier account are
/// removed when the new one has no value for them; settings you made yourself stay.
pub async fn apply_account(project: &Path, account: Option<&Account>) -> Result<()> {
    let lock = lock_for(project);
    let _guard = lock.lock().await;
    let managed = !config_get(project, "serika.account").await.is_empty();
    let unset = |key: &'static str| async move {
        let _ = git(project, &["config", "--local", "--unset-all", key], TIMEOUT).await;
    };
    match account {
        Some(account) => {
            git(project, &["config", "--local", "user.name", &account.name], TIMEOUT).await?;
            git(project, &["config", "--local", "user.email", &account.email], TIMEOUT).await?;
            if !account.username.is_empty() {
                git(project, &["config", "--local", "credential.username", &account.username], TIMEOUT).await?;
            } else if managed {
                unset("credential.username").await;
            }
            if !account.ssh_key.is_empty() {
                let key = account.ssh_key.replace('\\', "/");
                let command = format!("ssh -i \"{key}\" -o IdentitiesOnly=yes");
                git(project, &["config", "--local", "core.sshCommand", &command], TIMEOUT).await?;
            } else if managed {
                unset("core.sshCommand").await;
            }
            git(project, &["config", "--local", "serika.account", &account.id], TIMEOUT).await?;
        }
        None => {
            if managed {
                for key in ["user.name", "user.email", "credential.username", "core.sshCommand"] {
                    unset(key).await;
                }
            }
            unset("serika.account").await;
        }
    }
    Ok(())
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
    git_as(project, args, None, timeout).await
}

/// Like `git`, but signed in to `host` as `(login, token)` for this one command: every other
/// credential helper is switched off and one that hands git this login is used instead. The
/// token travels in the environment, never on the command line.
async fn git_as(project: &Path, args: &[&str], auth: Option<(&str, &str, &str)>, timeout: Duration) -> Result<String> {
    let mut cmd = command(project);
    cmd.args(["-c", "core.quotepath=off", "-c", "color.ui=never"]);
    if let Some((host, login, token)) = auth {
        let scoped = format!("credential.https://{host}.helper=");
        cmd.args(["-c", "credential.helper=", "-c", &scoped])
            .args(["-c", "credential.helper=!f() { echo username=\"$SERIKA_GIT_USER\"; echo password=\"$SERIKA_GIT_TOKEN\"; }; f"])
            .env("SERIKA_GIT_USER", login)
            .env("SERIKA_GIT_TOKEN", token);
    }
    cmd.args(args);
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
const SIDES_FORMAT: &str = "--format=%m%x1f%H%x1f%h%x1f%s%x1f%an%x1f%ct%x1e";

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
    // The repository root and where FETCH_HEAD lives, in one call.
    let paths = match git(project, &["rev-parse", "--show-toplevel", "--git-path", "FETCH_HEAD"], TIMEOUT).await {
        Ok(text) => text,
        Err(_) => return Ok(overview),
    };
    let mut lines = paths.lines();
    overview.repo = true;
    overview.root = lines.next().unwrap_or("").trim().to_string();
    let fetch_head = lines.next().unwrap_or("").trim().to_string();
    // Status (with branch and upstream counts), stashes, history and identity side by side;
    // incoming and outgoing come from one log over both sides of the upstream.
    let status = git(project, &["status", "--porcelain=v2", "--branch", "--untracked-files=all"], TIMEOUT);
    let stashes = git(project, &["stash", "list", "--format=%gd%x1f%gs%x1f%ct%x1e"], TIMEOUT);
    let history = git(project, &["log", "-n", "40", LOG_FORMAT], TIMEOUT);
    let sides = git(project, &["log", "-n", "200", "--left-right", SIDES_FORMAT, "HEAD...@{upstream}"], TIMEOUT);
    let who = identity(project);
    let (status, stashes, history, sides, who) = tokio::join!(status, stashes, history, sides, who);
    overview.identity = who;
    parse_status(&status?, &mut overview);
    overview.stashes = parse_stashes(&stashes.unwrap_or_default());
    overview.history = parse_commits(&history.unwrap_or_default());
    if overview.upstream.is_some() {
        // "<" marks commits only on HEAD (to push), ">" only on the upstream (to pull).
        for record in sides.unwrap_or_default().split('\u{1e}') {
            let record = record.trim_matches(['\n', '\r']);
            let Some((mark, rest)) = record.split_once('\u{1f}') else { continue };
            let Some(commit) = parse_commits(&format!("{rest}\u{1e}")).into_iter().next() else { continue };
            if mark == ">" {
                overview.incoming.push(commit);
            } else {
                overview.outgoing.push(commit);
            }
        }
    }
    if !fetch_head.is_empty() {
        let path = Path::new(&fetch_head);
        let path = if path.is_absolute() { path.to_path_buf() } else { project.join(path) };
        overview.fetched_at = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
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

/// The remote a network command talks to: the upstream's remote, else origin, else the first.
async fn active_remote(project: &Path) -> Option<String> {
    if let Ok(upstream) = git(project, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"], TIMEOUT).await {
        if let Some((remote, _)) = upstream.trim().split_once('/') {
            return Some(remote.to_string());
        }
    }
    let remotes = git(project, &["remote"], TIMEOUT).await.ok()?;
    let list: Vec<&str> = remotes.lines().map(str::trim).filter(|r| !r.is_empty()).collect();
    list.iter().find(|r| **r == "origin").or(list.first()).map(|r| r.to_string())
}

/// Host of an HTTPS remote (SSH remotes sign in with keys, not logins).
async fn https_host(project: &Path, remote: &str) -> Option<String> {
    let url = git(project, &["remote", "get-url", remote], TIMEOUT).await.ok()?;
    let rest = url.trim().strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    let host = host.rsplit('@').next()?; // drop "user@" if present
    Some(host.to_string())
}

/// Answers from the GitHub CLI change rarely; keep them for a few minutes.
fn gh_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Vec<String>)>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Vec<String>)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cached(key: &str) -> Option<Vec<String>> {
    let cache = gh_cache().lock().unwrap_or_else(|e| e.into_inner());
    cache.get(key).filter(|(at, _)| at.elapsed() < Duration::from_secs(300)).map(|(_, v)| v.clone())
}

fn remember(key: &str, value: Vec<String>) {
    gh_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), (std::time::Instant::now(), value));
}

/// GitHub CLI logins for `host`, the active one first.
pub async fn gh_logins(host: &str) -> Vec<String> {
    let key = format!("logins:{host}");
    if let Some(list) = cached(&key) {
        return list;
    }
    let list = gh_logins_now(host).await;
    remember(&key, list.clone());
    list
}

async fn gh_logins_now(host: &str) -> Vec<String> {
    let Some(gh) = crate::agents::find_cli("gh", None) else { return Vec::new() };
    let mut cmd = Command::new(gh);
    cmd.args(["auth", "status", "--json", "hosts"]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await else { return Vec::new() };
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let mut list: Vec<(bool, String)> = value
        .get("hosts")
        .and_then(|h| h.get(host))
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("state").and_then(|s| s.as_str()).unwrap_or("success") == "success")
        .filter_map(|e| Some((e.get("active").and_then(|a| a.as_bool()).unwrap_or(false), e.get("login")?.as_str()?.to_string())))
        .collect();
    list.sort_by_key(|(active, _)| !active);
    list.into_iter().map(|(_, login)| login).collect()
}

async fn gh_token(host: &str, login: &str) -> Option<String> {
    let key = format!("token:{host}:{}", login.to_ascii_lowercase());
    if let Some(token) = cached(&key) {
        return token.into_iter().next();
    }
    let token = gh_token_now(host, login).await;
    if let Some(t) = &token {
        remember(&key, vec![t.clone()]);
    }
    token
}

async fn gh_token_now(host: &str, login: &str) -> Option<String> {
    let gh = crate::agents::find_cli("gh", None)?;
    let mut cmd = Command::new(gh);
    cmd.args(["auth", "token", "--hostname", host, "--user", login]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await.ok()?.ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

fn auth_problem(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    ["repository not found", "authentication failed", "could not read username", "could not read password", "terminal prompts disabled",
     "permission to", "403", "401", "access denied", "not authorized", "invalid username or password"]
        .iter()
        .any(|needle| m.contains(needle))
}

/// A command that talks to the remote, signed in as the project's login (`credential.username`)
/// when the GitHub CLI holds it. If the remote refuses that login, the other GitHub CLI logins
/// for the host are tried; one that works is remembered for this repository. Returns git's
/// output and the login it switched to, if it had to.
async fn network(project: &Path, args: &[&str]) -> Result<(String, Option<String>)> {
    let host = match active_remote(project).await {
        Some(remote) => https_host(project, &remote).await,
        None => None,
    };
    let Some(host) = host else {
        return git(project, args, NETWORK_TIMEOUT).await.map(|out| (out, None));
    };
    let chosen = config_get(project, "credential.username").await;
    let first = if chosen.is_empty() {
        git(project, args, NETWORK_TIMEOUT).await
    } else {
        match gh_token(&host, &chosen).await {
            Some(token) => git_as(project, args, Some((&host, &chosen, &token)), NETWORK_TIMEOUT).await,
            // Not a GitHub CLI login: the credential manager reads credential.username itself.
            None => git(project, args, NETWORK_TIMEOUT).await,
        }
    };
    let error = match first {
        Ok(out) => return Ok((out, None)),
        Err(err) => err,
    };
    if !auth_problem(&error.to_string()) {
        return Err(error);
    }
    for login in gh_logins(&host).await {
        if login.eq_ignore_ascii_case(&chosen) {
            continue;
        }
        let Some(token) = gh_token(&host, &login).await else { continue };
        if let Ok(out) = git_as(project, args, Some((&host, &login, &token)), NETWORK_TIMEOUT).await {
            let _ = git(project, &["config", "--local", "credential.username", &login], TIMEOUT).await;
            return Ok((out, Some(login)));
        }
    }
    Err(error)
}

fn signed_in_note(text: String, switched: Option<String>) -> String {
    match switched {
        Some(login) => format!("{text} Signed in as {login}, which this project will use from now on."),
        None => text,
    }
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
            let (_, switched) = network(project, &["fetch", "--prune"]).await?;
            signed_in_note("Fetched.".to_string(), switched)
        }
        ("pull", _) => {
            let (out, switched) = network(project, &["pull", "--ff-only"]).await?;
            let line = out.lines().find(|l| l.contains("..") || l.contains("Already")).unwrap_or("Pulled.").trim().to_string();
            signed_in_note(line, switched)
        }
        ("push", _) => {
            let (_, switched) = network(project, &["push"]).await?;
            signed_in_note("Pushed.".to_string(), switched)
        }
        // First push of a new branch: to the first remote, and track it from now on.
        ("publish", _) => {
            let remotes = git(project, &["remote"], TIMEOUT).await?;
            let remote = remotes.lines().map(str::trim).find(|r| *r == "origin").or_else(|| remotes.lines().map(str::trim).find(|r| !r.is_empty()));
            let Some(remote) = remote else {
                return Err(Error::msg("this repository has no remote to publish to"));
            };
            let remote = remote.to_string();
            let (_, switched) = network(project, &["push", "-u", &remote, "HEAD"]).await?;
            signed_in_note(format!("Published to {remote}."), switched)
        }
        // `message` carries the branch name for these two.
        ("switch", _) => {
            let name = message.trim();
            let remote_branch = branches(project).await?.into_iter().find(|b| b.remote && b.name == name);
            match remote_branch {
                // A remote branch: make a local one that tracks it.
                Some(branch) => {
                    let local = branch.name.split_once('/').map(|(_, rest)| rest).unwrap_or(&branch.name).to_string();
                    valid_branch(project, &local).await?;
                    git(project, &["switch", "--track", "-c", &local, &branch.name], TIMEOUT).await?;
                    format!("Switched to {local}, tracking {}.", branch.name)
                }
                None => {
                    valid_branch(project, name).await?;
                    git(project, &["switch", name], TIMEOUT).await?;
                    format!("Switched to {name}.")
                }
            }
        }
        ("branch.create", _) => {
            let name = message.trim();
            valid_branch(project, name).await?;
            git(project, &["switch", "-c", name], TIMEOUT).await?;
            format!("Created and switched to {name}.")
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

        // Branches: create one, switch back, and refuse names that look like options.
        act(p, "stash.push", None, "park").await.unwrap();
        act(p, "branch.create", None, "ticket/4821").await.unwrap();
        assert_eq!(overview(p).await.unwrap().branch, "ticket/4821");
        act(p, "switch", None, "main").await.unwrap();
        let list = branches(p).await.unwrap();
        assert!(list[0].current && list[0].name == "main", "{list:?}");
        assert!(list.iter().any(|b| b.name == "ticket/4821" && !b.remote));
        assert!(act(p, "switch", None, "--orphan").await.is_err());
        assert!(act(p, "branch.create", None, "bad name").await.is_err());

        // Accounts: apply one, switch to one without a login, then back to global settings.
        let data = tempfile::tempdir().unwrap();
        let work = save_account(data.path(), Account { name: "Work Me".into(), email: "me@work.test".into(), username: "me-work".into(), ssh_key: "C:\\keys\\work".into(), ..Default::default() }).unwrap();
        let home = save_account(data.path(), Account { name: "Home Me".into(), email: "me@home.test".into(), ..Default::default() }).unwrap();
        assert!(save_account(data.path(), Account { name: "x".into(), email: "nope".into(), ..Default::default() }).is_err());
        apply_account(p, Some(&work)).await.unwrap();
        let id = identity(p).await;
        assert_eq!((id.email.as_str(), id.username.as_str(), id.account.as_str()), ("me@work.test", "me-work", work.id.as_str()));
        assert!(id.ssh_command.contains("C:/keys/work"));
        apply_account(p, Some(&home)).await.unwrap();
        let id = identity(p).await;
        assert_eq!(id.email, "me@home.test");
        assert!(id.username.is_empty() && id.ssh_command.is_empty(), "{id:?}");
        set_default(data.path(), "c:/repo", &home.id).unwrap();
        remove_account(data.path(), &home.id).unwrap();
        assert!(load_book(data.path()).defaults.is_empty());
        apply_account(p, None).await.unwrap();
        assert!(identity(p).await.account.is_empty());
    }
}
