use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::client::ChatTurn;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::model::ChatModel;

pub const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com";
pub const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com";
pub const DEFAULT_GEMINI_BASE: &str = "https://generativelanguage.googleapis.com";
pub const DEFAULT_OLLAMA_BASE: &str = "http://127.0.0.1:11434";
pub const DEFAULT_LOCAL_SERVER_BASE: &str = "http://127.0.0.1:1234";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Local,
    OpenAi,
    Anthropic,
    Gemini,
    Serika,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Local => "Local Workspace (No API)",
            ProviderKind::OpenAi => "OpenAI API",
            ProviderKind::Anthropic => "Claude API",
            ProviderKind::Gemini => "Gemini API",
            ProviderKind::Serika => "Serika Cloud",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            ProviderKind::Local => "LOCAL",
            ProviderKind::OpenAi => "OPENAI",
            ProviderKind::Anthropic => "CLAUDE",
            ProviderKind::Gemini => "GEMINI",
            ProviderKind::Serika => "SERIKA",
        }
    }

    pub fn from_model_id(model_id: &str) -> Self {
        if model_id.starts_with("local/") || model_id.starts_with("cli/") {
            ProviderKind::Local
        } else if model_id.starts_with("openai/") {
            ProviderKind::OpenAi
        } else if model_id.starts_with("claude/") || model_id.starts_with("anthropic/") {
            ProviderKind::Anthropic
        } else if model_id.starts_with("gemini/") {
            ProviderKind::Gemini
        } else {
            ProviderKind::Serika
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalHarness {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub available: bool,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderDiscoveryReport {
    pub models: Vec<ChatModel>,
    pub local_count: usize,
    pub openai_count: usize,
    pub anthropic_count: usize,
    pub gemini_count: usize,
    pub errors: Vec<String>,
}

/// Detect local CLI coding agents and workspace modes installed on the machine.
pub fn detect_local_harnesses() -> Vec<LocalHarness> {
    let mut out = vec![
        LocalHarness {
            id: "local/workspace".to_string(),
            name: "Local Workspace · Auto".to_string(),
            binary: "built-in".to_string(),
            available: true,
            description: "Built-in offline workspace agent: auto-routes tools, git, tests & files"
                .to_string(),
        },
        LocalHarness {
            id: "local/workspace:architect".to_string(),
            name: "Local Workspace · Architect".to_string(),
            binary: "built-in".to_string(),
            available: true,
            description: "Built-in offline architecture mapper & structural code analyzer"
                .to_string(),
        },
        LocalHarness {
            id: "local/workspace:test-runner".to_string(),
            name: "Local Workspace · Test & Verify".to_string(),
            binary: "built-in".to_string(),
            available: true,
            description: "Built-in offline test runner & build diagnostics engine".to_string(),
        },
        LocalHarness {
            id: "local/workspace:git-reviewer".to_string(),
            name: "Local Workspace · Git & Diff Reviewer".to_string(),
            binary: "built-in".to_string(),
            available: true,
            description: "Built-in offline working-tree & turn-diff inspector".to_string(),
        },
    ];

    let candidates = [
        (
            "cli/claude",
            "Claude Code CLI",
            "claude",
            "Local `claude` CLI harness in your workspace (supports `--model`)",
        ),
        (
            "cli/codex",
            "Codex CLI",
            "codex",
            "Local `codex` CLI harness in your workspace (supports `--model`)",
        ),
        (
            "cli/gemini",
            "Gemini CLI",
            "gemini",
            "Local `gemini` CLI harness in your workspace (supports `--model`)",
        ),
        (
            "cli/opencode",
            "OpenCode CLI",
            "opencode",
            "Local `opencode` CLI harness in your workspace (supports `--model`)",
        ),
        (
            "cli/ollama",
            "Ollama CLI",
            "ollama",
            "Local `ollama` model runner",
        ),
    ];

    for (id, name, bin, desc) in candidates {
        let available = binary_on_path(bin);
        out.push(LocalHarness {
            id: id.to_string(),
            name: name.to_string(),
            binary: bin.to_string(),
            available,
            description: desc.to_string(),
        });
    }
    out
}

pub fn binary_on_path(bin: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return true;
            }
        }
    }
    false
}

/// Returns initial local workspace & installed CLI models before async API discovery finishes.
pub fn builtin_provider_models(harnesses: &[LocalHarness]) -> Vec<ChatModel> {
    harnesses
        .iter()
        .filter(|h| h.available)
        .map(|h| ChatModel {
            id: h.id.clone(),
            name: h.name.clone(),
            description: Some(h.description.clone()),
            max_tokens: None,
            provider: Some("local".to_string()),
            premium: false,
            is_default: h.id == "local/workspace",
            usage_multiplier: None,
            availability: Some("ready".to_string()),
        })
        .collect()
}

/// Dynamically discover all models from:
/// 1. Local Workspace + Ollama (`GET /api/tags` / `ollama list`) + Local OpenAI Server (`GET /v1/models`) + Local CLIs
/// 2. OpenAI API (`GET /v1/models`) when an OpenAI key is configured
/// 3. Anthropic Claude API (`GET /v1/models`) when a Claude key is configured
/// 4. Google Gemini API (`GET /v1beta/models`) when a Gemini key is configured
pub async fn discover_all_models(
    http: &reqwest::Client,
    config: &Config,
) -> ProviderDiscoveryReport {
    let harnesses = detect_local_harnesses();
    let mut models = builtin_provider_models(&harnesses);
    let mut errors = Vec::new();

    let (ollama_res, local_srv_res, opencode_res, openai_res, anthropic_res, gemini_res) = tokio::join!(
        fetch_ollama_models(http, config),
        fetch_local_openai_models(http, config),
        fetch_opencode_cli_models(),
        async {
            if config.resolve_openai_key().is_some() {
                Some(fetch_openai_models(http, config).await)
            } else {
                None
            }
        },
        async {
            if config.resolve_anthropic_key().is_some() {
                Some(fetch_anthropic_models(http, config).await)
            } else {
                None
            }
        },
        async {
            if config.resolve_gemini_key().is_some() {
                Some(fetch_gemini_models(http, config).await)
            } else {
                None
            }
        }
    );

    if let Ok(ollama_models) = ollama_res {
        for m in ollama_models {
            if !models.iter().any(|existing| existing.id == m.id) {
                models.push(m);
            }
        }
    }
    if let Ok(srv_models) = local_srv_res {
        for m in srv_models {
            if !models.iter().any(|existing| existing.id == m.id) {
                models.push(m);
            }
        }
    }
    if let Ok(oc_models) = opencode_res {
        for m in oc_models {
            if !models.iter().any(|existing| existing.id == m.id) {
                models.push(m);
            }
        }
    }

    let local_count = models
        .iter()
        .filter(|m| m.provider.as_deref() == Some("local"))
        .count();

    let mut openai_count = 0;
    if let Some(res) = openai_res {
        match res {
            Ok(list) => {
                openai_count = list.len();
                models.extend(list);
            }
            Err(err) => errors.push(format!("OpenAI /v1/models: {err}")),
        }
    }

    let mut anthropic_count = 0;
    if let Some(res) = anthropic_res {
        match res {
            Ok(list) => {
                anthropic_count = list.len();
                models.extend(list);
            }
            Err(err) => errors.push(format!("Claude /v1/models: {err}")),
        }
    }

    let mut gemini_count = 0;
    if let Some(res) = gemini_res {
        match res {
            Ok(list) => {
                gemini_count = list.len();
                models.extend(list);
            }
            Err(err) => errors.push(format!("Gemini /v1beta/models: {err}")),
        }
    }

    ProviderDiscoveryReport {
        models,
        local_count,
        openai_count,
        anthropic_count,
        gemini_count,
        errors,
    }
}

/// Fetch live models from Ollama (`GET {ollama_base}/api/tags`) and fallback to `ollama list` CLI.
pub async fn fetch_ollama_models(http: &reqwest::Client, config: &Config) -> Result<Vec<ChatModel>> {
    let base = config
        .ollama_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OLLAMA_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/api/tags");

    let mut out = Vec::new();
    if let Ok(resp) = http.get(&url).timeout(Duration::from_millis(1500)).send().await {
        if resp.status().is_success() {
            if let Ok(val) = resp.json::<Value>().await {
                if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                    for item in arr {
                        let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
                            continue;
                        };
                        let param_size = item
                            .get("details")
                            .and_then(|d| d.get("parameter_size"))
                            .and_then(|p| p.as_str())
                            .unwrap_or("");
                        let quant = item
                            .get("details")
                            .and_then(|d| d.get("quantization_level"))
                            .and_then(|q| q.as_str())
                            .unwrap_or("");
                        let display = if !param_size.is_empty() {
                            format!("Ollama · {name} ({param_size})")
                        } else {
                            format!("Ollama · {name}")
                        };
                        let desc = if !quant.is_empty() {
                            format!("Local Ollama model ({param_size} {quant})")
                        } else {
                            "Local Ollama model via /api/tags".to_string()
                        };
                        out.push(ChatModel {
                            id: format!("local/ollama:{name}"),
                            name: display,
                            description: Some(desc),
                            max_tokens: None,
                            provider: Some("local".to_string()),
                            premium: false,
                            is_default: false,
                            usage_multiplier: None,
                            availability: Some("ready".to_string()),
                        });
                    }
                }
            }
        }
    }

    if out.is_empty() && binary_on_path("ollama") {
        let mut cmd = Command::new("ollama");
        cmd.arg("list")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }
        if let Ok(output) = cmd.output().await {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for (idx, line) in text.lines().enumerate() {
                    if idx == 0 && line.to_ascii_uppercase().contains("NAME") {
                        continue;
                    }
                    let Some(name) = line.split_whitespace().next() else {
                        continue;
                    };
                    if !name.is_empty() {
                        out.push(ChatModel {
                            id: format!("local/ollama:{name}"),
                            name: format!("Ollama · {name}"),
                            description: Some("Local model discovered via `ollama list`".into()),
                            max_tokens: None,
                            provider: Some("local".to_string()),
                            premium: false,
                            is_default: false,
                            usage_multiplier: None,
                            availability: Some("ready".to_string()),
                        });
                    }
                }
            }
        }
    }

    Ok(out)
}

/// Fetch live models from a local OpenAI-compatible server (`GET {local_base}/v1/models`, e.g. LM Studio, llama.cpp, vLLM, Jan).
pub async fn fetch_local_openai_models(
    http: &reqwest::Client,
    config: &Config,
) -> Result<Vec<ChatModel>> {
    let base = config
        .local_base_url
        .as_deref()
        .unwrap_or(DEFAULT_LOCAL_SERVER_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1/models");

    let resp = http
        .get(&url)
        .timeout(Duration::from_millis(1500))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let val: Value = resp.json().await?;
    let mut out = Vec::new();
    if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            let Some(id) = item.get("id").and_then(|s| s.as_str()) else {
                continue;
            };
            out.push(ChatModel {
                id: format!("local/server:{id}"),
                name: format!("Local Server · {id}"),
                description: Some(format!("Local OpenAI-compatible endpoint ({base})")),
                max_tokens: None,
                provider: Some("local".to_string()),
                premium: false,
                is_default: false,
                usage_multiplier: None,
                availability: Some("ready".to_string()),
            });
        }
    }
    Ok(out)
}

/// If `opencode` CLI is installed, query `opencode models` to list available models dynamically.
pub async fn fetch_opencode_cli_models() -> Result<Vec<ChatModel>> {
    if !binary_on_path("opencode") {
        return Ok(Vec::new());
    }
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "opencode", "models"]);
        c
    } else {
        let mut c = Command::new("opencode");
        c.arg("models");
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    let Ok(output) = tokio::time::timeout(Duration::from_secs(3), cmd.output()).await else {
        return Ok(Vec::new());
    };
    let Ok(output) = output else {
        return Ok(Vec::new());
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in text.lines().take(40) {
        let m = line.trim();
        if m.is_empty() || m.starts_with('#') || m.contains(' ') {
            continue;
        }
        out.push(ChatModel {
            id: format!("cli/opencode:{m}"),
            name: format!("OpenCode · {m}"),
            description: Some("Discovered from `opencode models`".into()),
            max_tokens: None,
            provider: Some("local".to_string()),
            premium: false,
            is_default: false,
            usage_multiplier: None,
            availability: Some("ready".to_string()),
        });
    }
    Ok(out)
}

/// Fetch live models directly from OpenAI's `GET /v1/models` endpoint.
pub async fn fetch_openai_models(http: &reqwest::Client, config: &Config) -> Result<Vec<ChatModel>> {
    let key = config
        .resolve_openai_key()
        .ok_or_else(|| Error::msg("OpenAI API key is not configured"))?;
    let base = config
        .openai_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OPENAI_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1/models");

    let resp = http
        .get(&url)
        .bearer_auth(&key)
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::msg(format!(
            "HTTP {status}: {}",
            crate::client::redact(&text)
        )));
    }
    let val: Value = resp.json().await?;
    let mut rows: Vec<(i64, String, String)> = Vec::new();
    if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            let Some(id) = item.get("id").and_then(|s| s.as_str()) else {
                continue;
            };
            let lower = id.to_ascii_lowercase();
            // Skip non-chat/completion models (embeddings, audio, image, moderation)
            if lower.contains("embedding")
                || lower.contains("whisper")
                || lower.contains("tts")
                || lower.contains("dall-e")
                || lower.contains("moderation")
                || lower.contains("realtime")
                || lower.contains("transcribe")
                || lower.starts_with("babbage")
                || lower.starts_with("davinci")
            {
                continue;
            }
            let created = item.get("created").and_then(|c| c.as_i64()).unwrap_or(0);
            let owner = item
                .get("owned_by")
                .and_then(|o| o.as_str())
                .unwrap_or("openai")
                .to_string();
            rows.push((created, id.to_string(), owner));
        }
    }
    // Sort newest first
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    Ok(rows
        .into_iter()
        .map(|(_, id, owner)| ChatModel {
            id: format!("openai/{id}"),
            name: id.clone(),
            description: Some(format!("Live from OpenAI /v1/models ({owner})")),
            max_tokens: None,
            provider: Some("openai".to_string()),
            premium: false,
            is_default: false,
            usage_multiplier: None,
            availability: Some("live".to_string()),
        })
        .collect())
}

/// Fetch live models directly from Anthropic's `GET /v1/models` endpoint.
pub async fn fetch_anthropic_models(
    http: &reqwest::Client,
    config: &Config,
) -> Result<Vec<ChatModel>> {
    let key = config
        .resolve_anthropic_key()
        .ok_or_else(|| Error::msg("Claude API key is not configured"))?;
    let base = config
        .anthropic_base_url
        .as_deref()
        .unwrap_or(DEFAULT_ANTHROPIC_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1/models");

    let resp = http
        .get(&url)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::msg(format!(
            "HTTP {status}: {}",
            crate::client::redact(&text)
        )));
    }
    let val: Value = resp.json().await?;
    let mut out = Vec::new();
    if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            let Some(id) = item.get("id").and_then(|s| s.as_str()) else {
                continue;
            };
            let display_name = item
                .get("display_name")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(id);
            let created_at = item
                .get("created_at")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            out.push(ChatModel {
                id: format!("claude/{id}"),
                name: display_name.to_string(),
                description: Some(if created_at.is_empty() {
                    "Live from Anthropic /v1/models".to_string()
                } else {
                    format!("Live from Anthropic /v1/models ({created_at})")
                }),
                max_tokens: None,
                provider: Some("claude".to_string()),
                premium: false,
                is_default: false,
                usage_multiplier: None,
                availability: Some("live".to_string()),
            });
        }
    }
    Ok(out)
}

/// Fetch live models directly from Google Gemini's `GET /v1beta/models` endpoint.
pub async fn fetch_gemini_models(http: &reqwest::Client, config: &Config) -> Result<Vec<ChatModel>> {
    let key = config
        .resolve_gemini_key()
        .ok_or_else(|| Error::msg("Gemini API key is not configured"))?;
    let base = config
        .gemini_base_url
        .as_deref()
        .unwrap_or(DEFAULT_GEMINI_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1beta/models");

    let resp = http
        .get(&url)
        .header("x-goog-api-key", &key)
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::msg(format!(
            "HTTP {status}: {}",
            crate::client::redact(&text)
        )));
    }
    let val: Value = resp.json().await?;
    let mut out = Vec::new();
    if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
        for item in arr {
            let Some(raw_name) = item.get("name").and_then(|s| s.as_str()) else {
                continue;
            };
            // If supportedGenerationMethods is present, ensure generateContent is supported
            if let Some(methods) = item
                .get("supportedGenerationMethods")
                .and_then(|m| m.as_array())
            {
                let supports_gen = methods
                    .iter()
                    .any(|m| m.as_str() == Some("generateContent"));
                if !supports_gen {
                    continue;
                }
            }
            let clean_id = raw_name.strip_prefix("models/").unwrap_or(raw_name);
            let display_name = item
                .get("displayName")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(clean_id);
            let desc = item
                .get("description")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| Some("Live from Gemini /v1beta/models".to_string()));
            let max_tokens = item.get("inputTokenLimit").and_then(|v| v.as_u64());
            out.push(ChatModel {
                id: format!("gemini/{clean_id}"),
                name: display_name.to_string(),
                description: desc,
                max_tokens,
                provider: Some("gemini".to_string()),
                premium: false,
                is_default: false,
                usage_multiplier: None,
                availability: Some("live".to_string()),
            });
        }
    }
    Ok(out)
}

/// Complete a turn using the Local Workspace Agent, a local Ollama model, a local OpenAI-compatible server, or a local CLI harness.
pub async fn complete_local(
    http: &reqwest::Client,
    config: &Config,
    model_id: &str,
    project: &Path,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    // 1. Local Ollama model (`local/ollama:<model>`)
    if let Some(ollama_model) = model_id.strip_prefix("local/ollama:") {
        return complete_ollama_http_or_cli(
            http,
            config,
            ollama_model,
            project,
            message,
            history,
            cancel,
            on_content,
            on_reasoning,
        )
        .await;
    }

    // 2. Local OpenAI-compatible Server model (`local/server:<model>`)
    if let Some(srv_model) = model_id.strip_prefix("local/server:") {
        return complete_local_openai_server(
            http,
            config,
            srv_model,
            message,
            history,
            cancel,
            on_content,
            on_reasoning,
        )
        .await;
    }

    // 3. Built-in Local Workspace Agent (`local/workspace` or `local/workspace:<profile>`)
    if model_id == "local"
        || model_id == "local/workspace"
        || model_id.starts_with("local/workspace:")
    {
        // If user configured a specific `local_model` override for the workspace:
        if let Some(sub) = config
            .local_model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "auto" && !s.starts_with("local/workspace"))
        {
            if let Some(om) = sub.strip_prefix("ollama:") {
                return complete_ollama_http_or_cli(
                    http,
                    config,
                    om,
                    project,
                    message,
                    history,
                    cancel,
                    on_content,
                    on_reasoning,
                )
                .await;
            }
            if let Some(sm) = sub.strip_prefix("server:") {
                return complete_local_openai_server(
                    http,
                    config,
                    sm,
                    message,
                    history,
                    cancel,
                    on_content,
                    on_reasoning,
                )
                .await;
            }
        }
        let profile = model_id
            .strip_prefix("local/workspace:")
            .unwrap_or("auto");
        on_reasoning(&format!(
            "Routing request through Local Workspace `{profile}` engine in {}…\n",
            project.display()
        ));
        let reply = synthesize_local_workspace_turn(project, profile, message, history);
        on_content(&reply);
        return Ok(reply);
    }

    // 4. Local CLI Harnesses (`cli/claude`, `cli/codex`, `cli/gemini`, `cli/opencode`, `cli/ollama`)
    // Supports optional `:model` suffix OR `config.local_model`!
    let raw_cli = model_id.strip_prefix("cli/").unwrap_or(model_id);
    let (harness_bin, explicit_submodel) = match raw_cli.split_once(':') {
        Some((bin, sub)) => (bin, Some(sub.trim())),
        None => (
            raw_cli,
            config
                .local_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "auto"),
        ),
    };

    if !binary_on_path(harness_bin) {
        let reply = synthesize_local_workspace_turn(project, "auto", message, history);
        on_content(&reply);
        return Ok(reply);
    }

    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(harness_bin);
        append_cli_harness_args(&mut c, harness_bin, explicit_submodel, message);
        c
    } else {
        let mut c = Command::new(harness_bin);
        append_cli_harness_args(&mut c, harness_bin, explicit_submodel, message);
        c
    };

    cmd.current_dir(project)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|err| Error::msg(format!("failed to launch local `{harness_bin}` CLI: {err}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::msg("failed to capture local CLI stdout"))?;
    let mut reader = BufReader::new(stdout).lines();
    let mut full = String::new();

    loop {
        let next_line = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
            line = reader.next_line() => line,
        };
        match next_line {
            Ok(Some(line)) => {
                let chunk = format!("{line}\n");
                full.push_str(&chunk);
                on_content(&chunk);
            }
            Ok(None) => break,
            Err(err) => {
                return Err(Error::msg(format!(
                    "error reading `{harness_bin}` output: {err}"
                )))
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() && full.trim().is_empty() {
        return Err(Error::msg(format!(
            "local CLI `{harness_bin}` exited with status {status}"
        )));
    }
    Ok(full.trim().to_string())
}

fn append_cli_harness_args(
    cmd: &mut Command,
    harness_bin: &str,
    submodel: Option<&str>,
    message: &str,
) {
    match harness_bin {
        "claude" => {
            if let Some(m) = submodel {
                cmd.args(["--model", m]);
            }
            cmd.args(["-p", message]);
        }
        "codex" => {
            if let Some(m) = submodel {
                cmd.args(["--model", m]);
            }
            cmd.args(["exec", message]);
        }
        "gemini" => {
            if let Some(m) = submodel {
                cmd.args(["--model", m]);
            }
            cmd.args(["-p", message]);
        }
        "opencode" => {
            cmd.arg("run");
            if let Some(m) = submodel {
                cmd.args(["--model", m]);
            }
            cmd.arg(message);
        }
        "ollama" => {
            let m = submodel.unwrap_or("qwen2.5-coder");
            cmd.args(["run", m, message]);
        }
        _ => {
            if let Some(m) = submodel {
                cmd.args(["--model", m]);
            }
            cmd.arg(message);
        }
    }
}

async fn complete_ollama_http_or_cli(
    http: &reqwest::Client,
    config: &Config,
    model_name: &str,
    project: &Path,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let base = config
        .ollama_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OLLAMA_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/api/chat");

    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| json!({ "role": t.role, "content": t.content }))
        .collect();
    messages.push(json!({ "role": "user", "content": message }));

    let body = json!({
        "model": model_name,
        "messages": messages,
        "stream": true,
    });

    if let Ok(resp) = http
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(600))
        .send()
        .await
    {
        if resp.status().is_success() {
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut full = String::new();
            let mut splitter = crate::protocol::ThinkStreamSplitter::new();
            loop {
                let next = tokio::select! {
                    _ = cancel.cancelled() => return Err(Error::Cancelled),
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = next else { break };
                let bytes = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim().to_string();
                    buffer.replace_range(..idx + 1, "");
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if let Some(msg_obj) = v.get("message") {
                            for r_key in ["thinking", "reasoning", "reasoning_content"] {
                                if let Some(r_txt) = msg_obj.get(r_key).and_then(|s| s.as_str()) {
                                    if !r_txt.is_empty() {
                                        on_reasoning(r_txt);
                                        break;
                                    }
                                }
                            }
                            if let Some(c) = msg_obj.get("content").and_then(|s| s.as_str()) {
                                if !c.is_empty() {
                                    full.push_str(c);
                                    splitter.feed(c, &mut on_content, &mut on_reasoning);
                                }
                            }
                        }
                        if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
                            splitter.flush(&mut on_content, &mut on_reasoning);
                            return Ok(full);
                        }
                    }
                }
            }
            splitter.flush(&mut on_content, &mut on_reasoning);
            if !full.is_empty() {
                return Ok(full);
            }
        }
    }

    // Fallback to `ollama run <model>` CLI if HTTP server wasn't reachable
    if binary_on_path("ollama") {
        let mut cmd = Command::new("ollama");
        cmd.args(["run", model_name, message])
            .current_dir(project)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }
        let output = cmd.output().await?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            let mut splitter = crate::protocol::ThinkStreamSplitter::new();
            splitter.feed(&text, &mut on_content, &mut on_reasoning);
            splitter.flush(&mut on_content, &mut on_reasoning);
            return Ok(text);
        }
    }

    Err(Error::msg(format!(
        "Could not reach local Ollama model `{model_name}` at {base}"
    )))
}

async fn complete_local_openai_server(
    http: &reqwest::Client,
    config: &Config,
    model_name: &str,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    on_content: impl FnMut(&str),
    on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let base = config
        .local_base_url
        .as_deref()
        .unwrap_or(DEFAULT_LOCAL_SERVER_BASE)
        .trim_end_matches('/');
    stream_openai_compatible(
        http,
        base,
        None,
        model_name,
        message,
        history,
        cancel,
        on_content,
        on_reasoning,
    )
    .await
}

fn synthesize_local_workspace_turn(
    project: &Path,
    profile: &str,
    message: &str,
    history: &[ChatTurn],
) -> String {
    if message.starts_with("Tool results:\n") {
        let body = message.trim_start_matches("Tool results:\n").trim();
        return format!(
            "### Local Workspace (`{profile}`) Execution Complete\n\nI executed the workspace operation locally (no external API used):\n\n```text\n{}\n```",
            clip_str(body, 4000)
        );
    }

    let lower = message.to_ascii_lowercase();
    let already_ran_tools = history
        .iter()
        .any(|t| t.content.starts_with("Tool results:\n"));

    if !already_ran_tools {
        if profile == "git-reviewer"
            || lower.contains("git")
            || lower.contains("diff")
            || lower.contains("uncommitted")
            || lower.contains("status")
        {
            return "Inspecting repository status and diffs locally:\n```serika-tool\n{\"name\":\"git_status\",\"arguments\":{}}\n```\n```serika-tool\n{\"name\":\"git_diff\",\"arguments\":{\"staged\":false}}\n```"
                .to_string();
        }
        if profile == "test-runner"
            || lower.contains("test")
            || lower.contains("cargo test")
            || lower.contains("verify")
        {
            let cmd = if project.join("Cargo.toml").exists() {
                "cargo test"
            } else if project.join("package.json").exists() {
                "npm test"
            } else {
                "git status -sb"
            };
            return format!(
                "Running workspace verification locally:\n```serika-tool\n{{\"name\":\"run_command\",\"arguments\":{{\"command\":\"{cmd}\"}}}}\n```"
            );
        }
        if profile == "architect"
            || lower.contains("architecture")
            || lower.contains("overview")
            || lower.contains("files")
            || lower.contains("list")
            || lower.contains("structure")
        {
            return "Scanning workspace files locally:\n```serika-tool\n{\"name\":\"list_files\",\"arguments\":{}}\n```"
                .to_string();
        }
        if let Some(rest) = lower.strip_prefix("run ") {
            let cmd = rest.trim();
            if !cmd.is_empty() && !cmd.contains('"') {
                return format!(
                    "Executing command locally:\n```serika-tool\n{{\"name\":\"run_command\",\"arguments\":{{\"command\":{}}}}}\n```",
                    serde_json::to_string(cmd).unwrap_or_else(|_| "\"git status\"".to_string())
                );
            }
        }
    }

    let project_name = project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    format!(
        "**Local Workspace Agent · `{profile}`** (`{project_name}` — API-free mode)\n\n\
         - **Workspace**: `{}`\n\
         - **Local Model Selection**: Choose between Local Workspace profiles (`Auto`, `Architect`, `Test & Verify`, `Git & Diff Reviewer`), discovered **Ollama** models (`/api/tags`), **Local OpenAI Server** models (`/v1/models`), or local CLIs (`claude`, `codex`, `gemini`, `opencode`) with a custom `--model`.\n\
         - **Live Cloud APIs**: Open **Providers & Keys** to add an optional **OpenAI**, **Gemini**, or **Claude** API key and fetch their live `/models` list directly from their API endpoints.",
        project.display()
    )
}

/// Complete a turn against the OpenAI Chat Completions API (`POST /v1/chat/completions`).
pub async fn complete_openai(
    http: &reqwest::Client,
    config: &Config,
    model_id: &str,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    on_content: impl FnMut(&str),
    on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let key = config.resolve_openai_key().ok_or_else(|| {
        Error::msg(
            "OpenAI API key is not configured. Add it in Providers & Keys (or set OPENAI_API_KEY), or switch to a Local Workspace model.",
        )
    })?;
    let model = model_id.strip_prefix("openai/").unwrap_or(model_id);
    let base = config
        .openai_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OPENAI_BASE)
        .trim_end_matches('/');
    stream_openai_compatible(
        http,
        base,
        Some(&key),
        model,
        message,
        history,
        cancel,
        on_content,
        on_reasoning,
    )
    .await
}

async fn stream_openai_compatible(
    http: &reqwest::Client,
    base: &str,
    bearer_key: Option<&str>,
    model: &str,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let url = format!("{base}/v1/chat/completions");
    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| json!({ "role": t.role, "content": t.content }))
        .collect();
    messages.push(json!({ "role": "user", "content": message }));

    let body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let mut req = http
        .post(&url)
        .header("accept", "text/event-stream")
        .json(&body)
        .timeout(Duration::from_secs(600));
    if let Some(k) = bearer_key {
        req = req.bearer_auth(k);
    }

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        r = req.send() => r?,
    };
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Api {
            status,
            message: format!("API error ({status}): {}", crate::client::redact(&text)),
            code: None,
        });
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut splitter = crate::protocol::ThinkStreamSplitter::new();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = next else { break };
        let bytes = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(idx) = buffer.find("\n\n") {
            let raw = buffer[..idx].to_string();
            buffer.replace_range(..idx + 2, "");
            for line in raw.lines() {
                let line = line.trim_end_matches('\r');
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    splitter.flush(&mut on_content, &mut on_reasoning);
                    return Ok(full);
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                    {
                        for r_key in ["reasoning_content", "reasoning", "thinking", "thought"] {
                            if let Some(r) = delta.get(r_key).and_then(|s| s.as_str()) {
                                if !r.is_empty() {
                                    on_reasoning(r);
                                    break;
                                }
                            }
                        }
                        if let Some(c) = delta.get("content").and_then(|s| s.as_str()) {
                            if !c.is_empty() {
                                full.push_str(c);
                                splitter.feed(c, &mut on_content, &mut on_reasoning);
                            }
                        }
                    }
                }
            }
        }
    }
    splitter.flush(&mut on_content, &mut on_reasoning);
    Ok(full)
}

/// Complete a turn against the Anthropic Claude Messages API (`POST /v1/messages`).
pub async fn complete_anthropic(
    http: &reqwest::Client,
    config: &Config,
    model_id: &str,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let key = config.resolve_anthropic_key().ok_or_else(|| {
        Error::msg(
            "Claude API key is not configured. Add it in Providers & Keys (or set ANTHROPIC_API_KEY), or switch to a Local Workspace model.",
        )
    })?;
    let model = model_id
        .strip_prefix("claude/")
        .or_else(|| model_id.strip_prefix("anthropic/"))
        .unwrap_or(model_id);
    let base = config
        .anthropic_base_url
        .as_deref()
        .unwrap_or(DEFAULT_ANTHROPIC_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1/messages");

    let mut system_prompt = String::new();
    let mut messages: Vec<Value> = Vec::new();
    for turn in history {
        if turn.role == "system" {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&turn.content);
        } else {
            let role = if turn.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            messages.push(json!({ "role": role, "content": turn.content }));
        }
    }
    messages.push(json!({ "role": "user", "content": message }));

    let mut body = json!({
        "model": model,
        "max_tokens": 8192,
        "messages": messages,
        "stream": true,
    });
    if !system_prompt.is_empty() {
        body["system"] = json!(system_prompt);
    }

    let req = http
        .post(&url)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("accept", "text/event-stream")
        .json(&body)
        .timeout(Duration::from_secs(600));

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        r = req.send() => r?,
    };
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Api {
            status,
            message: format!("Claude API error ({status}): {}", crate::client::redact(&text)),
            code: None,
        });
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut splitter = crate::protocol::ThinkStreamSplitter::new();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = next else { break };
        let bytes = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(idx) = buffer.find("\n\n") {
            let raw = buffer[..idx].to_string();
            buffer.replace_range(..idx + 2, "");
            for line in raw.lines() {
                let line = line.trim_end_matches('\r');
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if v.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                        if let Some(delta) = v.get("delta") {
                            if let Some(thinking) = delta.get("thinking").and_then(|s| s.as_str()) {
                                if !thinking.is_empty() {
                                    on_reasoning(thinking);
                                }
                            }
                            if let Some(text) = delta.get("text").and_then(|s| s.as_str()) {
                                if !text.is_empty() {
                                    full.push_str(text);
                                    splitter.feed(text, &mut on_content, &mut on_reasoning);
                                }
                            }
                        }
                    } else if v.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
                        splitter.flush(&mut on_content, &mut on_reasoning);
                        return Ok(full);
                    }
                }
            }
        }
    }
    splitter.flush(&mut on_content, &mut on_reasoning);
    Ok(full)
}

/// Complete a turn against the Google Gemini API (`POST /v1beta/models/{model}:streamGenerateContent?alt=sse`).
pub async fn complete_gemini(
    http: &reqwest::Client,
    config: &Config,
    model_id: &str,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let key = config.resolve_gemini_key().ok_or_else(|| {
        Error::msg(
            "Gemini API key is not configured. Add it in Providers & Keys (or set GEMINI_API_KEY), or switch to a Local Workspace model.",
        )
    })?;
    let model = model_id.strip_prefix("gemini/").unwrap_or(model_id);
    let base = config
        .gemini_base_url
        .as_deref()
        .unwrap_or(DEFAULT_GEMINI_BASE)
        .trim_end_matches('/');
    let url = format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse");

    let mut system_prompt = String::new();
    let mut contents: Vec<Value> = Vec::new();
    for turn in history {
        if turn.role == "system" {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&turn.content);
        } else {
            let role = if turn.role == "assistant" {
                "model"
            } else {
                "user"
            };
            contents.push(json!({
                "role": role,
                "parts": [{ "text": turn.content }],
            }));
        }
    }
    contents.push(json!({
        "role": "user",
        "parts": [{ "text": message }],
    }));

    let mut body = json!({
        "contents": contents,
    });
    if !system_prompt.is_empty() {
        body["systemInstruction"] = json!({
            "parts": [{ "text": system_prompt }]
        });
    }

    let req = http
        .post(&url)
        .header("x-goog-api-key", &key)
        .header("accept", "text/event-stream")
        .json(&body)
        .timeout(Duration::from_secs(600));

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        r = req.send() => r?,
    };
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Api {
            status,
            message: format!("Gemini API error ({status}): {}", crate::client::redact(&text)),
            code: None,
        });
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut splitter = crate::protocol::ThinkStreamSplitter::new();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = next else { break };
        let bytes = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(idx) = buffer.find("\n\n") {
            let raw = buffer[..idx].to_string();
            buffer.replace_range(..idx + 2, "");
            for line in raw.lines() {
                let line = line.trim_end_matches('\r');
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(parts) = v
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("content"))
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            let is_thought = part.get("thought").and_then(|t| t.as_bool()) == Some(true);
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    if is_thought {
                                        on_reasoning(text);
                                    } else {
                                        full.push_str(text);
                                        splitter.feed(text, &mut on_content, &mut on_reasoning);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    splitter.flush(&mut on_content, &mut on_reasoning);
    Ok(full)
}

fn clip_str(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_workspace_agent_runs_without_any_api_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\n",
        )
        .unwrap();
        let http = reqwest::Client::new();
        let cfg = Config::default();
        let mut streamed = String::new();
        let out = complete_local(
            &http,
            &cfg,
            "local/workspace:architect",
            dir.path(),
            "Inspect architecture and list files",
            &[],
            &CancellationToken::new(),
            |c| streamed.push_str(c),
            |_| {},
        )
        .await
        .unwrap();
        assert!(out.contains("list_files"));
        assert_eq!(out, streamed);
    }

    #[tokio::test]
    async fn discovers_models_dynamically_from_api_endpoints() {
        // 1. Mock Ollama `/api/tags`
        let (ollama_base, _t0) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/api/tags"));
            crate::testsupport::Response {
                status: 200,
                content_type: "application/json",
                body: r#"{"models":[{"name":"qwen2.5-coder:14b","details":{"parameter_size":"14B","quantization_level":"Q4_K_M"}}]}"#.into(),
            }
        })
        .await;

        // 2. Mock OpenAI `/v1/models`
        let (openai_base, _t1) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/v1/models"));
            crate::testsupport::Response {
                status: 200,
                content_type: "application/json",
                body: r#"{"data":[{"id":"gpt-4.1-live","created":1750000000,"owned_by":"openai"},{"id":"text-embedding-3-small","created":1700000000,"owned_by":"openai"}]}"#.into(),
            }
        })
        .await;

        // 3. Mock Anthropic `/v1/models`
        let (claude_base, _t2) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/v1/models"));
            crate::testsupport::Response {
                status: 200,
                content_type: "application/json",
                body: r#"{"data":[{"id":"claude-sonnet-4-live","display_name":"Claude Sonnet 4 (Live)","created_at":"2026-05-14T00:00:00Z"}]}"#.into(),
            }
        })
        .await;

        // 4. Mock Gemini `/v1beta/models`
        let (gemini_base, _t3) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/v1beta/models"));
            crate::testsupport::Response {
                status: 200,
                content_type: "application/json",
                body: r#"{"models":[{"name":"models/gemini-2.5-pro-live","displayName":"Gemini 2.5 Pro (Live)","inputTokenLimit":1048576,"supportedGenerationMethods":["generateContent"]}]}"#.into(),
            }
        })
        .await;

        let cfg = Config {
            ollama_base_url: Some(ollama_base),
            openai_api_key: Some("sk-test-openai".into()),
            openai_base_url: Some(openai_base),
            anthropic_api_key: Some("sk-ant-test".into()),
            anthropic_base_url: Some(claude_base),
            gemini_api_key: Some("AIza-test".into()),
            gemini_base_url: Some(gemini_base),
            ..Default::default()
        };

        let http = reqwest::Client::new();
        let report = discover_all_models(&http, &cfg).await;
        assert!(report
            .models
            .iter()
            .any(|m| m.id == "local/ollama:qwen2.5-coder:14b"));
        assert!(report.models.iter().any(|m| m.id == "openai/gpt-4.1-live"));
        assert!(!report
            .models
            .iter()
            .any(|m| m.id.contains("text-embedding")));
        assert!(report
            .models
            .iter()
            .any(|m| m.id == "claude/claude-sonnet-4-live"));
        assert!(report
            .models
            .iter()
            .any(|m| m.id == "gemini/gemini-2.5-pro-live"));
    }

    #[tokio::test]
    async fn streams_openai_anthropic_and_gemini_sse() {
        let (openai_base, _t1) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/v1/chat/completions"));
            crate::testsupport::Response {
                status: 200,
                content_type: "text/event-stream",
                body: "data: {\"choices\":[{\"delta\":{\"content\":\"Hello from OpenAI\"}}]}\n\ndata: [DONE]\n\n"
                    .into(),
            }
        })
        .await;

        let (claude_base, _t2) = crate::testsupport::spawn(|req| {
            assert!(req.path.ends_with("/v1/messages"));
            crate::testsupport::Response {
                status: 200,
                content_type: "text/event-stream",
                body: "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello from Claude\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n"
                    .into(),
            }
        })
        .await;

        let (gemini_base, _t3) = crate::testsupport::spawn(|req| {
            assert!(req.path.contains("/v1beta/models/gemini-2.5-pro:streamGenerateContent"));
            crate::testsupport::Response {
                status: 200,
                content_type: "text/event-stream",
                body: "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello from Gemini\"}]}}]}\n\n"
                    .into(),
            }
        })
        .await;

        let cfg = Config {
            openai_api_key: Some("sk-test-openai".into()),
            openai_base_url: Some(openai_base),
            anthropic_api_key: Some("sk-ant-test".into()),
            anthropic_base_url: Some(claude_base),
            gemini_api_key: Some("AIza-test".into()),
            gemini_base_url: Some(gemini_base),
            ..Default::default()
        };

        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();

        let r1 = complete_openai(
            &http,
            &cfg,
            "openai/gpt-4.1",
            "hi",
            &[],
            &cancel,
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(r1, "Hello from OpenAI");

        let r2 = complete_anthropic(
            &http,
            &cfg,
            "claude/claude-sonnet-4-20250514",
            "hi",
            &[],
            &cancel,
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(r2, "Hello from Claude");

        let r3 = complete_gemini(
            &http,
            &cfg,
            "gemini/gemini-2.5-pro",
            "hi",
            &[],
            &cancel,
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(r3, "Hello from Gemini");
    }
}
