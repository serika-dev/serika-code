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
            ProviderKind::Local => "Local Harness (No API)",
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

/// Detect local CLI coding agents installed on `PATH` (like T3 Code).
pub fn detect_local_harnesses() -> Vec<LocalHarness> {
    let candidates = [
        (
            "cli/claude",
            "Claude Code CLI",
            "claude",
            "Runs local `claude -p` CLI harness in your workspace (no API key needed)",
        ),
        (
            "cli/codex",
            "Codex CLI",
            "codex",
            "Runs local `codex exec` CLI harness in your workspace (no API key needed)",
        ),
        (
            "cli/gemini",
            "Gemini CLI",
            "gemini",
            "Runs local `gemini -p` CLI harness in your workspace (no API key needed)",
        ),
        (
            "cli/opencode",
            "OpenCode CLI",
            "opencode",
            "Runs local `opencode run` CLI harness in your workspace",
        ),
        (
            "cli/ollama",
            "Ollama Local",
            "ollama",
            "Runs local Ollama model (`qwen2.5-coder` / `llama3.2`) offline",
        ),
    ];

    let mut out = vec![LocalHarness {
        id: "local/workspace".to_string(),
        name: "Local Workspace Agent".to_string(),
        binary: "built-in".to_string(),
        available: true,
        description: "Built-in offline workspace inspector, test runner & git assistant (Zero API)"
            .to_string(),
    }];

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

/// Returns the combined model catalog of Local (API-free) harnesses + Optional OpenAI, Claude, and Gemini APIs.
pub fn builtin_provider_models(harnesses: &[LocalHarness]) -> Vec<ChatModel> {
    let mut models = Vec::new();

    for h in harnesses {
        if h.available {
            models.push(ChatModel {
                id: h.id.clone(),
                name: h.name.clone(),
                description: Some(h.description.clone()),
                max_tokens: Some(128_000),
                provider: Some("local".to_string()),
                premium: false,
                is_default: h.id == "local/workspace",
                usage_multiplier: None,
                availability: Some("ready".to_string()),
            });
        }
    }

    // Optional Anthropic Claude API models
    for (id, name, desc) in [
        (
            "claude/claude-sonnet-4-20250514",
            "Claude Sonnet 4",
            "Anthropic Claude API (optional BYOK key)",
        ),
        (
            "claude/claude-3-7-sonnet-latest",
            "Claude 3.7 Sonnet",
            "Anthropic Claude API extended thinking & coding",
        ),
        (
            "claude/claude-3-5-haiku-latest",
            "Claude 3.5 Haiku",
            "Fast Anthropic Claude API model",
        ),
    ] {
        models.push(ChatModel {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(desc.to_string()),
            max_tokens: Some(200_000),
            provider: Some("claude".to_string()),
            premium: false,
            is_default: false,
            usage_multiplier: None,
            availability: Some("optional_api".to_string()),
        });
    }

    // Optional OpenAI API models
    for (id, name, desc) in [
        (
            "openai/gpt-4.1",
            "GPT-4.1",
            "OpenAI flagship coding model (optional BYOK key)",
        ),
        (
            "openai/o4-mini",
            "o4-mini",
            "OpenAI fast reasoning model for engineering",
        ),
        (
            "openai/gpt-4o",
            "GPT-4o",
            "OpenAI omni multimodal model",
        ),
    ] {
        models.push(ChatModel {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(desc.to_string()),
            max_tokens: Some(128_000),
            provider: Some("openai".to_string()),
            premium: false,
            is_default: false,
            usage_multiplier: None,
            availability: Some("optional_api".to_string()),
        });
    }

    // Optional Google Gemini API models
    for (id, name, desc) in [
        (
            "gemini/gemini-2.5-pro",
            "Gemini 2.5 Pro",
            "Google Gemini deep coding & reasoning (optional BYOK key)",
        ),
        (
            "gemini/gemini-2.5-flash",
            "Gemini 2.5 Flash",
            "Google Gemini high-speed low-latency model",
        ),
    ] {
        models.push(ChatModel {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(desc.to_string()),
            max_tokens: Some(1_000_000),
            provider: Some("gemini".to_string()),
            premium: false,
            is_default: false,
            usage_multiplier: None,
            availability: Some("optional_api".to_string()),
        });
    }

    models
}

/// Complete a turn using the built-in API-free Local Workspace Agent or an installed local CLI harness.
pub async fn complete_local(
    model_id: &str,
    project: &Path,
    message: &str,
    history: &[ChatTurn],
    cancel: &CancellationToken,
    mut on_content: impl FnMut(&str),
) -> Result<String> {
    if model_id == "local/workspace" || model_id == "local" {
        let reply = synthesize_local_workspace_turn(project, message, history);
        on_content(&reply);
        return Ok(reply);
    }

    let harness_bin = match model_id {
        "cli/claude" => "claude",
        "cli/codex" => "codex",
        "cli/gemini" => "gemini",
        "cli/opencode" => "opencode",
        "cli/ollama" => "ollama",
        other => other.strip_prefix("cli/").unwrap_or("claude"),
    };

    if !binary_on_path(harness_bin) {
        // Gracefully fall back to built-in local workspace agent if CLI binary is missing
        let reply = synthesize_local_workspace_turn(project, message, history);
        on_content(&reply);
        return Ok(reply);
    }

    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        match harness_bin {
            "claude" => {
                c.args(["/C", "claude", "-p", message]);
            }
            "codex" => {
                c.args(["/C", "codex", "exec", message]);
            }
            "gemini" => {
                c.args(["/C", "gemini", "-p", message]);
            }
            "opencode" => {
                c.args(["/C", "opencode", "run", message]);
            }
            "ollama" => {
                c.args(["/C", "ollama", "run", "qwen2.5-coder", message]);
            }
            _ => {
                c.args(["/C", harness_bin, message]);
            }
        }
        c
    } else {
        let mut c = Command::new(harness_bin);
        match harness_bin {
            "claude" | "gemini" => {
                c.args(["-p", message]);
            }
            "codex" => {
                c.args(["exec", message]);
            }
            "opencode" => {
                c.args(["run", message]);
            }
            "ollama" => {
                c.args(["run", "qwen2.5-coder", message]);
            }
            _ => {
                c.arg(message);
            }
        }
        c
    };

    cmd.current_dir(project)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

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
            Err(err) => return Err(Error::msg(format!("error reading `{harness_bin}` output: {err}"))),
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

fn synthesize_local_workspace_turn(project: &Path, message: &str, history: &[ChatTurn]) -> String {
    // If the latest user message is tool output, summarize the findings cleanly!
    if message.starts_with("Tool results:\n") {
        let body = message.trim_start_matches("Tool results:\n").trim();
        return format!(
            "### Local Workspace Execution Complete\n\nI executed the workspace operation locally (no external API used):\n\n```text\n{}\n```",
            clip_str(body, 4000)
        );
    }

    let lower = message.to_ascii_lowercase();
    let already_ran_tools = history.iter().any(|t| t.content.starts_with("Tool results:\n"));

    if !already_ran_tools {
        if lower.contains("git")
            || lower.contains("diff")
            || lower.contains("uncommitted")
            || lower.contains("status")
        {
            return "Inspecting repository status and diffs locally:\n```serika-tool\n{\"name\":\"git_status\",\"arguments\":{}}\n```\n```serika-tool\n{\"name\":\"git_diff\",\"arguments\":{\"staged\":false}}\n```"
                .to_string();
        }
        if lower.contains("test") || lower.contains("cargo test") || lower.contains("verify") {
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
        if lower.contains("architecture")
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
        "**Local Workspace Agent** (`{project_name}` — API-free mode)\n\n\
         - **Workspace**: `{}`\n\
         - **Direct Local Actions**: Ask me to *\"inspect architecture\"*, *\"review git status and diffs\"*, *\"run tests\"*, or `run <command>` to execute local tools without any API.\n\
         - **Local CLI Harnesses & Optional APIs**: Open **Providers & Keys** in the sidebar to switch between local CLIs (`claude`, `codex`, `gemini`, `ollama`) or add an optional **OpenAI**, **Gemini**, or **Claude** API key.",
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
    mut on_content: impl FnMut(&str),
    mut on_reasoning: impl FnMut(&str),
) -> Result<String> {
    let key = config.resolve_openai_key().ok_or_else(|| {
        Error::msg(
            "OpenAI API key is not configured. Add it in Providers & Keys (or set OPENAI_API_KEY), or switch to a Local Harness.",
        )
    })?;
    let model = model_id.strip_prefix("openai/").unwrap_or(model_id);
    let base = config
        .openai_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OPENAI_BASE)
        .trim_end_matches('/');
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

    let req = http
        .post(&url)
        .bearer_auth(&key)
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
            message: format!("OpenAI API error ({status}): {}", crate::client::redact(&text)),
            code: None,
        });
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
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
                    return Ok(full);
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                    {
                        if let Some(c) = delta.get("content").and_then(|s| s.as_str()) {
                            if !c.is_empty() {
                                full.push_str(c);
                                on_content(c);
                            }
                        }
                        if let Some(r) = delta.get("reasoning_content").and_then(|s| s.as_str()) {
                            if !r.is_empty() {
                                on_reasoning(r);
                            }
                        }
                    }
                }
            }
        }
    }
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
            "Claude API key is not configured. Add it in Providers & Keys (or set ANTHROPIC_API_KEY), or switch to a Local Harness.",
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
            let role = if turn.role == "assistant" { "assistant" } else { "user" };
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
                            if let Some(text) = delta.get("text").and_then(|s| s.as_str()) {
                                if !text.is_empty() {
                                    full.push_str(text);
                                    on_content(text);
                                }
                            }
                            if let Some(thinking) = delta.get("thinking").and_then(|s| s.as_str()) {
                                if !thinking.is_empty() {
                                    on_reasoning(thinking);
                                }
                            }
                        }
                    } else if v.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
                        return Ok(full);
                    }
                }
            }
        }
    }
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
) -> Result<String> {
    let key = config.resolve_gemini_key().ok_or_else(|| {
        Error::msg(
            "Gemini API key is not configured. Add it in Providers & Keys (or set GEMINI_API_KEY), or switch to a Local Harness.",
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
            let role = if turn.role == "assistant" { "model" } else { "user" };
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
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    full.push_str(text);
                                    on_content(text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
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
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        let mut streamed = String::new();
        let out = complete_local(
            "local/workspace",
            dir.path(),
            "Inspect architecture and list files",
            &[],
            &CancellationToken::new(),
            |c| streamed.push_str(c),
        )
        .await
        .unwrap();
        assert!(out.contains("list_files"));
        assert_eq!(out, streamed);
    }

    #[tokio::test]
    async fn streams_openai_anthropic_and_gemini_sse() {
        // 1. OpenAI SSE mock
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

        // 2. Anthropic Claude SSE mock
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

        // 3. Gemini SSE mock
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

        let mut cfg = Config::default();
        cfg.openai_api_key = Some("sk-test-openai".into());
        cfg.openai_base_url = Some(openai_base);
        cfg.anthropic_api_key = Some("sk-ant-test".into());
        cfg.anthropic_base_url = Some(claude_base);
        cfg.gemini_api_key = Some("AIza-test".into());
        cfg.gemini_base_url = Some(gemini_base);

        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();

        let r1 = complete_openai(&http, &cfg, "openai/gpt-4.1", "hi", &[], &cancel, |_| {}, |_| {})
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

        let r3 = complete_gemini(&http, &cfg, "gemini/gemini-2.5-pro", "hi", &[], &cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(r3, "Hello from Gemini");
    }
}
