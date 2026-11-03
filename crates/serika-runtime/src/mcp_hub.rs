//! The user's MCP servers: where they are defined, and live connections to them.
//!
//! Servers come from Serika's own `mcp.json` and are imported automatically from wherever the
//! user already set them up: the project's `.mcp.json`, Claude Code (`~/.claude.json`, user
//! and per-project scope), Claude Desktop, and Codex (`~/.codex/config.toml`). Imports are read
//! live, so they stay in sync with those tools. Any server can be switched off in Serika.
//!
//! Serika's own agent sees each tool as `mcp__<server>__<tool>`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::mcp::{Era, McpConnection, McpTool, ToolResult, Transport};

/// One configured server.
#[derive(Debug, Clone, Serialize)]
pub struct ServerEntry {
    pub name: String,
    pub transport: Transport,
    /// "Serika", "Project", "Claude Code", "Claude Code (project)", "Claude Desktop", "Codex".
    pub source: String,
    pub enabled: bool,
    /// Defined in Serika's own file, so Serika may edit or remove it.
    pub editable: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServerStatus {
    /// idle, connecting, ready, error
    pub state: String,
    pub error: Option<String>,
    pub protocol: Option<String>,
    pub era: Option<String>,
    pub server_name: Option<String>,
    pub tools: Vec<ToolInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

// ------------------------------------------------------------------ configuration

fn serika_file(data_dir: &Path) -> PathBuf {
    data_dir.join("mcp.json")
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// `${VAR}` and `${VAR:-default}`, as Claude Code expands them in MCP configs.
pub fn expand_env(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        match std::env::var(name) {
            Ok(value) if !value.is_empty() => out.push_str(&value),
            _ => out.push_str(default.unwrap_or("")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(expand_env).collect())
        .unwrap_or_default()
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), expand_env(s))))
                .collect()
        })
        .unwrap_or_default()
}

/// A server in the JSON shape Claude Code, Claude Desktop, and `.mcp.json` share.
pub fn parse_claude_server(value: &Value) -> Option<Transport> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let url = value.get("url").and_then(Value::as_str).map(expand_env);
    let headers = string_map(value.get("headers"));
    match (kind.as_str(), url) {
        ("sse", Some(url)) => Some(Transport::Sse { url, headers }),
        ("http" | "streamable-http" | "streamablehttp" | "", Some(url)) => Some(Transport::Http { url, headers }),
        _ => {
            let command = value.get("command").and_then(Value::as_str).map(expand_env)?;
            Some(Transport::Stdio {
                command,
                args: strings(value.get("args")),
                env: string_map(value.get("env")),
                cwd: value.get("cwd").and_then(Value::as_str).map(|c| PathBuf::from(expand_env(c))),
            })
        }
    }
}

/// `[mcp_servers.<name>]` tables from Codex's config.toml. Returns (name, transport, enabled).
pub fn parse_codex(text: &str) -> Vec<(String, Transport, bool)> {
    let Ok(doc) = text.parse::<toml::Table>() else { return Vec::new() };
    let Some(servers) = doc.get("mcp_servers").and_then(|v| v.as_table()) else { return Vec::new() };
    let mut out = Vec::new();
    for (name, value) in servers {
        let Some(table) = value.as_table() else { continue };
        let s = |k: &str| table.get(k).and_then(|v| v.as_str()).map(expand_env);
        let map = |k: &str| -> BTreeMap<String, String> {
            table
                .get(k)
                .and_then(|v| v.as_table())
                .map(|t| t.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), expand_env(s)))).collect())
                .unwrap_or_default()
        };
        let enabled = table.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        let transport = if let Some(url) = s("url") {
            let mut headers = map("http_headers");
            if let Some(var) = s("bearer_token_env_var") {
                if let Ok(token) = std::env::var(var) {
                    headers.insert("Authorization".into(), format!("Bearer {token}"));
                }
            }
            Transport::Http { url, headers }
        } else if let Some(command) = s("command") {
            let args = table
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).map(expand_env).collect())
                .unwrap_or_default();
            Transport::Stdio { command, args, env: map("env"), cwd: s("cwd").map(PathBuf::from) }
        } else {
            continue;
        };
        out.push((name.clone(), transport, enabled));
    }
    out
}

fn same_folder(a: &str, b: &Path) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    norm(a) == norm(&b.display().to_string())
}

fn claude_desktop_file() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
    } else {
        dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
    }
}

/// Every MCP server visible from `project`, Serika's own first. A name defined twice keeps the first.
pub fn load_servers(data_dir: &Path, project: &Path) -> Vec<ServerEntry> {
    let own = read_json(&serika_file(data_dir)).unwrap_or_else(|| json!({}));
    let disabled: HashSet<String> = strings(own.get("disabled")).into_iter().map(|n| n.to_ascii_lowercase()).collect();
    let mut out: Vec<ServerEntry> = Vec::new();
    let mut add = |name: &str, transport: Transport, source: &str, editable: bool, enabled: bool| {
        if out.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            return;
        }
        let enabled = enabled && !disabled.contains(&name.to_ascii_lowercase());
        out.push(ServerEntry { name: name.to_string(), transport, source: source.to_string(), enabled, editable });
    };
    let from_map = |map: Option<&Value>, source: &str, editable: bool, add: &mut dyn FnMut(&str, Transport, &str, bool, bool)| {
        if let Some(servers) = map.and_then(Value::as_object) {
            for (name, value) in servers {
                let off = value.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                if let Some(transport) = parse_claude_server(value) {
                    add(name, transport, source, editable, !off);
                }
            }
        }
    };
    from_map(own.get("mcpServers"), "Serika", true, &mut add);
    if let Some(project_file) = read_json(&project.join(".mcp.json")) {
        from_map(project_file.get("mcpServers"), "Project", false, &mut add);
    }
    if let Some(home) = dirs::home_dir() {
        let claude_file = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(|d| PathBuf::from(d).join(".claude.json"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| home.join(".claude.json"));
        if let Some(claude) = read_json(&claude_file) {
            if let Some(projects) = claude.get("projects").and_then(Value::as_object) {
                for (path, entry) in projects {
                    if same_folder(path, project) {
                        from_map(entry.get("mcpServers"), "Claude Code (project)", false, &mut add);
                    }
                }
            }
            from_map(claude.get("mcpServers"), "Claude Code", false, &mut add);
        }
        let codex_home = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
        if let Ok(text) = std::fs::read_to_string(codex_home.join("config.toml")) {
            for (name, transport, enabled) in parse_codex(&text) {
                add(&name, transport, "Codex", false, enabled);
            }
        }
    }
    if let Some(desktop) = claude_desktop_file().and_then(|p| read_json(&p)) {
        from_map(desktop.get("mcpServers"), "Claude Desktop", false, &mut add);
    }
    out
}

/// Switch a server on or off in Serika, whatever file defines it.
pub fn set_enabled(data_dir: &Path, name: &str, enabled: bool) -> crate::Result<()> {
    let path = serika_file(data_dir);
    let mut own = read_json(&path).unwrap_or_else(|| json!({}));
    let mut disabled: Vec<String> = strings(own.get("disabled"));
    disabled.retain(|n| !n.eq_ignore_ascii_case(name));
    if !enabled {
        disabled.push(name.to_string());
    }
    own["disabled"] = json!(disabled);
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(path, serde_json::to_string_pretty(&own)?)?;
    Ok(())
}

/// Add or replace a server in Serika's own file. `definition` uses the Claude JSON shape.
pub fn save_server(data_dir: &Path, name: &str, definition: &Value) -> crate::Result<()> {
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(crate::Error::msg("server names use letters, digits, - _ and ."));
    }
    if parse_claude_server(definition).is_none() {
        return Err(crate::Error::msg("a server needs a \"command\" (local) or a \"url\" (remote)"));
    }
    let path = serika_file(data_dir);
    let mut own = read_json(&path).unwrap_or_else(|| json!({}));
    if !own.get("mcpServers").map(Value::is_object).unwrap_or(false) {
        own["mcpServers"] = json!({});
    }
    own["mcpServers"][name] = definition.clone();
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(path, serde_json::to_string_pretty(&own)?)?;
    Ok(())
}

pub fn remove_server(data_dir: &Path, name: &str) -> crate::Result<()> {
    let path = serika_file(data_dir);
    let mut own = read_json(&path).unwrap_or_else(|| json!({}));
    if let Some(map) = own.get_mut("mcpServers").and_then(Value::as_object_mut) {
        map.remove(name);
    }
    std::fs::write(path, serde_json::to_string_pretty(&own)?)?;
    Ok(())
}

/// A server in the JSON shape Claude Code reads from `--mcp-config`.
pub fn claude_json(transport: &Transport) -> Value {
    match transport {
        Transport::Stdio { command, args, env, cwd } => {
            let mut v = json!({"type": "stdio", "command": command, "args": args, "env": env});
            if let Some(cwd) = cwd {
                v["cwd"] = json!(cwd);
            }
            v
        }
        Transport::Http { url, headers } => json!({"type": "http", "url": url, "headers": headers}),
        Transport::Sse { url, headers } => json!({"type": "sse", "url": url, "headers": headers}),
    }
}

/// A name the model can use in a tool name: letters, digits, - and _.
pub fn tool_prefix(server: &str) -> String {
    server
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ------------------------------------------------------------------ connections

struct Live {
    key: String,
    conn: McpConnection,
    tools: Vec<McpTool>,
}

#[derive(Default)]
pub struct McpHub {
    live: Mutex<HashMap<String, Arc<Live>>>,
    status: StdMutex<HashMap<String, ServerStatus>>,
}

/// A tool ready for the agent: the server it lives on and its definition.
#[derive(Debug, Clone)]
pub struct AgentTool {
    pub server: String,
    pub tool: McpTool,
}

impl AgentTool {
    pub fn full_name(&self) -> String {
        format!("mcp__{}__{}", tool_prefix(&self.server), self.tool.name)
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

impl McpHub {
    fn set_status(&self, name: &str, status: ServerStatus) {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), status);
    }

    pub fn statuses(&self) -> HashMap<String, ServerStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Connect every enabled server (reusing live connections) and return their tools.
    pub async fn ensure(&self, entries: &[ServerEntry], root: &Path) -> Vec<AgentTool> {
        let wanted: Vec<&ServerEntry> = entries.iter().filter(|e| e.enabled).collect();
        // Drop connections whose server is gone, switched off, or redefined.
        {
            let mut live = self.live.lock().await;
            let stale: Vec<String> = live
                .iter()
                .filter(|(name, l)| {
                    !wanted
                        .iter()
                        .any(|e| &e.name == *name && serde_json::to_string(&e.transport).unwrap_or_default() == l.key)
                })
                .map(|(n, _)| n.clone())
                .collect();
            for name in stale {
                if let Some(old) = live.remove(&name) {
                    if let Ok(old) = Arc::try_unwrap(old) {
                        tokio::spawn(async move { old.conn.close().await });
                    }
                }
                self.status.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
            }
        }
        let connects = wanted.iter().map(|entry| async move {
            let key = serde_json::to_string(&entry.transport).unwrap_or_default();
            if let Some(existing) = self.live.lock().await.get(&entry.name).cloned() {
                return Some((entry.name.clone(), existing));
            }
            // A server that just failed is not retried on every message.
            if self.statuses().get(&entry.name).map(|s| s.state == "error").unwrap_or(false) {
                return None;
            }
            self.set_status(&entry.name, ServerStatus { state: "connecting".into(), ..Default::default() });
            match self.open(entry, root, &key).await {
                Ok(live) => {
                    let live = Arc::new(live);
                    self.live.lock().await.insert(entry.name.clone(), live.clone());
                    Some((entry.name.clone(), live))
                }
                Err(err) => {
                    self.set_status(&entry.name, ServerStatus { state: "error".into(), error: Some(err), ..Default::default() });
                    None
                }
            }
        });
        let ready = futures_util::future::join_all(connects).await;
        let mut tools = Vec::new();
        for (server, live) in ready.into_iter().flatten() {
            for tool in &live.tools {
                tools.push(AgentTool { server: server.clone(), tool: tool.clone() });
            }
        }
        tools
    }

    async fn open(&self, entry: &ServerEntry, root: &Path, key: &str) -> Result<Live, String> {
        let conn = McpConnection::connect(&entry.transport, "serika-code", crate::client::VERSION, Some(root), CONNECT_TIMEOUT)
            .await
            .map_err(|e| e.to_string())?;
        let tools = match conn.list_tools().await {
            Ok(tools) => tools,
            Err(err) => {
                let tail = conn.stderr_tail();
                conn.close().await;
                return Err(if tail.trim().is_empty() { err.to_string() } else { format!("{err}\n{}", tail.trim()) });
            }
        };
        self.set_status(&entry.name, ServerStatus {
            state: "ready".into(),
            error: None,
            protocol: Some(conn.protocol_version().to_string()),
            era: Some(match conn.era() { Era::Modern => "modern".into(), Era::Legacy => "legacy".into() }),
            server_name: conn.server_name(),
            tools: tools.iter().map(|t| ToolInfo { name: t.name.clone(), description: t.description.clone() }).collect(),
        });
        Ok(Live { key: key.to_string(), conn, tools })
    }

    /// Reconnect one server now (after an error, or to refresh its tools).
    pub async fn restart(&self, entries: &[ServerEntry], name: &str, root: &Path) -> ServerStatus {
        if let Some(old) = self.live.lock().await.remove(name) {
            if let Ok(old) = Arc::try_unwrap(old) {
                old.conn.close().await;
            }
        }
        self.status.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
        if let Some(entry) = entries.iter().find(|e| e.name == name) {
            let mut one = entry.clone();
            one.enabled = true;
            self.ensure(std::slice::from_ref(&one), root).await;
        }
        self.statuses().get(name).cloned().unwrap_or_default()
    }

    /// Run a tool by its agent name (`mcp__server__tool`).
    pub async fn call(&self, full_name: &str, arguments: Value, timeout: Duration) -> Result<ToolResult, String> {
        let live: Vec<(String, Arc<Live>)> = self.live.lock().await.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (server, conn) in live {
            let prefix = format!("mcp__{}__", tool_prefix(&server));
            if let Some(tool) = full_name.strip_prefix(&prefix) {
                if conn.tools.iter().any(|t| t.name == tool) {
                    return conn.conn.call_tool(tool, arguments, timeout).await.map_err(|e| e.to_string());
                }
            }
        }
        Err(format!("no connected MCP server offers {full_name}"))
    }

    pub async fn shutdown(&self) {
        let all: Vec<Arc<Live>> = self.live.lock().await.drain().map(|(_, v)| v).collect();
        for live in all {
            if let Ok(live) = Arc::try_unwrap(live) {
                live.conn.close().await;
            }
        }
    }
}

/// The part of the system prompt that lists MCP tools, kept within a budget.
pub fn instructions(tools: &[AgentTool]) -> String {
    if tools.is_empty() {
        return String::new();
    }
    let mut out = String::from("MCP tools (from the user's MCP servers; call them like any other tool, arguments as JSON):\n");
    let mut used = out.len();
    for tool in tools {
        let schema = tool.tool.input_schema.get("properties").cloned().unwrap_or_else(|| json!({}));
        let mut schema = schema.to_string();
        if schema.len() > 700 {
            schema = schema.chars().take(700).collect::<String>() + "…";
        }
        let description: String = tool.tool.description.chars().take(300).collect();
        let line = format!("- {} {}: {}\n", tool.full_name(), schema, description.replace('\n', " "));
        if used + line.len() > 16_000 {
            out.push_str("- (more tools are available but not listed)\n");
            break;
        }
        used += line.len();
        out.push_str(&line);
    }
    out
}

/// Turn a tool result into the text the model reads.
pub fn result_text(result: &ToolResult) -> String {
    let mut text = result.text.clone();
    if !result.images.is_empty() {
        text.push_str(&format!("\n[{} image(s) returned]", result.images.len()));
    }
    if text.chars().count() > 30_000 {
        text = text.chars().take(30_000).collect::<String>() + "\n… [cut]";
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_and_codex_shapes() {
        std::env::set_var("SERIKA_TEST_TOKEN", "abc");
        let stdio = parse_claude_server(&json!({"command": "npx", "args": ["-y", "pkg"], "env": {"K": "${SERIKA_TEST_TOKEN}"}})).unwrap();
        match stdio {
            Transport::Stdio { command, args, env, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args, vec!["-y", "pkg"]);
                assert_eq!(env.get("K").map(String::as_str), Some("abc"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_claude_server(&json!({"type": "sse", "url": "https://x/sse"})), Some(Transport::Sse { .. })));
        assert!(matches!(parse_claude_server(&json!({"url": "https://x/mcp"})), Some(Transport::Http { .. })));
        assert_eq!(expand_env("${SERIKA_NOPE_VAR:-dflt}/x"), "dflt/x");

        let codex = parse_codex("[mcp_servers.docs]\ncommand = \"uvx\"\nargs = [\"docs-mcp\"]\n[mcp_servers.remote]\nurl = \"https://r/mcp\"\nenabled = false\n");
        assert_eq!(codex.len(), 2);
        assert!(matches!(&codex[0].1, Transport::Stdio { command, .. } if command == "uvx"));
        assert!(!codex.iter().find(|(n, _, _)| n == "remote").unwrap().2);
    }

    #[test]
    fn serika_servers_save_toggle_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(".mcp.json"), r#"{"mcpServers":{"proj-tool":{"command":"node","args":["s.js"]}}}"#).unwrap();
        save_server(&data, "files", &json!({"command": "npx", "args": ["fs"]})).unwrap();
        assert!(save_server(&data, "bad name", &json!({"command": "x"})).is_err());
        let all = load_servers(&data, &project);
        let files = all.iter().find(|s| s.name == "files").unwrap();
        assert!(files.editable && files.enabled && files.source == "Serika");
        assert_eq!(all.iter().find(|s| s.name == "proj-tool").unwrap().source, "Project");
        set_enabled(&data, "proj-tool", false).unwrap();
        assert!(!load_servers(&data, &project).iter().find(|s| s.name == "proj-tool").unwrap().enabled);
        remove_server(&data, "files").unwrap();
        assert!(!load_servers(&data, &project).iter().any(|s| s.name == "files"));
        assert_eq!(tool_prefix("my server!"), "my_server_");
    }
}
