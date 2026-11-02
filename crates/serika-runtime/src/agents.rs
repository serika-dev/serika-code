//! Subscription coding agents.
//!
//! Besides Serika's own models, a chat can run on Claude Code or Codex using the login the
//! user already has with those tools (a Claude or ChatGPT subscription). Serika starts the
//! provider's CLI in the project folder, streams what it does into the same thread view,
//! and keeps the conversation in Serika's history. Nothing is billed through Serika for
//! these runs; the provider's own limits apply and are shown next to the account.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::agent::{TaskOutcome, TaskRequest};
use crate::config::Config;
use crate::diffutil::unified_diff;
use crate::error::{Error, Result};
use crate::events::{emit, AgentEvent, FileDiff};
use crate::model::ChatModel;
use crate::permissions::PermissionMode;
use crate::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAgent {
    Claude,
    Codex,
}

impl CliAgent {
    pub fn from_model(id: &str) -> Option<Self> {
        if id.starts_with("claude-code/") {
            Some(Self::Claude)
        } else if id.starts_with("codex/") {
            Some(Self::Codex)
        } else {
            None
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    fn binary_name(self) -> &'static str {
        self.key()
    }

    /// The `--model` value, or none when the id selects the provider's own default.
    pub fn model_arg(self, id: &str) -> Option<String> {
        let rest = id.split_once('/').map(|(_, rest)| rest).unwrap_or(id);
        if rest.is_empty() || rest == "default" {
            None
        } else {
            Some(rest.to_string())
        }
    }

    /// Prefix of this provider's model ids.
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// Shown until the CLI has listed its own models, or when it cannot be asked.
    pub fn fallback_models(self) -> Vec<AgentModel> {
        let rows: &[(&str, &str, &str)] = match self {
            Self::Claude => &[
                ("default", "Default", "Your default Claude model"),
                ("opus", "Opus", "Best for everyday, complex tasks"),
                ("sonnet", "Sonnet", "Efficient for routine tasks"),
                ("haiku", "Haiku", "Fastest for quick answers"),
            ],
            Self::Codex => &[("default", "Codex", "Your default Codex model")],
        };
        rows.iter()
            .map(|(value, name, description)| AgentModel {
                id: format!("{}/{value}", self.provider_id()),
                name: (*name).to_string(),
                description: Some((*description).to_string()),
                provider: Some(self.provider_id().to_string()),
                is_default: *value == "default",
                ..AgentModel::default()
            })
            .collect()
    }
}

/// A model a subscription agent offers, as its own CLI lists it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
    /// Effort levels the model accepts, lowest first. Empty when it has no effort setting.
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default, rename = "defaultEffort")]
    pub default_effort: Option<String>,
}

impl AgentModel {
    pub fn to_chat_model(&self) -> ChatModel {
        ChatModel {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            max_tokens: None,
            provider: self.provider.clone(),
            premium: false,
            is_default: self.is_default,
            usage_multiplier: None,
            availability: None,
        }
    }
}

const MODEL_CACHE_SECS: i64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CachedModels {
    fetched_at: i64,
    models: Vec<AgentModel>,
}

fn models_path(data_dir: &Path) -> PathBuf {
    data_dir.join("provider-models.json")
}

fn load_models(data_dir: &Path, provider: &str) -> Option<CachedModels> {
    let text = std::fs::read_to_string(models_path(data_dir)).ok()?;
    let mut all: HashMap<String, CachedModels> = serde_json::from_str(&text).ok()?;
    all.remove(provider)
}

/// The last model list a provider's CLI reported, however old. Empty when it never answered.
pub fn cached_models(data_dir: &Path, agent: CliAgent) -> Vec<AgentModel> {
    load_models(data_dir, agent.key()).map(|c| c.models).unwrap_or_default()
}

fn save_models(data_dir: &Path, provider: &str, models: &[AgentModel]) {
    let path = models_path(data_dir);
    let mut all: HashMap<String, CachedModels> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    all.insert(
        provider.to_string(),
        CachedModels {
            fetched_at: now_secs(),
            models: models.to_vec(),
        },
    );
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(path, text);
    }
}

/// Map the `models` array of Claude Code's `initialize` control response.
pub fn parse_claude_models(models: &Value) -> Vec<AgentModel> {
    let Some(list) = models.as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|item| {
            let value = item.get("value").and_then(|v| v.as_str())?.trim();
            if value.is_empty() {
                return None;
            }
            let name = item
                .get("displayName")
                .and_then(|v| v.as_str())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(value);
            let efforts = if item.get("supportsEffort").and_then(|v| v.as_bool()) == Some(true) {
                item.get("supportedEffortLevels")
                    .and_then(|v| v.as_array())
                    .map(|levels| {
                        levels
                            .iter()
                            .filter_map(|l| l.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            Some(AgentModel {
                id: format!("claude-code/{value}"),
                name: name.to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                provider: Some("claude-code".to_string()),
                is_default: value == "default",
                efforts,
                default_effort: None,
            })
        })
        .collect()
}

/// Map the `data` array of Codex app-server's `model/list` response.
pub fn parse_codex_models(data: &Value) -> Vec<AgentModel> {
    let Some(list) = data.as_array() else { return Vec::new() };
    list.iter()
        .filter(|item| item.get("hidden").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|item| {
            let model = item
                .get("model")
                .or_else(|| item.get("id"))
                .and_then(|v| v.as_str())?
                .trim();
            if model.is_empty() {
                return None;
            }
            let efforts = item
                .get("supportedReasoningEfforts")
                .and_then(|v| v.as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|l| {
                            l.get("reasoningEffort")
                                .and_then(|e| e.as_str())
                                .or_else(|| l.as_str())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(AgentModel {
                id: format!("codex/{model}"),
                name: item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(model)
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .filter(|d| !d.trim().is_empty())
                    .map(str::to_string),
                provider: Some("codex".to_string()),
                is_default: item.get("isDefault").and_then(|v| v.as_bool()) == Some(true),
                efforts,
                default_effort: item
                    .get("defaultReasoningEffort")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Start a CLI that speaks JSON lines, send `requests`, and return the first line `done` accepts.
async fn ask_json_lines(
    mut command: Command,
    requests: &[Value],
    wait: Duration,
    done: impl Fn(&Value) -> bool,
) -> Result<Value> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(std::env::temp_dir());
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| Error::msg("no stdin"))?;
    for request in requests {
        stdin.write_all(format!("{request}\n").as_bytes()).await?;
    }
    stdin.flush().await?;
    let stdout = child.stdout.take().ok_or_else(|| Error::msg("no stdout"))?;
    let mut lines = BufReader::new(stdout).lines();
    let found = tokio::time::timeout(wait, async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if done(&value) {
                    return Some(value);
                }
            }
        }
        None
    })
    .await;
    drop(stdin);
    let _ = child.kill().await;
    match found {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(Error::msg("the CLI closed before it answered")),
        Err(_) => Err(Error::msg("the CLI did not answer in time")),
    }
}

/// Ask Claude Code which models this login can use. The `initialize` handshake sends no prompt,
/// so it costs nothing.
async fn discover_claude_models(binary: &Path, config: &Config) -> Result<Vec<AgentModel>> {
    let mut command = command_for(binary);
    command.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]);
    if let Some(dir) = &config.claude_config_dir {
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    let request = serde_json::json!({
        "type": "control_request",
        "request_id": "serika_models",
        "request": {"subtype": "initialize"}
    });
    let reply = ask_json_lines(command, &[request], Duration::from_secs(25), |v| {
        v.get("type").and_then(|t| t.as_str()) == Some("control_response")
    })
    .await?;
    let body = reply.get("response").cloned().unwrap_or(Value::Null);
    if body.get("subtype").and_then(|s| s.as_str()) == Some("error") {
        let message = body.get("error").and_then(|e| e.as_str()).unwrap_or("initialize failed");
        return Err(Error::msg(message.to_string()));
    }
    let models = parse_claude_models(body.pointer("/response/models").unwrap_or(&Value::Null));
    if models.is_empty() {
        return Err(Error::msg("Claude Code listed no models"));
    }
    Ok(models)
}

/// Ask `codex app-server` for its model list over JSON-RPC, the way T3 Code does.
async fn discover_codex_models(binary: &Path, config: &Config) -> Result<Vec<AgentModel>> {
    let mut command = command_for(binary);
    command.arg("app-server");
    if let Some(home) = &config.codex_home {
        command.env("CODEX_HOME", home);
    }
    let requests = [
        serde_json::json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "serika_code", "title": "Serika Code", "version": crate::client::VERSION}
        }}),
        serde_json::json!({"method": "initialized"}),
        serde_json::json!({"id": 2, "method": "model/list", "params": {}}),
    ];
    let reply = ask_json_lines(command, &requests, Duration::from_secs(25), |v| {
        v.get("id").and_then(|i| i.as_i64()) == Some(2)
    })
    .await?;
    if let Some(error) = reply.get("error") {
        let message = error.get("message").and_then(|m| m.as_str()).unwrap_or("model/list failed");
        return Err(Error::msg(message.to_string()));
    }
    let models = parse_codex_models(reply.pointer("/result/data").unwrap_or(&Value::Null));
    if models.is_empty() {
        return Err(Error::msg("Codex listed no models"));
    }
    Ok(models)
}

/// The provider's models: a fresh cache, else a live query, else the stale cache, else a short
/// built-in list. The flag says whether the list came from the CLI.
async fn provider_models(
    agent: CliAgent,
    binary: Option<&Path>,
    config: &Config,
    data_dir: &Path,
    refresh: bool,
) -> (Vec<AgentModel>, bool) {
    let cached = load_models(data_dir, agent.key());
    if let Some(cache) = &cached {
        if !refresh && now_secs() - cache.fetched_at < MODEL_CACHE_SECS && !cache.models.is_empty() {
            return (cache.models.clone(), true);
        }
    }
    if let Some(binary) = binary {
        let live = match agent {
            CliAgent::Claude => discover_claude_models(binary, config).await,
            CliAgent::Codex => discover_codex_models(binary, config).await,
        };
        match live {
            Ok(models) => {
                save_models(data_dir, agent.key(), &models);
                return (models, true);
            }
            Err(err) => tracing::debug!("{} model list: {err}", agent.label()),
        }
    }
    match cached.filter(|c| !c.models.is_empty()) {
        Some(cache) => (cache.models, true),
        None => (agent.fallback_models(), false),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageWindowInfo {
    pub key: String,
    pub label: String,
    /// Fraction of the window that is used, 0 to 1.
    pub used: f64,
    pub resets_at: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub windows: Vec<UsageWindowInfo>,
    pub updated_at: i64,
    /// The provider stopped a request because a limit was reached.
    pub limited: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub binary: Option<String>,
    pub version: Option<String>,
    pub signed_in: Option<bool>,
    pub account: Option<String>,
    pub plan: Option<String>,
    pub detail: String,
    pub install_hint: String,
    pub login_hint: String,
    pub docs_url: String,
    pub usage_url: String,
    pub usage: Option<ProviderUsage>,
    pub models: Vec<AgentModel>,
    /// The model list came from the CLI, not the built-in fallback.
    pub models_live: bool,
    /// Effort level chosen in Serika, sent as `--effort` / `model_reasoning_effort`.
    pub effort: Option<String>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Find a provider CLI: the configured path, PATH, then folders their installers use.
pub fn find_cli(name: &str, configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
        dirs.push(home.join(".npm-global").join("bin"));
        dirs.push(home.join(".bun").join("bin"));
    }
    if let Some(app) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(app).join("npm"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    for dir in dirs {
        for ext in exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_for(binary: &Path) -> Command {
    let shim = binary
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);
    let mut command = if shim {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(binary);
        c
    } else {
        Command::new(binary)
    };
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}

async fn capture(mut command: Command, wait: Duration) -> Option<(bool, String)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(wait, command.output()).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).to_string();
    }
    Some((output.status.success(), text))
}

fn usage_path(data_dir: &Path) -> PathBuf {
    data_dir.join("provider-usage.json")
}

pub fn load_usage(data_dir: &Path, provider: &str) -> Option<ProviderUsage> {
    let text = std::fs::read_to_string(usage_path(data_dir)).ok()?;
    let mut all: HashMap<String, ProviderUsage> = serde_json::from_str(&text).ok()?;
    all.remove(provider)
}

pub fn save_usage(data_dir: &Path, provider: &str, usage: &ProviderUsage) {
    let path = usage_path(data_dir);
    let mut all: HashMap<String, ProviderUsage> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    all.insert(provider.to_string(), usage.clone());
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(path, text);
    }
}

fn window_label(key: &str) -> String {
    match key {
        "five_hour" => "5-hour session".to_string(),
        "seven_day" => "Weekly".to_string(),
        "seven_day_opus" => "Weekly · Opus".to_string(),
        "seven_day_sonnet" => "Weekly · Sonnet".to_string(),
        other => other.replace('_', " "),
    }
}

/// Parse Claude Code's `rate_limit_event` into usage windows.
pub fn parse_rate_limit(info: &Value, message: Option<String>) -> ProviderUsage {
    let mut windows = Vec::new();
    if let Some(map) = info.get("unifiedWindows").and_then(|w| w.as_object()) {
        for (key, window) in map {
            windows.push(UsageWindowInfo {
                key: key.clone(),
                label: window_label(key),
                used: window
                    .get("utilization")
                    .and_then(|u| u.as_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0),
                resets_at: window.get("resetsAt").and_then(|r| r.as_i64()),
                status: window
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("allowed")
                    .to_string(),
            });
        }
    }
    let order = |key: &str| match key {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    };
    windows.sort_by_key(|w| (order(&w.key), w.key.clone()));
    ProviderUsage {
        windows,
        updated_at: now_secs(),
        limited: info.get("status").and_then(|s| s.as_str()) == Some("rejected"),
        message,
    }
}

pub async fn claude_status(config: &Config, data_dir: &Path, refresh: bool) -> ProviderStatus {
    let agent = CliAgent::Claude;
    let mut status = ProviderStatus {
        id: agent.key().to_string(),
        name: agent.label().to_string(),
        installed: false,
        binary: None,
        version: None,
        signed_in: None,
        account: None,
        plan: None,
        detail: "Claude Code is not installed.".to_string(),
        install_hint: "npm install -g @anthropic-ai/claude-code".to_string(),
        login_hint: "claude auth login".to_string(),
        docs_url: "https://claude.com/product/claude-code".to_string(),
        usage_url: "https://claude.ai/settings/usage".to_string(),
        usage: load_usage(data_dir, "claude"),
        models: agent.fallback_models(),
        models_live: false,
        effort: config.claude_effort.clone(),
    };
    let Some(binary) = find_cli("claude", config.claude_path.as_deref()) else {
        return status;
    };
    status.installed = true;
    status.binary = Some(binary.display().to_string());
    // List models while the version and sign-in checks run.
    let models = {
        let (binary, config, data_dir) = (binary.clone(), config.clone(), data_dir.to_path_buf());
        tokio::spawn(async move { provider_models(agent, Some(&binary), &config, &data_dir, refresh).await })
    };

    let mut version = command_for(&binary);
    version.arg("--version");
    if let Some((_, text)) = capture(version, Duration::from_secs(10)).await {
        status.version = text.lines().next().map(|l| l.trim().to_string());
    }

    let mut auth = command_for(&binary);
    auth.args(["auth", "status"]);
    if let Some(dir) = &config.claude_config_dir {
        auth.env("CLAUDE_CONFIG_DIR", dir);
    }
    match capture(auth, Duration::from_secs(12)).await {
        Some((_, text)) => match serde_json::from_str::<Value>(text.trim()) {
            Ok(value) => {
                let logged_in = value.get("loggedIn").and_then(|v| v.as_bool()).unwrap_or(false);
                status.signed_in = Some(logged_in);
                status.account = value.get("email").and_then(|v| v.as_str()).map(str::to_string);
                status.plan = value
                    .get("subscriptionType")
                    .and_then(|v| v.as_str())
                    .map(|plan| {
                        let mut chars = plan.chars();
                        chars
                            .next()
                            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                            .unwrap_or_default()
                    });
                status.detail = if logged_in {
                    "Signed in with your Claude subscription.".to_string()
                } else {
                    "Not signed in. Run `claude auth login` in a terminal.".to_string()
                };
            }
            Err(_) => status.detail = "Installed. Could not read the sign-in status.".to_string(),
        },
        None => status.detail = "Installed. The sign-in check timed out.".to_string(),
    }
    if let Ok((models, live)) = models.await {
        status.models = models;
        status.models_live = live;
    }
    status
}

pub async fn codex_status(config: &Config, data_dir: &Path, refresh: bool) -> ProviderStatus {
    let agent = CliAgent::Codex;
    let mut status = ProviderStatus {
        id: agent.key().to_string(),
        name: agent.label().to_string(),
        installed: false,
        binary: None,
        version: None,
        signed_in: None,
        account: None,
        plan: None,
        detail: "Codex is not installed.".to_string(),
        install_hint: "npm install -g @openai/codex".to_string(),
        login_hint: "codex login".to_string(),
        docs_url: "https://developers.openai.com/codex/cli".to_string(),
        usage_url: "https://chatgpt.com/codex/settings/usage".to_string(),
        usage: load_usage(data_dir, "codex"),
        models: agent.fallback_models(),
        models_live: false,
        effort: config.codex_effort.clone(),
    };
    let Some(binary) = find_cli("codex", config.codex_path.as_deref()) else {
        return status;
    };
    status.installed = true;
    status.binary = Some(binary.display().to_string());
    // List models while the version and sign-in checks run.
    let models = {
        let (binary, config, data_dir) = (binary.clone(), config.clone(), data_dir.to_path_buf());
        tokio::spawn(async move { provider_models(agent, Some(&binary), &config, &data_dir, refresh).await })
    };

    let mut version = command_for(&binary);
    version.arg("--version");
    if let Some((_, text)) = capture(version, Duration::from_secs(10)).await {
        status.version = text.lines().next().map(|l| l.trim().to_string());
    }
    let mut login = command_for(&binary);
    login.args(["login", "status"]);
    if let Some(home) = &config.codex_home {
        login.env("CODEX_HOME", home);
    }
    match capture(login, Duration::from_secs(12)).await {
        Some((ok, text)) => {
            let line = text.lines().next().unwrap_or("").trim().to_string();
            status.signed_in = Some(ok && !line.to_ascii_lowercase().contains("not logged"));
            status.plan = if line.to_ascii_lowercase().contains("chatgpt") {
                Some("ChatGPT".to_string())
            } else if line.to_ascii_lowercase().contains("api key") {
                Some("API key".to_string())
            } else {
                None
            };
            status.detail = if status.signed_in == Some(true) {
                if line.is_empty() { "Signed in.".to_string() } else { line }
            } else {
                "Not signed in. Run `codex login` in a terminal.".to_string()
            };
        }
        None => status.detail = "Installed. The sign-in check timed out.".to_string(),
    }
    if let Ok((models, live)) = models.await {
        status.models = models;
        status.models_live = live;
    }
    status
}

#[derive(Default)]
struct RunState {
    rounds: u32,
    text: String,
    reasoning: String,
    partial_seen: bool,
    provider_session: Option<String>,
    tools: HashMap<String, (String, Value)>,
    result_text: Option<String>,
    is_error: bool,
    denials: Vec<String>,
    usage_note: Option<ProviderUsage>,
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

fn first_line(text: &str) -> String {
    clip(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim(), 240)
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

impl Runtime {
    /// Settings as saved now; the app changes them while this runtime keeps running.
    pub fn live_config(&self) -> Config {
        Config::load_or_create(&self.paths).unwrap_or_else(|_| self.config.clone())
    }

    /// Sign-in, limits and models of each subscription agent. `refresh` skips the model cache.
    pub async fn provider_statuses(&self, refresh: bool) -> Vec<ProviderStatus> {
        let data = self.paths.data_dir.clone();
        let config = self.live_config();
        let (claude, codex) = tokio::join!(
            claude_status(&config, &data, refresh),
            codex_status(&config, &data, refresh)
        );
        vec![claude, codex]
    }

    /// One tiny Claude request to read the subscription windows. It costs a few tokens.
    /// The saved effort level, when the chosen model is known to accept it.
    fn effort_for(&self, agent: CliAgent, config: &Config, model: &str) -> Option<String> {
        let effort = match agent {
            CliAgent::Claude => config.claude_effort.clone(),
            CliAgent::Codex => config.codex_effort.clone(),
        }?;
        let models = load_models(&self.paths.data_dir, agent.key())?.models;
        let wanted = if agent.model_arg(model).is_none() {
            format!("{}/default", agent.provider_id())
        } else {
            model.to_string()
        };
        models
            .iter()
            .find(|m| m.id == wanted)
            .filter(|m| m.efforts.iter().any(|e| *e == effort))
            .map(|_| effort)
    }

    pub async fn probe_claude_usage(&self) -> Result<ProviderUsage> {
        let config = self.live_config();
        let binary = find_cli("claude", config.claude_path.as_deref())
            .ok_or_else(|| Error::msg("Claude Code is not installed"))?;
        let mut command = command_for(&binary);
        command.args([
            "-p",
            "ok",
            "--model",
            "haiku",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "plan",
        ]);
        if let Some(dir) = &config.claude_config_dir {
            command.env("CLAUDE_CONFIG_DIR", dir);
        }
        command.current_dir(std::env::temp_dir());
        let (_, text) = capture(command, Duration::from_secs(45))
            .await
            .ok_or_else(|| Error::msg("Claude Code did not answer in time"))?;
        let mut usage: Option<ProviderUsage> = None;
        let mut message: Option<String> = None;
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            match value.get("type").and_then(|t| t.as_str()) {
                Some("rate_limit_event") => {
                    if let Some(info) = value.get("rate_limit_info") {
                        usage = Some(parse_rate_limit(info, None));
                    }
                }
                Some("result") => {
                    if value.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                        message = value.get("result").and_then(|r| r.as_str()).map(str::to_string);
                    }
                }
                _ => {}
            }
        }
        let mut usage = usage.ok_or_else(|| Error::msg("Claude Code did not report its limits"))?;
        usage.message = message;
        save_usage(&self.paths.data_dir, "claude", &usage);
        Ok(usage)
    }

    pub(crate) async fn run_cli_agent(
        &self,
        agent: CliAgent,
        request: &TaskRequest,
        project: &Path,
        permission: PermissionMode,
        model: &str,
    ) -> Result<TaskOutcome> {
        let config = self.live_config();
        let binary = find_cli(
            agent.binary_name(),
            match agent {
                CliAgent::Claude => config.claude_path.as_deref(),
                CliAgent::Codex => config.codex_path.as_deref(),
            },
        )
        .ok_or_else(|| {
            Error::msg(format!(
                "{} is not installed. Install it, then sign in once in a terminal.",
                agent.label()
            ))
        })?;

        let session_id = self.open_session(project, request, model, permission)?;
        emit(&request.events, AgentEvent::Started { session_id: session_id.clone() });
        emit(
            &request.events,
            AgentEvent::Status(format!("{} · {permission}", agent.label())),
        );
        let mut attachments = request.attachments.clone();
        crate::attachments::load_text(&mut attachments);
        let turn_id = self.sessions.append_user_message(
            &session_id,
            request.prompt.trim(),
            crate::attachments::to_json(&attachments).as_deref(),
        )?;
        // Text files travel inline; images and other files are handed over by path.
        let by_path: Vec<crate::attachments::Attachment> = attachments
            .iter()
            .filter(|a| a.kind != crate::attachments::AttachmentKind::Text)
            .cloned()
            .collect();
        let prompt_text = format!(
            "{}{}",
            crate::attachments::with_files(request.prompt.trim(), &attachments),
            crate::attachments::path_note(&by_path)
        );
        let attachment_dir = attachments
            .first()
            .and_then(|a| std::path::Path::new(&a.path).parent().map(|p| p.to_path_buf()));
        let resume = self.sessions.provider_session(&session_id, agent.key())?;
        let effort = self.effort_for(agent, &config, model);
        // Without its own session (first run on this provider, or after a rewind, fork, or
        // deleted turn) the provider gets the conversation as a handoff. A resumed session only
        // gets what other models said since it last ran.
        let stored = self.sessions.messages(&session_id).unwrap_or_default();
        let after = match &resume {
            None => 0,
            Some(_) => self.sessions.provider_synced(&session_id, agent.key())?.unwrap_or(turn_id),
        };
        let handoff = crate::handoff::build(&stored, after, turn_id, crate::handoff::BUDGET);
        if handoff.messages > 0 {
            let from = crate::handoff::previous_model(&stored, turn_id).unwrap_or_default();
            if from != model {
                emit(
                    &request.events,
                    AgentEvent::Handoff { from, to: model.to_string(), messages: handoff.messages },
                );
            }
        }
        let prompt_text = crate::handoff::wrap(&handoff, resume.is_none(), &prompt_text);

        let mut command = command_for(&binary);
        match agent {
            CliAgent::Claude => {
                let mode = match permission {
                    PermissionMode::ReadOnly => "plan",
                    PermissionMode::Standard => "acceptEdits",
                    PermissionMode::Restricted => "default",
                    PermissionMode::Full => "bypassPermissions",
                };
                command.args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                    "--permission-mode",
                    mode,
                ]);
                if let Some(model) = agent.model_arg(model) {
                    command.args(["--model", &model]);
                }
                if let Some(effort) = &effort {
                    command.args(["--effort", effort]);
                }
                if let Some(id) = &resume {
                    command.args(["--resume", id]);
                }
                if let Some(dir) = &attachment_dir {
                    command.arg("--add-dir").arg(dir);
                }
                if let Some(dir) = &config.claude_config_dir {
                    command.env("CLAUDE_CONFIG_DIR", dir);
                }
            }
            CliAgent::Codex => {
                let sandbox = match permission {
                    PermissionMode::ReadOnly => "read-only",
                    PermissionMode::Standard | PermissionMode::Restricted => "workspace-write",
                    PermissionMode::Full => "danger-full-access",
                };
                command.args(["exec", "--json", "--skip-git-repo-check", "-s", sandbox]);
                command.arg("-C").arg(project);
                if let Some(model) = agent.model_arg(model) {
                    command.args(["-m", &model]);
                }
                if let Some(effort) = &effort {
                    command.arg("-c").arg(format!("model_reasoning_effort=\"{effort}\""));
                }
                for image in attachments
                    .iter()
                    .filter(|a| a.kind == crate::attachments::AttachmentKind::Image)
                {
                    command.arg("-i").arg(&image.path);
                }
                if let Some(id) = &resume {
                    command.args(["resume", id]);
                }
                command.arg("-");
                if let Some(home) = &config.codex_home {
                    command.env("CODEX_HOME", home);
                }
            }
        }
        command
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|err| Error::msg(format!("could not start {}: {err}", agent.label())))?;
        if let Some(mut stdin) = child.stdin.take() {
            let prompt = prompt_text.clone();
            tokio::spawn(async move {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
        }
        let stderr_text = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let sink = stderr_text.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut text = sink.lock().unwrap_or_else(|e| e.into_inner());
                    if text.len() < 8000 {
                        text.push_str(&line);
                        text.push('\n');
                    }
                }
            });
        }
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::msg("could not read the provider output"))?;
        let mut lines = BufReader::new(stdout).lines();
        let mut state = RunState::default();

        loop {
            let next = tokio::select! {
                _ = request.cancel.cancelled() => {
                    let _ = child.kill().await;
                    self.persist_cli_turn(&session_id, agent, &state)?;
                    return Err(Error::Cancelled);
                }
                line = lines.next_line() => line,
            };
            match next {
                Ok(Some(line)) => {
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    match agent {
                        CliAgent::Claude => self.handle_claude(&value, &mut state, request, &session_id),
                        CliAgent::Codex => self.handle_codex(&value, &mut state, request),
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    return Err(Error::msg(format!("error reading {}: {err}", agent.label())));
                }
            }
        }
        let status = child.wait().await?;
        self.persist_cli_turn(&session_id, agent, &state)?;

        if !state.denials.is_empty() {
            state.denials.sort();
            state.denials.dedup();
            emit(
                &request.events,
                AgentEvent::Status(format!(
                    "{} needed approval for {}. Use Auto-accept edits or Full access to let it run.",
                    agent.label(),
                    state.denials.join(", ")
                )),
            );
        }
        if state.is_error {
            let message = state
                .result_text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| format!("{} stopped with an error.", agent.label()));
            return Err(Error::msg(message));
        }
        if !status.success() && state.text.trim().is_empty() {
            let stderr = stderr_text.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let detail = first_line(&stderr);
            return Err(Error::msg(if detail.is_empty() {
                format!("{} exited with {status}", agent.label())
            } else {
                format!("{}: {detail}", agent.label())
            }));
        }
        let summary = state.text.trim().to_string();
        emit(&request.events, AgentEvent::Completed { summary: summary.clone() });
        Ok(TaskOutcome {
            session_id,
            summary,
            finished: true,
        })
    }

    fn persist_cli_turn(&self, session_id: &str, agent: CliAgent, state: &RunState) -> Result<()> {
        if let Some(id) = &state.provider_session {
            self.sessions.set_provider_session(session_id, agent.key(), id)?;
        }
        let text = state.text.trim();
        if !text.is_empty() || !state.reasoning.trim().is_empty() {
            self.sessions.append_message_with_reasoning(
                session_id,
                "assistant",
                text,
                Some(state.reasoning.trim()),
            )?;
        }
        // Everything up to here is in the provider's own transcript now.
        let last = self.sessions.last_message_id(session_id)?;
        self.sessions.set_provider_synced(session_id, agent.key(), last)?;
        Ok(())
    }

    fn handle_claude(&self, value: &Value, state: &mut RunState, request: &TaskRequest, session_id: &str) {
        let events = &request.events;
        match value.get("type").and_then(|t| t.as_str()) {
            Some("system") => {
                if value.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                    state.provider_session = value
                        .get("session_id")
                        .and_then(|s| s.as_str())
                        .map(str::to_string);
                }
            }
            Some("stream_event") => {
                let Some(event) = value.get("event") else { return };
                match event.get("type").and_then(|t| t.as_str()) {
                    Some("message_start") => {
                        state.rounds += 1;
                        if state.rounds > 1 {
                            emit(events, AgentEvent::Status(format!("turn {}", state.rounds)));
                            if !state.text.trim().is_empty() {
                                state.text.push_str("\n\n");
                            }
                            if !state.reasoning.trim().is_empty() {
                                state.reasoning.push_str("\n\n");
                            }
                        } else {
                            emit(events, AgentEvent::Status("turn 1".to_string()));
                        }
                    }
                    Some("content_block_start") => {
                        let block = event.get("content_block").cloned().unwrap_or(Value::Null);
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
                            state.tools.insert(id.clone(), (name.clone(), Value::Null));
                            emit(events, AgentEvent::ToolRequested {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: "{}".to_string(),
                            });
                            emit(events, AgentEvent::ToolStarted { id, name });
                        }
                    }
                    Some("content_block_delta") => {
                        let delta = event.get("delta").cloned().unwrap_or(Value::Null);
                        match delta.get("type").and_then(|t| t.as_str()) {
                            Some("text_delta") => {
                                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                    state.partial_seen = true;
                                    state.text.push_str(text);
                                    emit(events, AgentEvent::MessageChunk { text: text.to_string() });
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                                    state.partial_seen = true;
                                    state.reasoning.push_str(text);
                                    emit(events, AgentEvent::ReasoningChunk { text: text.to_string() });
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                let Some(content) = value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                else {
                    return;
                };
                for block in content {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") if !state.partial_seen => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !state.text.trim().is_empty() {
                                    state.text.push_str("\n\n");
                                }
                                state.text.push_str(text);
                                emit(events, AgentEvent::MessageChunk { text: text.to_string() });
                            }
                        }
                        Some("thinking") if !state.partial_seen => {
                            if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                                state.reasoning.push_str(text);
                                emit(events, AgentEvent::ReasoningChunk { text: text.to_string() });
                            }
                        }
                        Some("tool_use") => {
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            state.tools.insert(id.clone(), (name.clone(), input.clone()));
                            emit(events, AgentEvent::ToolRequested {
                                id,
                                name,
                                arguments: input.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let Some(content) = value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                else {
                    return;
                };
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                        continue;
                    }
                    let id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let (name, input) = state
                        .tools
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| ("tool".to_string(), Value::Null));
                    let ok = block.get("is_error").and_then(|e| e.as_bool()) != Some(true);
                    let output = content_text(block.get("content").unwrap_or(&Value::Null));
                    emit(events, AgentEvent::ToolOutput {
                        id: id.clone(),
                        stream: clip(&output, 6000),
                    });
                    if ok {
                        if let Some(diff) = claude_edit_diff(&name, &input, request) {
                            let _ = self.sessions.record_change(session_id, &diff.path, &diff.diff);
                            emit(events, AgentEvent::FileChanged(diff));
                        }
                    }
                    let _ = self.sessions.record_tool(
                        &id,
                        session_id,
                        &name,
                        &input.to_string(),
                        &clip(&output, 6000),
                        ok,
                    );
                    emit(events, AgentEvent::ToolFinished {
                        id,
                        name,
                        ok,
                        summary: first_line(&output),
                    });
                }
            }
            Some("rate_limit_event") => {
                if let Some(info) = value.get("rate_limit_info") {
                    let usage = parse_rate_limit(info, None);
                    save_usage(&self.paths.data_dir, "claude", &usage);
                    state.usage_note = Some(usage.clone());
                    emit(events, AgentEvent::ProviderUsage {
                        provider: "claude".to_string(),
                        usage,
                    });
                }
            }
            Some("result") => {
                state.is_error = value.get("is_error").and_then(|e| e.as_bool()) == Some(true);
                state.result_text = value.get("result").and_then(|r| r.as_str()).map(str::to_string);
                if let Some(id) = value.get("session_id").and_then(|s| s.as_str()) {
                    state.provider_session = Some(id.to_string());
                }
                if let Some(denials) = value.get("permission_denials").and_then(|d| d.as_array()) {
                    for denial in denials {
                        if let Some(name) = denial.get("tool_name").and_then(|n| n.as_str()) {
                            state.denials.push(name.to_string());
                        }
                    }
                }
                if state.is_error {
                    if let Some(mut usage) = state.usage_note.clone() {
                        usage.limited = value.get("api_error").and_then(|e| e.as_str()) == Some("usage_limit_reached")
                            || usage.limited;
                        usage.message = state.result_text.clone();
                        save_usage(&self.paths.data_dir, "claude", &usage);
                        emit(events, AgentEvent::ProviderUsage {
                            provider: "claude".to_string(),
                            usage,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_codex(&self, value: &Value, state: &mut RunState, request: &TaskRequest) {
        let events = &request.events;
        match value.get("type").and_then(|t| t.as_str()) {
            Some("thread.started") => {
                state.provider_session = value
                    .get("thread_id")
                    .and_then(|t| t.as_str())
                    .map(str::to_string);
            }
            Some("turn.started") => {
                state.rounds += 1;
                emit(events, AgentEvent::Status(format!("turn {}", state.rounds)));
            }
            Some("item.started") | Some("item.updated") | Some("item.completed") => {
                let completed = value.get("type").and_then(|t| t.as_str()) == Some("item.completed");
                let Some(item) = value.get("item") else { return };
                let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("agent_message") if completed => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !state.text.trim().is_empty() {
                                state.text.push_str("\n\n");
                                emit(events, AgentEvent::MessageChunk { text: "\n\n".to_string() });
                            }
                            state.text.push_str(text);
                            emit(events, AgentEvent::MessageChunk { text: text.to_string() });
                        }
                    }
                    Some("reasoning") if completed => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !state.reasoning.trim().is_empty() {
                                state.reasoning.push_str("\n\n");
                            }
                            state.reasoning.push_str(text);
                            emit(events, AgentEvent::ReasoningChunk { text: format!("{text}\n") });
                        }
                    }
                    Some(kind @ ("command_execution" | "file_change" | "mcp_tool_call" | "web_search")) => {
                        let (name, args, summary) = codex_tool(kind, item);
                        if !completed {
                            if !state.tools.contains_key(&id) {
                                state.tools.insert(id.clone(), (name.clone(), args.clone()));
                                emit(events, AgentEvent::ToolRequested {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: args.to_string(),
                                });
                                emit(events, AgentEvent::ToolStarted { id, name });
                            }
                            return;
                        }
                        let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("completed");
                        let exit_ok = item.get("exit_code").and_then(|c| c.as_i64()).map(|c| c == 0).unwrap_or(true);
                        let ok = status != "failed" && exit_ok;
                        if !state.tools.contains_key(&id) {
                            emit(events, AgentEvent::ToolRequested {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: args.to_string(),
                            });
                        }
                        let output = item
                            .get("aggregated_output")
                            .and_then(|o| o.as_str())
                            .unwrap_or(&summary)
                            .to_string();
                        emit(events, AgentEvent::ToolOutput {
                            id: id.clone(),
                            stream: clip(&output, 6000),
                        });
                        emit(events, AgentEvent::ToolFinished {
                            id,
                            name,
                            ok,
                            summary: first_line(if output.trim().is_empty() { &summary } else { &output }),
                        });
                    }
                    _ => {}
                }
            }
            Some("turn.failed") => {
                state.is_error = true;
                state.result_text = value
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string);
            }
            Some("error") => {
                state.is_error = true;
                state.result_text = value.get("message").and_then(|m| m.as_str()).map(str::to_string);
            }
            _ => {}
        }
    }
}

fn codex_tool(kind: &str, item: &Value) -> (String, Value, String) {
    match kind {
        "command_execution" => {
            let command = item.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
            (
                "run_command".to_string(),
                serde_json::json!({"command": command}),
                command,
            )
        }
        "file_change" => {
            let paths: Vec<String> = item
                .get("changes")
                .and_then(|c| c.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|c| c.get("path").and_then(|p| p.as_str()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let summary = paths.join(", ");
            (
                "apply_patch".to_string(),
                serde_json::json!({"path": paths.first().cloned().unwrap_or_default()}),
                summary,
            )
        }
        "mcp_tool_call" => {
            let server = item.get("server").and_then(|s| s.as_str()).unwrap_or("mcp");
            let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
            (
                format!("{server}.{tool}"),
                item.get("arguments").cloned().unwrap_or(Value::Null),
                format!("{server}.{tool}"),
            )
        }
        _ => {
            let query = item.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string();
            (
                "web_search".to_string(),
                serde_json::json!({"query": query}),
                query,
            )
        }
    }
}

/// A diff for Claude Code's file tools, built from the tool input.
fn claude_edit_diff(name: &str, input: &Value, request: &TaskRequest) -> Option<FileDiff> {
    let path = input.get("file_path").and_then(|p| p.as_str())?;
    let shown = Path::new(path)
        .strip_prefix(&request.project)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.replace('\\', "/"));
    let diff = match name {
        "Edit" => unified_diff(
            &shown,
            input.get("old_string").and_then(|s| s.as_str())?,
            input.get("new_string").and_then(|s| s.as_str())?,
        ),
        "Write" => {
            let content = input.get("content").and_then(|s| s.as_str())?;
            let clipped: String = content.lines().take(300).collect::<Vec<_>>().join("\n");
            unified_diff(&shown, "", &clipped)
        }
        "MultiEdit" => {
            let edits = input.get("edits").and_then(|e| e.as_array())?;
            let mut out = String::new();
            for edit in edits {
                out.push_str(&unified_diff(
                    &shown,
                    edit.get("old_string").and_then(|s| s.as_str()).unwrap_or(""),
                    edit.get("new_string").and_then(|s| s.as_str()).unwrap_or(""),
                ));
            }
            out
        }
        _ => return None,
    };
    if diff.trim().is_empty() {
        return None;
    }
    Some(FileDiff { path: shown, diff })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_pick_the_provider() {
        assert_eq!(CliAgent::from_model("claude-code/sonnet"), Some(CliAgent::Claude));
        assert_eq!(CliAgent::from_model("codex/gpt-5"), Some(CliAgent::Codex));
        assert_eq!(CliAgent::from_model("openrouter/free"), None);
        assert_eq!(CliAgent::Claude.model_arg("claude-code/default"), None);
        assert_eq!(CliAgent::Claude.model_arg("claude-code/opus").as_deref(), Some("opus"));
    }

    #[test]
    fn reads_claude_rate_limit_windows() {
        let info: Value = serde_json::from_str(
            r#"{"status":"rejected","resetsAt":1793628000,"rateLimitType":"seven_day","unifiedWindows":{"seven_day":{"utilization":1,"resetsAt":1793628000,"status":"rejected"},"five_hour":{"utilization":0.25,"resetsAt":1793641200,"status":"allowed"}}}"#,
        )
        .unwrap();
        let usage = parse_rate_limit(&info, None);
        assert!(usage.limited);
        assert_eq!(usage.windows[0].key, "five_hour");
        assert_eq!(usage.windows[0].label, "5-hour session");
        assert!((usage.windows[0].used - 0.25).abs() < f64::EPSILON);
        assert_eq!(usage.windows[1].label, "Weekly");
    }

    #[test]
    fn reads_claude_initialize_models() {
        let models: Value = serde_json::from_str(
            r#"[{"value":"default","displayName":"Default (recommended)","description":"Opus 5.5 · Best for everyday, complex tasks","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},
                {"value":"claude-haiku-4-5-20251001","displayName":"Haiku 4.5","description":"Fastest for quick answers"},
                {"value":"","displayName":"broken"}]"#,
        )
        .unwrap();
        let list = parse_claude_models(&models);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "claude-code/default");
        assert!(list[0].is_default);
        assert_eq!(list[0].efforts.len(), 5);
        assert_eq!(list[1].id, "claude-code/claude-haiku-4-5-20251001");
        assert_eq!(list[1].name, "Haiku 4.5");
        assert!(list[1].efforts.is_empty());
    }

    #[test]
    fn reads_codex_model_list() {
        let data: Value = serde_json::from_str(
            r#"[{"id":"gpt-5.5-codex","model":"gpt-5.5-codex","displayName":"GPT-5.5 Codex","description":"Agentic coding","hidden":false,"isDefault":true,
                 "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low","description":""},{"reasoningEffort":"high","description":""}]},
                {"id":"old","model":"old","displayName":"Old","description":"","hidden":true,"isDefault":false,"defaultReasoningEffort":"low","supportedReasoningEfforts":[]}]"#,
        )
        .unwrap();
        let list = parse_codex_models(&data);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "codex/gpt-5.5-codex");
        assert!(list[0].is_default);
        assert_eq!(list[0].efforts, vec!["low", "high"]);
        assert_eq!(list[0].default_effort.as_deref(), Some("medium"));
        assert_eq!(list[0].description.as_deref(), Some("Agentic coding"));
    }
}
