//! Model Context Protocol client.
//!
//! Speaks both protocol eras:
//!
//! * **Modern** (revision 2026-07-28): stateless. Every request carries the protocol version,
//!   client info and client capabilities in `params._meta`; there is no `initialize` handshake.
//!   `server/discover` tells us what the server supports. Over Streamable HTTP every message is
//!   its own POST with the `MCP-Protocol-Version` / `Mcp-Method` / `Mcp-Name` / `Mcp-Param-*`
//!   headers.
//! * **Legacy** (2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05): `initialize` handshake,
//!   `Mcp-Session-Id` sessions over Streamable HTTP, and the deprecated HTTP+SSE transport.
//!
//! The era is detected per connection: a modern `server/discover` probe goes first and the
//! client falls back to `initialize` when the server does not recognise it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::{Error, Result};

/// The current protocol revision (stateless, no handshake).
pub const MODERN_VERSION: &str = "2026-07-28";
/// Handshake-based revisions, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// How long a stdio server gets to answer the modern probe before `initialize` is sent too.
const PROBE_WAIT: Duration = Duration::from_secs(3);
/// How long a stdio server gets to exit after its stdin closes.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Rounds of `input_required` answered before giving up.
const MAX_INPUT_ROUNDS: usize = 8;
/// Bytes of a stdio server's stderr kept for error messages.
const STDERR_TAIL: usize = 4096;
const BASE64_SENTINEL_PREFIX: &str = "=?base64?";
const BASE64_SENTINEL_SUFFIX: &str = "?=";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Where an MCP server lives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Transport {
    /// A local process speaking newline-delimited JSON-RPC on stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Streamable HTTP. Falls back to legacy sessions and then HTTP+SSE automatically.
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    /// The deprecated 2024-11-05 HTTP+SSE transport, used explicitly.
    Sse {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

impl Transport {
    /// A short human description used in error messages until the server names itself.
    pub fn label(&self) -> String {
        match self {
            Transport::Stdio { command, args, .. } => {
                let mut text = command.clone();
                for arg in args.iter().take(4) {
                    text.push(' ');
                    text.push_str(arg);
                }
                if args.len() > 4 {
                    text.push_str(" ...");
                }
                format!("`{text}`")
            }
            Transport::Http { url, .. } | Transport::Sse { url, .. } => url.clone(),
        }
    }
}

/// Which protocol family a connection negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Era {
    Modern,
    Legacy,
}

/// A tool the server offers.
#[derive(Debug, Clone, Serialize)]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
}

/// A tool call's outcome, flattened for the model.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
    /// `(mime type, base64 data)` pairs.
    pub images: Vec<(String, String)>,
}

/// A live connection to one MCP server. Cheap to share: every method takes `&self`, and
/// concurrent requests are multiplexed by JSON-RPC id.
pub struct McpConnection {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for McpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConnection")
            .field("label", &self.inner.label)
            .field("era", &self.inner.era)
            .field("protocol_version", &self.inner.version)
            .finish()
    }
}

impl McpConnection {
    /// Connects and negotiates the protocol era and version. `timeout` bounds the whole
    /// connection setup; it is also the default timeout of later `list_tools` calls.
    pub async fn connect(
        transport: &Transport,
        client_name: &str,
        client_version: &str,
        root: Option<&Path>,
        timeout: Duration,
    ) -> Result<Self> {
        Self::connect_with(transport, client_name, client_version, root, timeout, PROBE_WAIT).await
    }

    async fn connect_with(
        transport: &Transport,
        client_name: &str,
        client_version: &str,
        root: Option<&Path>,
        timeout: Duration,
        probe_wait: Duration,
    ) -> Result<Self> {
        let started = Instant::now();
        let deadline = started + timeout;
        let ctx = ConnectCtx {
            client_info: json!({ "name": client_name, "version": client_version }),
            root: root.map(Path::to_path_buf),
            label: transport.label(),
            deadline,
            timeout,
            ids: AtomicU64::new(1),
        };
        let (wire, negotiated) = match transport {
            Transport::Stdio { command, args, env, cwd } => {
                connect_stdio(&ctx, command, args, env, cwd.as_deref(), probe_wait).await?
            }
            Transport::Http { url, headers } => connect_http(&ctx, url, headers).await?,
            Transport::Sse { url, headers } => {
                let client = http_client()?;
                let sse = match connect_sse(&client, url, headers, ctx.root.clone(), ctx.remaining()).await {
                    Ok(sse) => sse,
                    Err(failure) => return Err(ctx.error(failure, "connect (HTTP+SSE)", "")),
                };
                let wire = Wire::Sse(sse);
                let negotiated = legacy_handshake(&ctx, &wire, LEGACY_VERSIONS[0]).await?;
                (wire, negotiated)
            }
        };
        let label = negotiated
            .server_info
            .as_ref()
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str)
            .map(|name| format!("'{name}'"))
            .unwrap_or_else(|| ctx.label.clone());
        tracing::debug!(
            server = %label,
            era = ?negotiated.era,
            version = %negotiated.version,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MCP server connected"
        );
        let capabilities = modern_capabilities(ctx.root.as_deref());
        Ok(Self {
            inner: Arc::new(Inner {
                wire,
                era: negotiated.era,
                version: negotiated.version,
                server_info: negotiated.server_info,
                instructions: negotiated.instructions,
                meta: json!({
                    META_PROTOCOL_VERSION: MODERN_VERSION,
                    META_CLIENT_INFO: ctx.client_info,
                    META_CLIENT_CAPABILITIES: capabilities,
                }),
                root: ctx.root,
                ids: ctx.ids,
                default_timeout: timeout.max(Duration::from_secs(5)),
                label,
                tool_headers: Mutex::new(None),
            }),
        })
    }

    pub fn era(&self) -> Era {
        self.inner.era
    }

    pub fn protocol_version(&self) -> &str {
        &self.inner.version
    }

    /// The server's self-reported name (`serverInfo.name`).
    pub fn server_name(&self) -> Option<String> {
        self.inner
            .server_info
            .as_ref()
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Usage instructions the server offers for the model.
    pub fn instructions(&self) -> Option<String> {
        self.inner.instructions.clone()
    }

    /// Lists every tool, following `nextCursor` pagination.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let inner = &self.inner;
        let deadline = Instant::now() + inner.default_timeout;
        let mut tools = Vec::new();
        let mut headers = HashMap::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = inner.request("tools/list", params, remaining(deadline)).await?;
            if let Some(list) = result.get("tools").and_then(Value::as_array) {
                for tool in list {
                    let Some(name) = tool.get("name").and_then(Value::as_str) else { continue };
                    let input_schema = tool
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object" }));
                    let params = header_params(&input_schema);
                    if !params.is_empty() {
                        headers.insert(name.to_string(), params);
                    }
                    let title = tool
                        .get("title")
                        .and_then(Value::as_str)
                        .or_else(|| tool.pointer("/annotations/title").and_then(Value::as_str))
                        .map(str::to_string);
                    tools.push(McpTool {
                        name: name.to_string(),
                        title,
                        description: tool
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        input_schema,
                    });
                }
            }
            pages += 1;
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|next| !next.is_empty())
                .map(str::to_string);
            if cursor.is_none() || pages >= 1000 {
                break;
            }
        }
        *lock(&inner.tool_headers) = Some(headers);
        Ok(tools)
    }

    /// Calls a tool. Protocol-level failures are `Err`; a tool that ran and failed returns
    /// `Ok` with `is_error` set.
    pub async fn call_tool(&self, name: &str, arguments: Value, timeout: Duration) -> Result<ToolResult> {
        let inner = &self.inner;
        let deadline = Instant::now() + timeout;
        // Modern Streamable HTTP mirrors some arguments into headers; that needs the schemas.
        if inner.era == Era::Modern
            && matches!(inner.wire, Wire::Http(_))
            && lock(&inner.tool_headers).is_none()
        {
            if let Err(err) = self.list_tools().await {
                tracing::debug!(server = %inner.label, "could not list tools before a call: {err}");
            }
        }
        let arguments = if arguments.is_null() { json!({}) } else { arguments };
        let params = json!({ "name": name, "arguments": arguments });
        let result = inner.request_with_input("tools/call", params, deadline).await?;
        Ok(flatten_tool_result(&result))
    }

    /// The last few KB a stdio server wrote to stderr ("" for HTTP transports).
    pub fn stderr_tail(&self) -> String {
        self.inner.wire.stderr_tail()
    }

    /// Ends the connection: closes a stdio server's stdin (killing it if it lingers) or
    /// ends a legacy HTTP session.
    pub async fn close(self) {
        self.inner.wire.close().await;
    }
}

// ---------------------------------------------------------------------------------------------
// Session layer
// ---------------------------------------------------------------------------------------------

/// Path inside the arguments (chain of `properties` keys) and the header name it mirrors into.
type HeaderParam = (Vec<String>, String);

struct Inner {
    wire: Wire,
    era: Era,
    version: String,
    server_info: Option<Value>,
    instructions: Option<String>,
    /// The modern `_meta` envelope sent on every request.
    meta: Value,
    root: Option<PathBuf>,
    ids: AtomicU64,
    default_timeout: Duration,
    label: String,
    tool_headers: Mutex<Option<HashMap<String, Vec<HeaderParam>>>>,
}

impl Inner {
    async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let modern = self.era == Era::Modern;
        let params = if modern { with_meta(params, &self.meta) } else { params };
        let extra = if modern && matches!(self.wire, Wire::Http(_)) {
            self.modern_headers(method, &params)
        } else {
            Vec::new()
        };
        call(&self.wire, &self.ids, method, params, extra, timeout)
            .await
            .map_err(|failure| self.error(failure, method, timeout))
    }

    /// A request that may come back `input_required` (modern multi-round-trip). Roots requests
    /// are answered with the project root; anything else (sampling, elicitation) is refused.
    async fn request_with_input(&self, method: &str, base: Value, deadline: Instant) -> Result<Value> {
        let mut params = base.clone();
        for _ in 0..MAX_INPUT_ROUNDS {
            let budget = remaining(deadline);
            if budget.is_zero() {
                return Err(Error::msg(format!(
                    "MCP server {}: {method} timed out while the server kept asking for input",
                    self.label
                )));
            }
            let result = self.request(method, params, budget).await?;
            if result.get("resultType").and_then(Value::as_str) != Some("input_required") {
                return Ok(result);
            }
            let responses = answer_input_requests(result.get("inputRequests"), self.root.as_deref())
                .map_err(|what| {
                    Error::msg(format!(
                        "MCP server {} asked for {what} to finish {method}, which Serika cannot provide",
                        self.label
                    ))
                })?;
            let state = result.get("requestState").cloned();
            if responses.is_empty() && state.is_some() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            params = base.clone();
            if let Some(object) = params.as_object_mut() {
                if !responses.is_empty() {
                    object.insert("inputResponses".into(), Value::Object(responses));
                }
                if let Some(state) = state {
                    object.insert("requestState".into(), state);
                }
            }
        }
        Err(Error::msg(format!(
            "MCP server {}: {method} still needed input after {MAX_INPUT_ROUNDS} rounds",
            self.label
        )))
    }

    /// `Mcp-Name` and `Mcp-Param-*` headers for a modern Streamable HTTP request.
    fn modern_headers(&self, method: &str, params: &Value) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let name_field = match method {
            "tools/call" | "prompts/get" => Some("name"),
            "resources/read" => Some("uri"),
            "tasks/get" | "tasks/update" | "tasks/cancel" => Some("taskId"),
            _ => None,
        };
        if let Some(value) = name_field.and_then(|field| params.get(field)).and_then(Value::as_str) {
            headers.push(("Mcp-Name".to_string(), encode_header_value(value)));
        }
        if method == "tools/call" {
            let tool = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = params.get("arguments").unwrap_or(&Value::Null);
            if let Some(declared) = lock(&self.tool_headers).as_ref().and_then(|map| map.get(tool)) {
                headers.extend(param_headers(declared, arguments));
            }
        }
        headers
    }

    fn error(&self, failure: Failure, method: &str, timeout: Duration) -> Error {
        failure_error(failure, &self.label, method, timeout, &self.wire.stderr_tail())
    }
}

/// Answers a modern `inputRequests` map. `Err` names the first request we cannot serve.
fn answer_input_requests(requests: Option<&Value>, root: Option<&Path>) -> std::result::Result<Map<String, Value>, String> {
    let mut responses = Map::new();
    let Some(requests) = requests.and_then(Value::as_object) else { return Ok(responses) };
    for (key, request) in requests {
        match request.get("method").and_then(Value::as_str).unwrap_or_default() {
            "roots/list" => {
                responses.insert(key.clone(), roots_result(root));
            }
            "sampling/createMessage" => return Err("an LLM completion (sampling)".into()),
            "elicitation/create" => return Err("user input (elicitation)".into()),
            other => return Err(format!("input of type '{other}'")),
        }
    }
    Ok(responses)
}

struct ConnectCtx {
    client_info: Value,
    root: Option<PathBuf>,
    label: String,
    deadline: Instant,
    timeout: Duration,
    ids: AtomicU64,
}

impl ConnectCtx {
    fn remaining(&self) -> Duration {
        remaining(self.deadline)
    }

    fn modern_meta(&self) -> Value {
        json!({
            META_PROTOCOL_VERSION: MODERN_VERSION,
            META_CLIENT_INFO: self.client_info,
            META_CLIENT_CAPABILITIES: modern_capabilities(self.root.as_deref()),
        })
    }

    fn initialize_params(&self, version: &str) -> Value {
        let capabilities = if self.root.is_some() {
            json!({ "roots": { "listChanged": false } })
        } else {
            json!({})
        };
        json!({
            "protocolVersion": version,
            "capabilities": capabilities,
            "clientInfo": self.client_info,
        })
    }

    fn error(&self, failure: Failure, method: &str, stderr: &str) -> Error {
        failure_error(failure, &self.label, method, self.timeout, stderr)
    }
}

fn modern_capabilities(root: Option<&Path>) -> Value {
    if root.is_some() {
        json!({ "roots": {} })
    } else {
        json!({})
    }
}

/// What connection setup settled on.
struct Negotiated {
    era: Era,
    version: String,
    server_info: Option<Value>,
    instructions: Option<String>,
}

impl Negotiated {
    fn modern(discover: &Value) -> Self {
        Self {
            era: Era::Modern,
            version: MODERN_VERSION.to_string(),
            server_info: discover
                .get("_meta")
                .and_then(|meta| meta.get(META_SERVER_INFO))
                .or_else(|| discover.get("serverInfo"))
                .cloned(),
            instructions: string_field(discover, "instructions"),
        }
    }

    fn legacy(init: &Value, requested: &str) -> Self {
        let version = init
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(requested)
            .to_string();
        if !LEGACY_VERSIONS.contains(&version.as_str()) {
            tracing::warn!(version = %version, "MCP server negotiated an unknown protocol version");
        }
        Self {
            era: Era::Legacy,
            version,
            server_info: init.get("serverInfo").cloned(),
            instructions: string_field(init, "instructions"),
        }
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}

/// A protocol version both sides speak.
#[derive(Debug, PartialEq)]
enum Pick {
    Modern,
    Legacy(String),
    Nothing,
}

fn pick_version<S: AsRef<str>>(supported: &[S]) -> Pick {
    if supported.iter().any(|v| v.as_ref() == MODERN_VERSION) {
        return Pick::Modern;
    }
    for version in LEGACY_VERSIONS {
        if supported.iter().any(|v| v.as_ref() == *version) {
            return Pick::Legacy(version.to_string());
        }
    }
    Pick::Nothing
}

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    value.and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

/// The version to use given a successful `server/discover`.
fn pick_from_discover(result: &Value) -> Pick {
    match string_list(result.get("supportedVersions")) {
        Some(list) if !list.is_empty() => pick_version(&list),
        // A server that answers discover but lists nothing speaks the modern era.
        _ => Pick::Modern,
    }
}

/// How a failed probe should be read.
#[derive(Debug, PartialEq)]
enum ProbeError {
    /// Not a modern server (method not found, no reply, HTTP 404...).
    NotModern,
    /// A modern error that lists the supported versions.
    Supported(Pick),
    /// A modern server that refused us for another reason.
    Fatal,
}

fn classify_probe_error(failure: &Failure) -> ProbeError {
    match failure.rpc() {
        Some(err) if is_modern_error(err) => match string_list(err.data.as_ref().and_then(|d| d.get("supported"))) {
            Some(list) => ProbeError::Supported(pick_version(&list)),
            None => ProbeError::Fatal,
        },
        _ => ProbeError::NotModern,
    }
}

/// Errors only a 2026-07-28 server produces (or that carry a supported-versions list).
fn is_modern_error(err: &RpcError) -> bool {
    (-32029..=-32020).contains(&err.code)
        || err
            .data
            .as_ref()
            .and_then(|data| data.get("supported"))
            .is_some_and(Value::is_array)
}

/// Sends a request and waits for its response, with a timeout.
async fn call(
    wire: &Wire,
    ids: &AtomicU64,
    method: &str,
    params: Value,
    extra: Vec<(String, String)>,
    timeout: Duration,
) -> std::result::Result<Value, Failure> {
    if timeout.is_zero() {
        return Err(Failure::Timeout);
    }
    let id = ids.fetch_add(1, Ordering::Relaxed);
    let message = request_message(id, method, params);
    match tokio::time::timeout(timeout, wire.request(message, id, method, extra)).await {
        Ok(result) => result,
        Err(_) => {
            wire.router().forget(id);
            Err(Failure::Timeout)
        }
    }
}

/// `initialize` + `notifications/initialized` on an already-open wire.
async fn legacy_handshake(ctx: &ConnectCtx, wire: &Wire, version: &str) -> Result<Negotiated> {
    let init = call(
        wire,
        &ctx.ids,
        "initialize",
        ctx.initialize_params(version),
        Vec::new(),
        ctx.remaining(),
    )
    .await
    .map_err(|failure| ctx.error(failure, "initialize", &wire.stderr_tail()))?;
    let negotiated = Negotiated::legacy(&init, version);
    if let Wire::Http(http) = wire {
        http.core.set_legacy_version(&negotiated.version);
    }
    wire.notify(notification_message("notifications/initialized"), "notifications/initialized")
        .await
        .map_err(|failure| ctx.error(failure, "notifications/initialized", &wire.stderr_tail()))?;
    if let Wire::Http(http) = wire {
        http.start_listener();
    }
    Ok(negotiated)
}

// ---------------------------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn from_value(value: &Value) -> Self {
        Self {
            code: value.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
            data: value.get("data").cloned(),
        }
    }
}

#[derive(Debug)]
enum Failure {
    /// A JSON-RPC error response.
    Rpc(RpcError),
    /// A non-2xx HTTP status, with the JSON-RPC error in its body if there was one.
    Status { status: u16, body: String, rpc: Option<RpcError> },
    Timeout,
    /// The server went away before answering.
    Closed,
    Other(Error),
}

impl Failure {
    fn rpc(&self) -> Option<&RpcError> {
        match self {
            Failure::Rpc(err) => Some(err),
            Failure::Status { rpc: Some(err), .. } => Some(err),
            _ => None,
        }
    }

    fn status(status: u16, body: String) -> Self {
        let rpc = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.get("error").map(RpcError::from_value));
        Failure::Status { status, body, rpc }
    }
}

impl From<reqwest::Error> for Failure {
    fn from(err: reqwest::Error) -> Self {
        Failure::Other(Error::msg(describe_reqwest(&err)))
    }
}

fn describe_reqwest(err: &reqwest::Error) -> String {
    let mut text = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

fn failure_error(failure: Failure, label: &str, method: &str, timeout: Duration, stderr: &str) -> Error {
    let (mut text, with_stderr) = match failure {
        Failure::Rpc(err) => (
            format!("MCP server {label}: {method} failed: {} (code {})", err.message, err.code),
            false,
        ),
        Failure::Status { status, body, rpc } => match rpc {
            Some(err) => (
                format!(
                    "MCP server {label}: {method} failed with HTTP {status}: {} (code {})",
                    err.message, err.code
                ),
                false,
            ),
            None => {
                let body = body.trim();
                let snippet: String = body.chars().take(300).collect();
                if snippet.is_empty() {
                    (format!("MCP server {label}: {method} failed with HTTP {status}"), false)
                } else {
                    (format!("MCP server {label}: {method} failed with HTTP {status}: {snippet}"), false)
                }
            }
        },
        Failure::Timeout => (
            format!(
                "MCP server {label}: {method} timed out after {:.1}s",
                timeout.as_secs_f64()
            ),
            true,
        ),
        Failure::Closed => (format!("MCP server {label} closed the connection during {method}"), true),
        Failure::Other(err) => (format!("MCP server {label}: {method}: {err}"), true),
    };
    if with_stderr && !stderr.is_empty() {
        text.push_str("\nServer stderr:\n");
        text.push_str(stderr);
    }
    Error::msg(text)
}

// ---------------------------------------------------------------------------------------------
// JSON-RPC routing
// ---------------------------------------------------------------------------------------------

fn request_message(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn notification_message(method: &str) -> Value {
    json!({ "jsonrpc": "2.0", "method": method })
}

fn response_result(message: Value) -> std::result::Result<Value, Failure> {
    if let Some(error) = message.get("error") {
        return Err(Failure::Rpc(RpcError::from_value(error)));
    }
    match message {
        Value::Object(mut object) => Ok(object.remove("result").unwrap_or(Value::Null)),
        _ => Err(Failure::Other(Error::msg("malformed JSON-RPC response"))),
    }
}

/// Merges the modern `_meta` envelope into request params.
fn with_meta(params: Value, meta: &Value) -> Value {
    let mut object = match params {
        Value::Object(object) => object,
        _ => Map::new(),
    };
    let entry = object.entry("_meta").or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if let (Some(target), Some(source)) = (entry.as_object_mut(), meta.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    Value::Object(object)
}

/// Matches responses to waiting requests and answers requests the server sends us.
struct Router {
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    closed: AtomicBool,
    root: Option<PathBuf>,
}

impl Router {
    fn new(root: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self { pending: Mutex::new(HashMap::new()), closed: AtomicBool::new(false), root })
    }

    fn register(&self, id: u64) -> oneshot::Receiver<Value> {
        let (tx, rx) = oneshot::channel();
        if !self.closed.load(Ordering::Acquire) {
            lock(&self.pending).insert(id, tx);
        }
        rx
    }

    fn forget(&self, id: u64) {
        lock(&self.pending).remove(&id);
    }

    /// Fails every waiting request (their receivers see the sender dropped).
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        lock(&self.pending).clear();
    }

    /// Handles a message (or batch) from the server. Returns the replies to send back.
    fn incoming(&self, message: Value) -> Vec<Value> {
        match message {
            Value::Array(items) => items.into_iter().filter_map(|item| self.incoming_one(item)).collect(),
            other => self.incoming_one(other).into_iter().collect(),
        }
    }

    fn incoming_one(&self, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").filter(|id| !id.is_null()).cloned();
        match (method, id) {
            (Some(method), Some(id)) => Some(answer_server_request(&method, id, self.root.as_deref())),
            (Some(method), None) => {
                let params = message.get("params").map(Value::to_string).unwrap_or_default();
                tracing::debug!(method = %method, params = %params, "MCP notification");
                None
            }
            (None, Some(id)) => {
                let waiter = id_number(&id).and_then(|id| lock(&self.pending).remove(&id));
                match waiter {
                    Some(tx) => {
                        let _ = tx.send(message);
                    }
                    None => tracing::debug!(id = %id, "MCP response for no pending request"),
                }
                None
            }
            (None, None) => {
                if let Some(error) = message.get("error") {
                    tracing::debug!(error = %error, "MCP error without an id");
                }
                None
            }
        }
    }
}

fn id_number(id: &Value) -> Option<u64> {
    id.as_u64().or_else(|| id.as_str().and_then(|text| text.parse().ok()))
}

/// Our reply to a request the server sent us (legacy era).
fn answer_server_request(method: &str, id: Value, root: Option<&Path>) -> Value {
    match method {
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "roots/list" => json!({ "jsonrpc": "2.0", "id": id, "result": roots_result(root) }),
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {other}") },
        }),
    }
}

fn roots_result(root: Option<&Path>) -> Value {
    match root {
        Some(root) => {
            let name = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.to_string_lossy().into_owned());
            json!({ "roots": [{ "uri": file_uri(root), "name": name }] })
        }
        None => json!({ "roots": [] }),
    }
}

/// `file:///C:/Users/x` style URI with percent-encoding.
fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let raw = raw.strip_prefix("//?/").unwrap_or(&raw);
    let mut uri = String::from("file://");
    if !raw.starts_with('/') {
        uri.push('/');
    }
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

// ---------------------------------------------------------------------------------------------
// Wires
// ---------------------------------------------------------------------------------------------

enum Wire {
    Stdio(StdioWire),
    Http(HttpWire),
    Sse(SseWire),
}

impl Wire {
    fn router(&self) -> &Arc<Router> {
        match self {
            Wire::Stdio(wire) => &wire.router,
            Wire::Http(wire) => &wire.core.router,
            Wire::Sse(wire) => &wire.router,
        }
    }

    async fn request(
        &self,
        message: Value,
        id: u64,
        method: &str,
        extra: Vec<(String, String)>,
    ) -> std::result::Result<Value, Failure> {
        match self {
            Wire::Stdio(wire) => wire.request(&message, id).await,
            Wire::Http(wire) => wire.core.request(&message, id, method, &extra).await,
            Wire::Sse(wire) => wire.request(&message, id).await,
        }
    }

    async fn notify(&self, message: Value, method: &str) -> std::result::Result<(), Failure> {
        match self {
            Wire::Stdio(wire) => wire.send(&message),
            Wire::Http(wire) => wire.core.notify(&message, method).await,
            Wire::Sse(wire) => wire.post(&message).await,
        }
    }

    fn stderr_tail(&self) -> String {
        match self {
            Wire::Stdio(wire) => wire.stderr_tail(),
            _ => String::new(),
        }
    }

    async fn close(&self) {
        match self {
            Wire::Stdio(wire) => wire.close().await,
            Wire::Http(wire) => wire.close().await,
            Wire::Sse(wire) => wire.close(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

// ----- stdio ---------------------------------------------------------------------------------

struct StdioWire {
    /// Lines to write to the server's stdin. Dropping it closes stdin.
    tx: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    child: Mutex<Option<tokio::process::Child>>,
    pid: Option<u32>,
    router: Arc<Router>,
    stderr: Arc<Mutex<Vec<u8>>>,
    tasks: Vec<JoinHandle<()>>,
}

impl StdioWire {
    fn spawn(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&Path>,
        root: Option<PathBuf>,
    ) -> Result<Self> {
        let (program, argv) = resolve_command(command, args, env);
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&argv)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(dir) = cwd.or(root.as_deref()) {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd
            .spawn()
            .map_err(|err| Error::msg(format!("could not start MCP server `{command}`: {err}")))?;
        #[cfg(windows)]
        crate::browser::job::adopt(&child);
        let pid = child.id();
        let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(Error::msg(format!("could not open the pipes of MCP server `{command}`")));
        };

        let router = Router::new(root);
        let tail = Arc::new(Mutex::new(Vec::new()));
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

        let writer = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if stdin.write_all(&line).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
            // Dropping stdin here is what tells the server to shut down.
        });

        let reader = {
            let router = router.clone();
            let replies = tx.downgrade();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match lines.read_until(b'\n', &mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let text = line.trim_ascii();
                    if text.is_empty() {
                        continue;
                    }
                    let message = match serde_json::from_slice::<Value>(text) {
                        Ok(message) => message,
                        Err(_) => {
                            tracing::debug!(line = %String::from_utf8_lossy(text), "non-JSON line on MCP server stdout");
                            continue;
                        }
                    };
                    for reply in router.incoming(message) {
                        if let (Some(tx), Ok(mut bytes)) = (replies.upgrade(), serde_json::to_vec(&reply)) {
                            bytes.push(b'\n');
                            let _ = tx.send(bytes);
                        }
                    }
                }
                router.close();
            })
        };

        let stderr_task = {
            let tail = tail.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            tracing::debug!(target: "serika_runtime::mcp::stderr", "{}", String::from_utf8_lossy(&buf[..n]).trim_end());
                            let mut tail = lock(&tail);
                            tail.extend_from_slice(&buf[..n]);
                            if tail.len() > STDERR_TAIL {
                                let excess = tail.len() - STDERR_TAIL;
                                tail.drain(..excess);
                            }
                        }
                    }
                }
            })
        };

        Ok(Self {
            tx: Mutex::new(Some(tx)),
            child: Mutex::new(Some(child)),
            pid,
            router,
            stderr: tail,
            tasks: vec![writer, reader, stderr_task],
        })
    }

    fn send(&self, message: &Value) -> std::result::Result<(), Failure> {
        let mut bytes = serde_json::to_vec(message).map_err(|err| Failure::Other(err.into()))?;
        bytes.push(b'\n');
        match lock(&self.tx).as_ref() {
            Some(tx) => tx.send(bytes).map_err(|_| Failure::Closed),
            None => Err(Failure::Closed),
        }
    }

    /// Registers `id` and writes the message, without waiting.
    fn start(&self, message: &Value, id: u64) -> std::result::Result<oneshot::Receiver<Value>, Failure> {
        let rx = self.router.register(id);
        if let Err(failure) = self.send(message) {
            self.router.forget(id);
            return Err(failure);
        }
        Ok(rx)
    }

    async fn request(&self, message: &Value, id: u64) -> std::result::Result<Value, Failure> {
        let rx = self.start(message, id)?;
        match rx.await {
            Ok(response) => response_result(response),
            Err(_) => Err(Failure::Closed),
        }
    }

    fn stderr_tail(&self) -> String {
        let tail = lock(&self.stderr);
        // Skip a UTF-8 sequence cut in half by the trimming.
        let start = tail.iter().position(|b| b & 0xC0 != 0x80).unwrap_or(tail.len());
        String::from_utf8_lossy(&tail[start..]).trim().to_string()
    }

    async fn close(&self) {
        lock(&self.tx).take();
        let child = lock(&self.child).take();
        if let Some(mut child) = child {
            if tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await.is_err() {
                kill_tree(self.pid).await;
                let _ = child.kill().await;
            }
        }
        for task in &self.tasks {
            task.abort();
        }
        self.router.close();
    }
}

impl Drop for StdioWire {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(mut child) = lock(&self.child).take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                kill_tree_detached(self.pid);
            }
            // `kill_on_drop` takes care of the direct child.
        }
    }
}

/// Kills a process and its children (a `.cmd` shim's node process, for one).
async fn kill_tree(pid: Option<u32>) {
    #[cfg(windows)]
    if let Some(pid) = pid {
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let _ = tokio::time::timeout(Duration::from_secs(5), cmd.status()).await;
    }
    #[cfg(not(windows))]
    let _ = pid;
}

fn kill_tree_detached(pid: Option<u32>) {
    #[cfg(windows)]
    if let Some(pid) = pid {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = pid;
}

/// Resolves a stdio server command. On Windows, extension-less commands are looked up on PATH
/// (`.exe`, `.cmd`, `.bat`); batch shims such as `npx.cmd` run through `cmd.exe` (Rust's
/// standard library does that, with batch-safe argument escaping, when the program is a
/// `.cmd`/`.bat` file), and unknown commands go through `cmd /C` so its own lookup applies.
fn resolve_command(command: &str, args: &[String], env: &BTreeMap<String, String>) -> (std::ffi::OsString, Vec<String>) {
    #[cfg(windows)]
    {
        let path = Path::new(command);
        if path.extension().is_some() {
            return (command.into(), args.to_vec());
        }
        let has_dir = command.contains(['/', '\\']);
        let dirs: Vec<PathBuf> = if has_dir {
            vec![PathBuf::new()]
        } else {
            let search = env
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| std::ffi::OsString::from(value))
                .or_else(|| std::env::var_os("PATH"))
                .unwrap_or_default();
            std::env::split_paths(&search).collect()
        };
        for dir in dirs {
            for ext in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{command}.{ext}"));
                if candidate.is_file() {
                    return (candidate.into_os_string(), args.to_vec());
                }
            }
        }
        let mut argv = vec!["/C".to_string(), command.to_string()];
        argv.extend(args.iter().cloned());
        ("cmd".into(), argv)
    }
    #[cfg(not(windows))]
    {
        let _ = env;
        (command.into(), args.to_vec())
    }
}

/// Spawns a stdio server and negotiates. A server that dies on the modern probe is started
/// again and spoken to with `initialize` only.
async fn connect_stdio(
    ctx: &ConnectCtx,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: Option<&Path>,
    probe_wait: Duration,
) -> Result<(Wire, Negotiated)> {
    let mut probe = true;
    loop {
        let wire = StdioWire::spawn(command, args, env, cwd, ctx.root.clone())?;
        match negotiate_stdio(ctx, &wire, probe, probe_wait).await {
            Ok(negotiated) => return Ok((Wire::Stdio(wire), negotiated)),
            Err(StdioSetupError::DiedOnProbe) if probe => {
                tracing::debug!(server = %ctx.label, "MCP server exited on the modern probe; retrying with initialize");
                wire.close().await;
                probe = false;
            }
            Err(StdioSetupError::DiedOnProbe) => {
                let tail = wire.stderr_tail();
                wire.close().await;
                return Err(ctx.error(Failure::Closed, "initialize", &tail));
            }
            Err(StdioSetupError::Failed(failure, method)) => {
                // Give the process a moment so its last stderr lines make it into the message.
                tokio::time::sleep(Duration::from_millis(150)).await;
                let tail = wire.stderr_tail();
                wire.close().await;
                return Err(ctx.error(failure, method, &tail));
            }
        }
    }
}

enum StdioSetupError {
    DiedOnProbe,
    Failed(Failure, &'static str),
}

enum StdioEvent {
    Discover(Option<Value>),
    Init(Option<Value>),
}

/// Modern probe raced against a delayed `initialize`: `server/discover` goes first; if no
/// recognisable answer comes within `probe_wait`, `initialize` is sent as well and whichever
/// era answers properly wins. A slow-starting modern server still reads discover first.
async fn negotiate_stdio(
    ctx: &ConnectCtx,
    wire: &StdioWire,
    probe: bool,
    probe_wait: Duration,
) -> std::result::Result<Negotiated, StdioSetupError> {
    let (events_tx, mut events) = mpsc::unbounded_channel::<StdioEvent>();
    let forward = |rx: oneshot::Receiver<Value>, init: bool| {
        let events_tx = events_tx.clone();
        tokio::spawn(async move {
            let response = rx.await.ok();
            let _ = events_tx.send(if init { StdioEvent::Init(response) } else { StdioEvent::Discover(response) });
        });
    };
    let send_discover = || -> std::result::Result<(), StdioSetupError> {
        let id = ctx.ids.fetch_add(1, Ordering::Relaxed);
        let message = request_message(id, "server/discover", json!({ "_meta": ctx.modern_meta() }));
        let rx = wire.start(&message, id).map_err(|_| StdioSetupError::DiedOnProbe)?;
        forward(rx, false);
        Ok(())
    };
    let send_init = |version: &str| -> std::result::Result<(), StdioSetupError> {
        let id = ctx.ids.fetch_add(1, Ordering::Relaxed);
        let message = request_message(id, "initialize", ctx.initialize_params(version));
        let rx = wire.start(&message, id).map_err(|f| StdioSetupError::Failed(f, "initialize"))?;
        forward(rx, true);
        Ok(())
    };

    let mut init_version: Option<String> = None;
    let mut init_failure: Option<Failure> = None;
    let mut discover_pending = false;
    let mut discover_retried = false;
    if probe {
        send_discover()?;
        discover_pending = true;
    } else {
        send_init(LEGACY_VERSIONS[0])?;
        init_version = Some(LEGACY_VERSIONS[0].to_string());
    }
    let probe_at = Instant::now() + probe_wait;

    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { return Err(StdioSetupError::Failed(Failure::Closed, "initialize")) };
                match event {
                    StdioEvent::Discover(None) => {
                        if init_version.is_none() {
                            return Err(StdioSetupError::DiedOnProbe);
                        }
                        return Err(StdioSetupError::Failed(Failure::Closed, "initialize"));
                    }
                    StdioEvent::Discover(Some(response)) => {
                        discover_pending = false;
                        let legacy = match response_result(response) {
                            Ok(result) => match pick_from_discover(&result) {
                                Pick::Modern => return Ok(Negotiated::modern(&result)),
                                Pick::Legacy(version) => Some(version),
                                Pick::Nothing => {
                                    let offered = string_list(result.get("supportedVersions")).unwrap_or_default();
                                    return Err(StdioSetupError::Failed(no_common_version(&offered), "server/discover"));
                                }
                            },
                            Err(failure) => match classify_probe_error(&failure) {
                                ProbeError::NotModern => {
                                    if let Some(failure) = init_failure.take() {
                                        return Err(StdioSetupError::Failed(failure, "initialize"));
                                    }
                                    Some(LEGACY_VERSIONS[0].to_string())
                                }
                                ProbeError::Supported(Pick::Modern) if !discover_retried => {
                                    discover_retried = true;
                                    discover_pending = true;
                                    send_discover()?;
                                    None
                                }
                                ProbeError::Supported(Pick::Legacy(version)) => Some(version),
                                _ => return Err(StdioSetupError::Failed(failure, "server/discover")),
                            },
                        };
                        if let Some(version) = legacy {
                            if init_version.is_none() {
                                send_init(&version)?;
                                init_version = Some(version);
                            } else if let Some(failure) = init_failure.take() {
                                return Err(StdioSetupError::Failed(failure, "initialize"));
                            }
                        }
                    }
                    StdioEvent::Init(None) => return Err(StdioSetupError::Failed(Failure::Closed, "initialize")),
                    StdioEvent::Init(Some(response)) => match response_result(response) {
                        Ok(result) => {
                            let requested = init_version.as_deref().unwrap_or(LEGACY_VERSIONS[0]);
                            let negotiated = Negotiated::legacy(&result, requested);
                            wire.send(&notification_message("notifications/initialized"))
                                .map_err(|f| StdioSetupError::Failed(f, "notifications/initialized"))?;
                            return Ok(negotiated);
                        }
                        Err(failure) => {
                            if discover_pending {
                                init_failure = Some(failure);
                            } else {
                                return Err(StdioSetupError::Failed(failure, "initialize"));
                            }
                        }
                    },
                }
            }
            _ = tokio::time::sleep_until(probe_at.into()), if init_version.is_none() => {
                tracing::debug!(server = %ctx.label, "no answer to server/discover yet; sending initialize too");
                send_init(LEGACY_VERSIONS[0])?;
                init_version = Some(LEGACY_VERSIONS[0].to_string());
            }
            _ = tokio::time::sleep_until(ctx.deadline.into()) => {
                let method = if init_version.is_some() { "initialize" } else { "server/discover" };
                return Err(StdioSetupError::Failed(Failure::Timeout, method));
            }
        }
    }
}

fn no_common_version(offered: &[String]) -> Failure {
    Failure::Other(Error::msg(format!(
        "no protocol version in common (server supports {}; Serika supports {MODERN_VERSION}, {})",
        if offered.is_empty() { "none".to_string() } else { offered.join(", ") },
        LEGACY_VERSIONS.join(", ")
    )))
}

// ----- Streamable HTTP -----------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct HttpState {
    modern: bool,
    /// `MCP-Protocol-Version` for legacy sessions (2025-06-18 and later).
    version_header: Option<String>,
    session: Option<String>,
}

struct HttpCore {
    client: reqwest::Client,
    url: String,
    headers: BTreeMap<String, String>,
    state: Mutex<HttpState>,
    router: Arc<Router>,
}

struct HttpWire {
    core: Arc<HttpCore>,
    listener: Mutex<Option<JoinHandle<()>>>,
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| Error::msg(format!("could not create the HTTP client: {}", describe_reqwest(&err))))
}

fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

impl HttpCore {
    fn set_legacy_version(&self, version: &str) {
        let mut state = lock(&self.state);
        state.modern = false;
        state.version_header = (version >= "2025-06-18").then(|| version.to_string());
    }

    fn with_headers(&self, mut builder: reqwest::RequestBuilder, method: Option<&str>, extra: &[(String, String)]) -> reqwest::RequestBuilder {
        let state = lock(&self.state).clone();
        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        if state.modern {
            builder = builder.header("MCP-Protocol-Version", MODERN_VERSION);
            if let Some(method) = method {
                builder = builder.header("Mcp-Method", encode_header_value(method));
            }
        } else {
            if let Some(version) = &state.version_header {
                builder = builder.header("MCP-Protocol-Version", version.as_str());
            }
            if let Some(session) = &state.session {
                builder = builder.header("Mcp-Session-Id", session.as_str());
            }
        }
        for (key, value) in extra {
            builder = builder.header(key.as_str(), value.as_str());
        }
        builder
    }

    fn post(&self, body: &Value, method: Option<&str>, extra: &[(String, String)]) -> reqwest::RequestBuilder {
        let builder = self
            .client
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
            .body(serde_json::to_vec(body).unwrap_or_default());
        self.with_headers(builder, method, extra)
    }

    fn capture_session(&self, response: &reqwest::Response) {
        let mut state = lock(&self.state);
        if state.modern {
            return;
        }
        if let Some(session) = response.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            if !session.is_empty() {
                state.session = Some(session.to_string());
            }
        }
    }

    async fn request(
        self: &Arc<Self>,
        message: &Value,
        id: u64,
        method: &str,
        extra: &[(String, String)],
    ) -> std::result::Result<Value, Failure> {
        let rx = self.router.register(id);
        let result = self.exchange(message, method, extra, rx).await;
        if result.is_err() {
            self.router.forget(id);
        }
        result
    }

    async fn exchange(
        self: &Arc<Self>,
        message: &Value,
        method: &str,
        extra: &[(String, String)],
        mut rx: oneshot::Receiver<Value>,
    ) -> std::result::Result<Value, Failure> {
        let response = self.post(message, Some(method), extra).send().await?;
        self.capture_session(&response);
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Failure::status(status.as_u16(), body));
        }
        if is_event_stream(&response) {
            let mut stream = response.bytes_stream();
            let mut parser = SseParser::default();
            loop {
                tokio::select! {
                    biased;
                    answer = &mut rx => {
                        return answer.map_err(|_| Failure::Closed).and_then(response_result);
                    }
                    chunk = stream.next() => match chunk {
                        Some(Ok(bytes)) => {
                            for event in parser.feed(&bytes) {
                                self.deliver_event(&event);
                            }
                        }
                        Some(Err(err)) => return Err(err.into()),
                        None => {
                            for event in parser.finish() {
                                self.deliver_event(&event);
                            }
                            break;
                        }
                    }
                }
            }
            return match rx.try_recv() {
                Ok(answer) => response_result(answer),
                Err(_) => Err(Failure::Other(Error::msg("the server ended its event stream without answering"))),
            };
        }
        let bytes = response.bytes().await?;
        if !bytes.trim_ascii().is_empty() {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|err| Failure::Other(Error::msg(format!("the server sent invalid JSON: {err}"))))?;
            self.deliver(value);
        }
        // Usually already answered by the body; a legacy server may answer on its GET stream.
        rx.await.map_err(|_| Failure::Closed).and_then(response_result)
    }

    fn deliver_event(self: &Arc<Self>, event: &SseEvent) {
        if event.event != "message" {
            return;
        }
        match serde_json::from_str::<Value>(&event.data) {
            Ok(value) => self.deliver(value),
            Err(_) => tracing::debug!(data = %event.data, "non-JSON SSE event from MCP server"),
        }
    }

    fn deliver(self: &Arc<Self>, message: Value) {
        let replies = self.router.incoming(message);
        if replies.is_empty() || lock(&self.state).modern {
            return;
        }
        for reply in replies {
            let request = self.post(&reply, None, &[]);
            tokio::spawn(async move {
                if let Err(err) = request.send().await {
                    tracing::debug!("could not answer an MCP server request: {}", describe_reqwest(&err));
                }
            });
        }
    }

    async fn notify(&self, message: &Value, method: &str) -> std::result::Result<(), Failure> {
        let response = self.post(message, Some(method), &[]).send().await?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(Failure::status(status.as_u16(), body))
    }

    /// The legacy standalone GET stream, for requests and notifications the server sends on its
    /// own. Servers without one answer 405, which is fine.
    async fn listen(self: Arc<Self>) {
        let builder = self.client.get(&self.url).header(reqwest::header::ACCEPT, "text/event-stream");
        let response = match self.with_headers(builder, None, &[]).send().await {
            Ok(response) if response.status().is_success() && is_event_stream(&response) => response,
            Ok(response) => {
                tracing::debug!(status = response.status().as_u16(), "MCP server has no standalone event stream");
                return;
            }
            Err(err) => {
                tracing::debug!("MCP event stream failed: {}", describe_reqwest(&err));
                return;
            }
        };
        let mut stream = response.bytes_stream();
        let mut parser = SseParser::default();
        while let Some(Ok(chunk)) = stream.next().await {
            for event in parser.feed(&chunk) {
                self.deliver_event(&event);
            }
        }
    }
}

impl HttpWire {
    fn new(client: reqwest::Client, url: &str, headers: &BTreeMap<String, String>, root: Option<PathBuf>) -> Self {
        Self {
            core: Arc::new(HttpCore {
                client,
                url: url.to_string(),
                headers: headers.clone(),
                state: Mutex::new(HttpState { modern: true, ..HttpState::default() }),
                router: Router::new(root),
            }),
            listener: Mutex::new(None),
        }
    }

    fn start_listener(&self) {
        let core = self.core.clone();
        let task = tokio::spawn(core.listen());
        if let Some(previous) = lock(&self.listener).replace(task) {
            previous.abort();
        }
    }

    async fn close(&self) {
        if let Some(task) = lock(&self.listener).take() {
            task.abort();
        }
        let state = lock(&self.core.state).clone();
        if let (false, Some(_)) = (state.modern, &state.session) {
            let request = self.core.with_headers(self.core.client.delete(&self.core.url), None, &[]);
            let _ = tokio::time::timeout(Duration::from_secs(3), request.send()).await;
        }
        self.core.router.close();
    }
}

impl Drop for HttpWire {
    fn drop(&mut self) {
        if let Some(task) = lock(&self.listener).take() {
            task.abort();
        }
    }
}

async fn connect_http(ctx: &ConnectCtx, url: &str, headers: &BTreeMap<String, String>) -> Result<(Wire, Negotiated)> {
    let client = http_client()?;
    let wire = Wire::Http(HttpWire::new(client.clone(), url, headers, ctx.root.clone()));
    let mut legacy_version = LEGACY_VERSIONS[0].to_string();
    let mut retried = false;
    loop {
        let probe = call(
            &wire,
            &ctx.ids,
            "server/discover",
            json!({ "_meta": ctx.modern_meta() }),
            Vec::new(),
            ctx.remaining(),
        )
        .await;
        match probe {
            Ok(result) => match pick_from_discover(&result) {
                Pick::Modern => return Ok((wire, Negotiated::modern(&result))),
                Pick::Legacy(version) => legacy_version = version,
                Pick::Nothing => {
                    let offered = string_list(result.get("supportedVersions")).unwrap_or_default();
                    return Err(ctx.error(no_common_version(&offered), "server/discover", ""));
                }
            },
            Err(failure) => match classify_probe_error(&failure) {
                ProbeError::Supported(Pick::Modern) if !retried => {
                    retried = true;
                    continue;
                }
                ProbeError::Supported(Pick::Legacy(version)) => legacy_version = version,
                ProbeError::NotModern => match &failure {
                    Failure::Status { status: 401 | 403, .. } => {
                        return Err(ctx.error(failure, "server/discover", ""));
                    }
                    Failure::Status { .. } | Failure::Rpc(_) => {}
                    _ => return Err(ctx.error(failure, "server/discover", "")),
                },
                _ => return Err(ctx.error(failure, "server/discover", "")),
            },
        }
        break;
    }

    // Legacy Streamable HTTP with sessions.
    tracing::debug!(url, "MCP server is not 2026-07-28; trying a legacy session");
    if let Wire::Http(http) = &wire {
        lock(&http.core.state).modern = false;
    }
    let init = call(
        &wire,
        &ctx.ids,
        "initialize",
        ctx.initialize_params(&legacy_version),
        Vec::new(),
        ctx.remaining(),
    )
    .await;
    match init {
        Ok(result) => {
            let negotiated = Negotiated::legacy(&result, &legacy_version);
            if let Wire::Http(http) = &wire {
                http.core.set_legacy_version(&negotiated.version);
            }
            wire.notify(notification_message("notifications/initialized"), "notifications/initialized")
                .await
                .map_err(|failure| ctx.error(failure, "notifications/initialized", ""))?;
            if let Wire::Http(http) = &wire {
                http.start_listener();
            }
            Ok((wire, negotiated))
        }
        Err(Failure::Status { status, .. }) if (400..500).contains(&status) && status != 401 && status != 403 => {
            // The deprecated HTTP+SSE transport.
            tracing::debug!(url, status, "legacy initialize refused; trying HTTP+SSE");
            let sse = connect_sse(&client, url, headers, ctx.root.clone(), ctx.remaining())
                .await
                .map_err(|failure| ctx.error(failure, "connect (HTTP+SSE fallback)", ""))?;
            let wire = Wire::Sse(sse);
            let negotiated = legacy_handshake(ctx, &wire, LEGACY_VERSIONS[0]).await?;
            Ok((wire, negotiated))
        }
        Err(failure) => Err(ctx.error(failure, "initialize", "")),
    }
}

// ----- HTTP+SSE (2024-11-05) -----------------------------------------------------------------

struct SseWire {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    headers: BTreeMap<String, String>,
    router: Arc<Router>,
    reader: JoinHandle<()>,
}

async fn connect_sse(
    client: &reqwest::Client,
    url: &str,
    headers: &BTreeMap<String, String>,
    root: Option<PathBuf>,
    timeout: Duration,
) -> std::result::Result<SseWire, Failure> {
    let deadline = Instant::now() + timeout;
    let mut builder = client.get(url).header(reqwest::header::ACCEPT, "text/event-stream");
    for (key, value) in headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    let response = tokio::time::timeout(timeout, builder.send())
        .await
        .map_err(|_| Failure::Timeout)??;
    let status = response.status();
    if !status.is_success() {
        let body = tokio::time::timeout(remaining(deadline), response.text())
            .await
            .ok()
            .and_then(|body| body.ok())
            .unwrap_or_default();
        return Err(Failure::status(status.as_u16(), body));
    }
    if !is_event_stream(&response) {
        return Err(Failure::Other(Error::msg("the server did not answer with an event stream")));
    }
    let base = response.url().clone();
    let router = Router::new(root);
    let (endpoint_tx, endpoint_rx) = oneshot::channel::<std::result::Result<reqwest::Url, String>>();
    let reader = {
        let router = router.clone();
        let client = client.clone();
        let headers = headers.clone();
        tokio::spawn(async move {
            let mut endpoint_tx = Some(endpoint_tx);
            let mut endpoint: Option<reqwest::Url> = None;
            let mut stream = response.bytes_stream();
            let mut parser = SseParser::default();
            while let Some(Ok(chunk)) = stream.next().await {
                for event in parser.feed(&chunk) {
                    match event.event.as_str() {
                        "endpoint" => {
                            let resolved = base
                                .join(event.data.trim())
                                .map_err(|err| format!("invalid endpoint '{}': {err}", event.data.trim()))
                                .and_then(|url| {
                                    if url.origin() == base.origin() {
                                        Ok(url)
                                    } else {
                                        Err(format!("the endpoint {url} is not on the server's origin"))
                                    }
                                });
                            if let Ok(url) = &resolved {
                                endpoint = Some(url.clone());
                            }
                            if let Some(tx) = endpoint_tx.take() {
                                let _ = tx.send(resolved);
                            }
                        }
                        "message" => {
                            let Ok(message) = serde_json::from_str::<Value>(&event.data) else {
                                tracing::debug!(data = %event.data, "non-JSON SSE message from MCP server");
                                continue;
                            };
                            for reply in router.incoming(message) {
                                let Some(endpoint) = endpoint.clone() else { continue };
                                let request = sse_post(&client, endpoint, &headers, &reply);
                                tokio::spawn(async move {
                                    let _ = request.send().await;
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            router.close();
        })
    };
    let endpoint = match tokio::time::timeout(remaining(deadline), endpoint_rx).await {
        Ok(Ok(Ok(endpoint))) => endpoint,
        Ok(Ok(Err(reason))) => {
            reader.abort();
            return Err(Failure::Other(Error::msg(reason)));
        }
        Ok(Err(_)) => {
            reader.abort();
            return Err(Failure::Other(Error::msg("the event stream closed before naming its endpoint")));
        }
        Err(_) => {
            reader.abort();
            return Err(Failure::Timeout);
        }
    };
    Ok(SseWire { client: client.clone(), endpoint, headers: headers.clone(), router, reader })
}

fn sse_post(client: &reqwest::Client, endpoint: reqwest::Url, headers: &BTreeMap<String, String>, body: &Value) -> reqwest::RequestBuilder {
    let mut builder = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(body).unwrap_or_default());
    for (key, value) in headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    builder
}

impl SseWire {
    async fn post(&self, message: &Value) -> std::result::Result<(), Failure> {
        let response = sse_post(&self.client, self.endpoint.clone(), &self.headers, message).send().await?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(Failure::status(status.as_u16(), body))
    }

    async fn request(&self, message: &Value, id: u64) -> std::result::Result<Value, Failure> {
        let rx = self.router.register(id);
        if let Err(failure) = self.post(message).await {
            self.router.forget(id);
            return Err(failure);
        }
        match rx.await {
            Ok(response) => response_result(response),
            Err(_) => Err(Failure::Closed),
        }
    }

    fn close(&self) {
        self.reader.abort();
        self.router.close();
    }
}

impl Drop for SseWire {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------------------------

/// Header value encoding: plain visible ASCII passes through; anything else (or a value that
/// itself looks like the sentinel) becomes `=?base64?<base64 of UTF-8>?=`.
fn encode_header_value(value: &str) -> String {
    let visible = value.bytes().all(|b| (0x20..=0x7e).contains(&b));
    let padded = value.starts_with(' ') || value.ends_with(' ');
    let sentinel_like = value.len() >= BASE64_SENTINEL_PREFIX.len() + BASE64_SENTINEL_SUFFIX.len()
        && value
            .get(..BASE64_SENTINEL_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(BASE64_SENTINEL_PREFIX))
        && value.ends_with(BASE64_SENTINEL_SUFFIX);
    if visible && !padded && !sentinel_like {
        return value.to_string();
    }
    format!(
        "{BASE64_SENTINEL_PREFIX}{}{BASE64_SENTINEL_SUFFIX}",
        base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
    )
}

/// RFC 9110 token characters.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// `x-mcp-header` declarations reachable from the schema root through `properties` keys only.
fn header_params(schema: &Value) -> Vec<HeaderParam> {
    fn walk(node: &Value, path: &mut Vec<String>, out: &mut Vec<HeaderParam>) {
        let Some(properties) = node.get("properties").and_then(Value::as_object) else { return };
        for (key, property) in properties {
            path.push(key.clone());
            if let Some(name) = property.get("x-mcp-header").and_then(Value::as_str) {
                if is_token(name) && !out.iter().any(|(_, seen)| seen.eq_ignore_ascii_case(name)) {
                    out.push((path.clone(), name.to_string()));
                }
            }
            walk(property, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(schema, &mut Vec::new(), &mut out);
    out
}

/// `Mcp-Param-*` headers for the arguments of a call.
fn param_headers(declared: &[HeaderParam], arguments: &Value) -> Vec<(String, String)> {
    declared
        .iter()
        .filter_map(|(path, name)| {
            let mut node = arguments;
            for key in path {
                node = node.get(key)?;
            }
            let text = match node {
                Value::String(text) => text.clone(),
                Value::Bool(flag) => flag.to_string(),
                Value::Number(number) => number.to_string(),
                _ => return None,
            };
            Some((format!("Mcp-Param-{name}"), encode_header_value(&text)))
        })
        .collect()
}

/// Flattens a `tools/call` result into text and images.
fn flatten_tool_result(result: &Value) -> ToolResult {
    let mut parts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    let content = result.get("content").and_then(Value::as_array);
    for item in content.into_iter().flatten() {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
        let text_of = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match kind {
            "text" => parts.push(text_of("text")),
            "image" => {
                let mime = item.get("mimeType").and_then(Value::as_str).unwrap_or("image/png");
                images.push((mime.to_string(), text_of("data")));
            }
            "audio" => {
                let mime = item.get("mimeType").and_then(Value::as_str).unwrap_or("audio");
                parts.push(format!("[audio: {mime}]"));
            }
            "resource" => {
                let resource = item.get("resource").unwrap_or(&Value::Null);
                let uri = resource.get("uri").and_then(Value::as_str).unwrap_or_default();
                match resource.get("text").and_then(Value::as_str) {
                    Some(text) => parts.push(text.to_string()),
                    None => {
                        let mime = resource.get("mimeType").and_then(Value::as_str).unwrap_or_default();
                        match resource.get("blob").and_then(Value::as_str) {
                            Some(blob) if mime.starts_with("image/") => images.push((mime.to_string(), blob.to_string())),
                            _ => parts.push(format!("[resource {uri}]")),
                        }
                    }
                }
            }
            "resource_link" => {
                let name = text_of("name");
                let uri = text_of("uri");
                parts.push(format!("[link: {name} {uri}]"));
            }
            other => parts.push(format!("[unsupported content: {other}]")),
        }
    }
    let mut text = parts.join("\n");
    if content.map_or(true, |items| items.is_empty()) {
        if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null()) {
            text = serde_json::to_string_pretty(structured).unwrap_or_default();
        }
    }
    ToolResult {
        text,
        is_error: result.get("isError").and_then(Value::as_bool).unwrap_or(false),
        images,
    }
}

// ----- Server-Sent Events --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct SseEvent {
    event: String,
    data: String,
}

/// Incremental `text/event-stream` parser.
#[derive(Debug, Default)]
struct SseParser {
    buffer: Vec<u8>,
    event: String,
    data: Vec<String>,
}

impl SseParser {
    fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.buffer[start..].iter().position(|b| *b == b'\n') {
            let end = start + offset;
            let mut line = &self.buffer[start..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            let line = String::from_utf8_lossy(line).into_owned();
            self.line(&line, &mut events);
            start = end + 1;
        }
        self.buffer.drain(..start);
        events
    }

    /// Flushes whatever is left when the stream ends.
    fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_string();
            self.line(&line, &mut events);
        }
        self.line("", &mut events);
        events
    }

    fn line(&mut self, line: &str, events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                let event = if self.event.is_empty() { "message".to_string() } else { std::mem::take(&mut self.event) };
                events.push(SseEvent { event, data: self.data.join("\n") });
            }
            self.event.clear();
            self.data.clear();
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.find(':') {
            Some(index) => {
                let value = &line[index + 1..];
                (&line[..index], value.strip_prefix(' ').unwrap_or(value))
            }
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => self.data.push(value.to_string()),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_values_pass_through_or_get_base64() {
        assert_eq!(encode_header_value("eu-west"), "eu-west");
        assert_eq!(encode_header_value("tools/call"), "tools/call");
        assert_eq!(encode_header_value(""), "");
        assert_eq!(encode_header_value("a b"), "a b");
        // Leading / trailing whitespace.
        assert_eq!(encode_header_value(" x"), "=?base64?IHg=?=");
        assert_eq!(encode_header_value("x "), "=?base64?eCA=?=");
        // Non-ASCII and control characters.
        assert_eq!(encode_header_value("café"), "=?base64?Y2Fmw6k=?=");
        assert_eq!(encode_header_value("a\tb"), "=?base64?YQli?=");
        assert_eq!(encode_header_value("日本"), "=?base64?5pel5pys?=");
        // A value that looks like the sentinel is itself encoded.
        let tricky = "=?base64?SGk=?=";
        let encoded = encode_header_value(tricky);
        assert_ne!(encoded, tricky);
        let payload = encoded.strip_prefix("=?base64?").unwrap().strip_suffix("?=").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(payload).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), tricky);
        assert_ne!(encode_header_value("=?BASE64?SGk=?="), "=?BASE64?SGk=?=");
    }

    #[test]
    fn x_mcp_header_only_through_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "region": { "type": "string", "x-mcp-header": "Region" },
                "repeat": { "type": "integer" },
                "nested": {
                    "type": "object",
                    "properties": { "tenant": { "type": "string", "x-mcp-header": "Tenant" } }
                },
                "list": { "type": "array", "items": { "type": "object", "properties": { "x": { "x-mcp-header": "Ignored" } } } },
                "dup": { "type": "string", "x-mcp-header": "region" },
                "bad": { "type": "string", "x-mcp-header": "Not A Token" }
            },
            "anyOf": [{ "properties": { "y": { "x-mcp-header": "AlsoIgnored" } } }]
        });
        let mut found = header_params(&schema);
        found.sort();
        let names: Vec<&str> = found.iter().map(|(_, name)| name.as_str()).collect();
        assert!(names.contains(&"Tenant"));
        assert!(!names.contains(&"Ignored"));
        assert!(!names.contains(&"AlsoIgnored"));
        assert!(!names.contains(&"Not A Token"));
        assert_eq!(names.iter().filter(|n| n.eq_ignore_ascii_case("region")).count(), 1);
        let tenant = found.iter().find(|(_, name)| name == "Tenant").unwrap();
        assert_eq!(tenant.0, vec!["nested".to_string(), "tenant".to_string()]);

        let declared = vec![
            (vec!["region".to_string()], "Region".to_string()),
            (vec!["count".to_string()], "Count".to_string()),
            (vec!["flag".to_string()], "Flag".to_string()),
            (vec!["missing".to_string()], "Missing".to_string()),
            (vec!["none".to_string()], "None".to_string()),
            (vec!["place".to_string()], "Place".to_string()),
        ];
        let headers = param_headers(
            &declared,
            &json!({ "region": "eu-west", "count": 42, "flag": true, "none": null, "place": "Zürich" }),
        );
        assert_eq!(
            headers,
            vec![
                ("Mcp-Param-Region".to_string(), "eu-west".to_string()),
                ("Mcp-Param-Count".to_string(), "42".to_string()),
                ("Mcp-Param-Flag".to_string(), "true".to_string()),
                ("Mcp-Param-Place".to_string(), "=?base64?WsO8cmljaA==?=".to_string()),
            ]
        );
    }

    #[test]
    fn sse_parser_handles_chunks_comments_and_multiline_data() {
        let mut parser = SseParser::default();
        let mut events = parser.feed(b": keep-alive\n\nevent: endpoint\ndata: /message?sessionId=abc\n\n");
        events.extend(parser.feed(b"data: {\"a\":"));
        events.extend(parser.feed(b"1}\r\n\r\nid: 7\ndata: line1\ndata:line2\n"));
        events.extend(parser.feed(b"\nevent: ping\n\n"));
        events.extend(parser.feed(b"data: tail"));
        events.extend(parser.finish());
        assert_eq!(
            events,
            vec![
                SseEvent { event: "endpoint".into(), data: "/message?sessionId=abc".into() },
                SseEvent { event: "message".into(), data: "{\"a\":1}".into() },
                SseEvent { event: "message".into(), data: "line1\nline2".into() },
                SseEvent { event: "message".into(), data: "tail".into() },
            ]
        );
        // UTF-8 split across chunks.
        let mut parser = SseParser::default();
        let bytes = "data: é\n\n".as_bytes();
        let mut events = parser.feed(&bytes[..7]);
        events.extend(parser.feed(&bytes[7..]));
        assert_eq!(events, vec![SseEvent { event: "message".into(), data: "é".into() }]);
    }

    #[test]
    fn tool_results_flatten() {
        let result = json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "image", "data": "AAAA", "mimeType": "image/jpeg" },
                { "type": "resource", "resource": { "uri": "file:///a.txt", "text": "file body" } },
                { "type": "resource", "resource": { "uri": "file:///b.bin", "blob": "Zm9v", "mimeType": "application/octet-stream" } },
                { "type": "resource_link", "uri": "file:///c", "name": "c" },
                { "type": "audio", "data": "xx", "mimeType": "audio/wav" }
            ],
            "isError": true
        });
        let flat = flatten_tool_result(&result);
        assert_eq!(flat.text, "hello\nfile body\n[resource file:///b.bin]\n[link: c file:///c]\n[audio: audio/wav]");
        assert!(flat.is_error);
        assert_eq!(flat.images, vec![("image/jpeg".to_string(), "AAAA".to_string())]);

        let structured = flatten_tool_result(&json!({ "content": [], "structuredContent": { "n": 1 } }));
        assert_eq!(structured.text, "{\n  \"n\": 1\n}");
        assert!(!structured.is_error);

        let both = flatten_tool_result(&json!({ "content": [{ "type": "text", "text": "x" }], "structuredContent": { "n": 1 } }));
        assert_eq!(both.text, "x");
    }

    #[test]
    fn modern_errors_are_recognised() {
        let err = |code: i64, data: Option<Value>| RpcError { code, message: String::new(), data };
        assert!(is_modern_error(&err(-32022, Some(json!({ "supported": ["2026-07-28"], "requested": "x" })))));
        assert!(is_modern_error(&err(-32020, None)));
        assert!(is_modern_error(&err(-32021, None)));
        assert!(is_modern_error(&err(-32029, None)));
        assert!(is_modern_error(&err(-32602, Some(json!({ "supported": ["2025-06-18"] })))));
        assert!(!is_modern_error(&err(-32601, None)));
        assert!(!is_modern_error(&err(-32000, Some(json!({ "other": 1 })))));
        assert!(!is_modern_error(&err(-32030, None)));

        let failure = Failure::status(
            400,
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32022, "message": "no", "data": { "supported": ["2025-11-25", "2025-06-18"] } } }).to_string(),
        );
        assert_eq!(classify_probe_error(&failure), ProbeError::Supported(Pick::Legacy("2025-11-25".into())));
        let failure = Failure::Rpc(err(-32601, None));
        assert_eq!(classify_probe_error(&failure), ProbeError::NotModern);
        let failure = Failure::status(404, "Not Found".into());
        assert_eq!(classify_probe_error(&failure), ProbeError::NotModern);
        let failure = Failure::Rpc(err(-32021, None));
        assert_eq!(classify_probe_error(&failure), ProbeError::Fatal);

        assert_eq!(pick_version(&["2026-07-28", "2025-11-25"]), Pick::Modern);
        assert_eq!(pick_version(&["2025-03-26", "2024-11-05"]), Pick::Legacy("2025-03-26".into()));
        assert_eq!(pick_version(&["1999-01-01"]), Pick::Nothing);
        assert_eq!(pick_from_discover(&json!({ "resultType": "complete" })), Pick::Modern);
    }

    #[test]
    fn router_matches_responses_and_answers_requests() {
        let router = Router::new(Some(PathBuf::from(r"C:\Work\My Project")));
        let mut rx = router.register(5);
        assert!(router.incoming(json!({ "jsonrpc": "2.0", "method": "notifications/message", "params": {} })).is_empty());
        let replies = router.incoming(json!([
            { "jsonrpc": "2.0", "id": "s1", "method": "ping" },
            { "jsonrpc": "2.0", "id": 9, "method": "roots/list" },
            { "jsonrpc": "2.0", "id": 10, "method": "sampling/createMessage" },
            { "jsonrpc": "2.0", "id": 5, "result": { "ok": true } }
        ]));
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0], json!({ "jsonrpc": "2.0", "id": "s1", "result": {} }));
        assert_eq!(replies[1]["result"]["roots"][0]["name"], "My Project");
        assert_eq!(replies[2]["error"]["code"], -32601);
        assert_eq!(response_result(rx.try_recv().unwrap()).unwrap(), json!({ "ok": true }));
        let rx2 = router.register(6);
        router.close();
        assert!(rx2.blocking_recv().is_err());
    }

    #[test]
    fn file_uris_and_meta() {
        assert_eq!(file_uri(Path::new(r"C:\Users\A B\proj")), "file:///C:/Users/A%20B/proj");
        assert_eq!(file_uri(Path::new("/home/u/p")), "file:///home/u/p");
        let merged = with_meta(json!({ "name": "t", "_meta": { "keep": 1 } }), &json!({ "k": "v" }));
        assert_eq!(merged, json!({ "name": "t", "_meta": { "keep": 1, "k": "v" } }));
        assert_eq!(with_meta(Value::Null, &json!({ "k": 1 })), json!({ "_meta": { "k": 1 } }));
    }

    #[test]
    fn input_requests_answer_roots_only() {
        let root = PathBuf::from("/tmp/p");
        let ok = answer_input_requests(Some(&json!({ "r1": { "method": "roots/list" } })), Some(&root)).unwrap();
        assert_eq!(ok["r1"]["roots"][0]["uri"], "file:///tmp/p");
        let refused = answer_input_requests(
            Some(&json!({ "r1": { "method": "roots/list" }, "e": { "method": "elicitation/create", "params": {} } })),
            Some(&root),
        );
        assert!(refused.unwrap_err().contains("elicitation"));
    }

    #[test]
    fn transport_serde_shape() {
        let stdio: Transport = serde_json::from_value(json!({ "type": "stdio", "command": "npx", "args": ["-y", "x"] })).unwrap();
        assert_eq!(
            stdio,
            Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), "x".into()], env: BTreeMap::new(), cwd: None }
        );
        let http: Transport = serde_json::from_value(json!({ "type": "http", "url": "http://x/mcp" })).unwrap();
        assert_eq!(http, Transport::Http { url: "http://x/mcp".into(), headers: BTreeMap::new() });
        let sse: Transport = serde_json::from_value(json!({ "type": "sse", "url": "http://x/sse", "headers": { "A": "b" } })).unwrap();
        assert!(matches!(sse, Transport::Sse { .. }));
    }

    // ----- End-to-end checks against real servers (need Node 24 + npx and network) -----------

    const EVERYTHING_MODERN: &str = "@modelcontextprotocol/server-everything@2.1.0";
    const EVERYTHING_LEGACY: &str = "@modelcontextprotocol/server-everything@2026.1.26";
    const FILESYSTEM: &str = "@modelcontextprotocol/server-filesystem@2.1.0";
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);
    const CALL_TIMEOUT: Duration = Duration::from_secs(30);

    fn npx(args: &[&str]) -> Transport {
        Transport::Stdio {
            command: "npx".into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    async fn timed_connect(transport: &Transport, root: Option<&Path>, probe_wait: Duration) -> (McpConnection, Duration) {
        let started = Instant::now();
        let conn = McpConnection::connect_with(transport, "serika-test", "0.0.0", root, CONNECT_TIMEOUT, probe_wait)
            .await
            .unwrap_or_else(|err| panic!("connect {} failed: {err}", transport.label()));
        (conn, started.elapsed())
    }

    async fn echo_round(conn: &McpConnection) {
        let tools = conn.list_tools().await.expect("tools/list");
        assert!(tools.iter().any(|t| t.name == "echo"), "no echo tool in {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());
        let result = conn.call_tool("echo", json!({ "message": "hi" }), CALL_TIMEOUT).await.expect("echo");
        println!("    tools: {}, echo -> {:?} (is_error {})", tools.len(), result.text, result.is_error);
        assert_eq!(result.text, "Echo: hi");
        assert!(!result.is_error);
    }

    fn report(name: &str, conn: &McpConnection, took: Duration) {
        println!(
            "{name}: era {:?}, version {}, server {:?}, connect {:.2}s",
            conn.era(),
            conn.protocol_version(),
            conn.server_name(),
            took.as_secs_f64()
        );
    }

    /// Starts `npx <package> <mode>` on `port` and waits until it listens.
    async fn start_http_server(package: &str, mode: &str, port: u16) -> tokio::process::Child {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/C", "npx", "-y", package, mode])
            .env("PORT", port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn().expect("start npx");
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)].into_iter().flatten() {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx.send(line);
                }
            });
        }
        let needle = format!("port {port}");
        let wait = async {
            while let Some(line) = rx.recv().await {
                println!("    [server] {line}");
                if line.contains(&needle) {
                    return;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(120), wait).await.expect("server did not start");
        child
    }

    async fn stop_server(child: tokio::process::Child) {
        kill_tree(child.id()).await;
        drop(child);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_stdio_everything_modern() {
        let transport = npx(&["-y", EVERYTHING_MODERN]);
        let (conn, took) = timed_connect(&transport, None, PROBE_WAIT).await;
        report("stdio everything 2.1.0", &conn, took);
        assert_eq!(conn.era(), Era::Modern);
        assert_eq!(conn.protocol_version(), MODERN_VERSION);
        echo_round(&conn).await;
        // Concurrent calls multiplex over one pipe.
        let calls = (0..5).map(|i| conn.call_tool("echo", json!({ "message": format!("m{i}") }), CALL_TIMEOUT));
        let results = futures_util::future::join_all(calls).await;
        for (i, result) in results.into_iter().enumerate() {
            assert_eq!(result.unwrap().text, format!("Echo: m{i}"));
        }
        let sum = conn.call_tool("get-sum", json!({ "a": 2, "b": 3 }), CALL_TIMEOUT).await.unwrap();
        println!("    get-sum -> {:?}", sum.text);
        let image = conn.call_tool("get-tiny-image", json!({}), CALL_TIMEOUT).await.unwrap();
        println!("    get-tiny-image -> {} image(s), text {:?}", image.images.len(), image.text);
        assert!(!image.images.is_empty());
        println!("    instructions: {} chars", conn.instructions().unwrap_or_default().len());
        conn.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_stdio_everything_modern_without_probe_wait() {
        // initialize is sent right behind server/discover: the modern answer must still win.
        let transport = npx(&["-y", EVERYTHING_MODERN]);
        let (conn, took) = timed_connect(&transport, None, Duration::ZERO).await;
        report("stdio everything 2.1.0 (zero probe wait)", &conn, took);
        assert_eq!(conn.era(), Era::Modern);
        echo_round(&conn).await;
        conn.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_stdio_everything_legacy() {
        let root = std::env::temp_dir();
        let transport = npx(&["-y", EVERYTHING_LEGACY]);
        let (conn, took) = timed_connect(&transport, Some(&root), PROBE_WAIT).await;
        report("stdio everything 2026.1.26", &conn, took);
        assert_eq!(conn.era(), Era::Legacy);
        echo_round(&conn).await;
        let tools = conn.list_tools().await.unwrap();
        if tools.iter().any(|t| t.name == "get-roots-list") {
            let roots = conn.call_tool("get-roots-list", json!({}), CALL_TIMEOUT).await.unwrap();
            println!("    get-roots-list -> {:?}", roots.text);
        }
        conn.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_stdio_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let path = dir.path().to_string_lossy().to_string();
        let transport = npx(&["-y", FILESYSTEM, &path]);
        let (conn, took) = timed_connect(&transport, Some(dir.path()), PROBE_WAIT).await;
        report("stdio filesystem 2.1.0", &conn, took);
        let tools = conn.list_tools().await.unwrap();
        assert!(tools.iter().any(|t| t.name == "list_directory"));
        let listing = conn.call_tool("list_directory", json!({ "path": path }), CALL_TIMEOUT).await.unwrap();
        println!("    list_directory -> {:?} (is_error {})", listing.text, listing.is_error);
        assert!(!listing.is_error);
        assert!(listing.text.contains("[FILE] hello.txt"));
        assert!(listing.text.contains("[DIR] sub"));
        conn.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_stdio_filesystem_legacy() {
        // The older filesystem server is 2025-era only.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();
        let path = dir.path().to_string_lossy().to_string();
        let transport = npx(&["-y", "@modelcontextprotocol/server-filesystem@2026.1.14", &path]);
        let (conn, took) = timed_connect(&transport, Some(dir.path()), PROBE_WAIT).await;
        report("stdio filesystem 2026.1.14", &conn, took);
        assert_eq!(conn.era(), Era::Legacy);
        let listing = conn.call_tool("list_directory", json!({ "path": path }), CALL_TIMEOUT).await.unwrap();
        println!("    list_directory -> {:?} (is_error {})", listing.text, listing.is_error);
        assert!(listing.text.contains("[FILE] hello.txt"));
        conn.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_http_modern_and_legacy_session() {
        let port = 39871;
        let server = start_http_server(EVERYTHING_MODERN, "streamableHttp", port).await;
        let url = format!("http://127.0.0.1:{port}/mcp");
        let transport = Transport::Http { url: url.clone(), headers: BTreeMap::new() };
        let (conn, took) = timed_connect(&transport, None, PROBE_WAIT).await;
        report("http everything 2.1.0", &conn, took);
        assert_eq!(conn.era(), Era::Modern);
        echo_round(&conn).await;
        let header = conn
            .call_tool("get-header-params", json!({ "region": "eu-west", "repeat": 2 }), CALL_TIMEOUT)
            .await
            .unwrap();
        println!("    get-header-params -> {:?} (is_error {})", header.text, header.is_error);
        assert!(header.text.contains("\"regionHeader\":\"eu-west\""));
        let header = conn
            .call_tool("get-header-params", json!({ "region": "Zürich" }), CALL_TIMEOUT)
            .await
            .unwrap();
        println!("    get-header-params (non-ASCII) -> {:?}", header.text);
        assert!(!header.is_error);
        conn.close().await;
        stop_server(server).await;

        let port = 39872;
        let server = start_http_server(EVERYTHING_LEGACY, "streamableHttp", port).await;
        let transport = Transport::Http { url: format!("http://127.0.0.1:{port}/mcp"), headers: BTreeMap::new() };
        let root = std::env::temp_dir();
        let (conn, took) = timed_connect(&transport, Some(&root), PROBE_WAIT).await;
        report("http everything 2026.1.26", &conn, took);
        assert_eq!(conn.era(), Era::Legacy);
        echo_round(&conn).await;
        conn.close().await;
        stop_server(server).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_http_sse() {
        let port = 39873;
        let server = start_http_server(EVERYTHING_MODERN, "sse", port).await;
        let url = format!("http://127.0.0.1:{port}/sse");
        let transport = Transport::Sse { url: url.clone(), headers: BTreeMap::new() };
        let (conn, took) = timed_connect(&transport, None, PROBE_WAIT).await;
        report("sse everything 2.1.0", &conn, took);
        assert_eq!(conn.era(), Era::Legacy);
        echo_round(&conn).await;
        conn.close().await;

        // Streamable HTTP pointed at an SSE-only endpoint falls all the way back.
        let transport = Transport::Http { url, headers: BTreeMap::new() };
        let (conn, took) = timed_connect(&transport, None, PROBE_WAIT).await;
        report("http -> sse fallback everything 2.1.0", &conn, took);
        assert_eq!(conn.era(), Era::Legacy);
        echo_round(&conn).await;
        conn.close().await;
        stop_server(server).await;

        let port = 39874;
        let server = start_http_server(EVERYTHING_LEGACY, "sse", port).await;
        let transport = Transport::Sse { url: format!("http://127.0.0.1:{port}/sse"), headers: BTreeMap::new() };
        let root = std::env::temp_dir();
        let (conn, took) = timed_connect(&transport, Some(&root), PROBE_WAIT).await;
        report("sse everything 2026.1.26", &conn, took);
        echo_round(&conn).await;
        conn.close().await;
        stop_server(server).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn e2e_bad_command_reports_stderr() {
        let transport = npx(&["-y", "@modelcontextprotocol/this-package-does-not-exist-serika"]);
        let started = Instant::now();
        let err = McpConnection::connect(&transport, "serika-test", "0", None, Duration::from_secs(60))
            .await
            .unwrap_err();
        println!("bad command failed after {:.2}s: {err}", started.elapsed().as_secs_f64());
    }
}
