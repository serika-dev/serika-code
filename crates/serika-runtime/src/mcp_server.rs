//! Serika's own tools as an MCP server, for agents that run in their own process.
//!
//! Claude Code and Codex bring their own tool loop, so they cannot call Serika's browser
//! directly. While the desktop runs, its backend serves the browser tools over Streamable
//! HTTP on 127.0.0.1 with a random bearer token, and passes that endpoint to the CLI. The
//! agent then drives the same browser the user watches in the panel.
//!
//! The server speaks both protocol eras: the stateless 2026-07-28 revision (per-request
//! `_meta`, `server/discover`) and the `initialize` handshake of 2025-11-25 and earlier.
//! It never mints sessions and answers every POST with a single JSON object.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::browser::Frame;
use crate::Runtime;

pub const SERVER_NAME: &str = "serika";
const MODERN: &str = "2026-07-28";
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Where a running server listens and the token it expects.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub token: String,
}

/// Called with every frame the server's browser actions produce, so the panel can follow.
pub type FrameSink = Arc<dyn Fn(Frame) + Send + Sync>;

/// Start the server on a free local port. It runs until the process exits.
pub async fn start(runtime: Arc<Runtime>, on_frame: FrameSink) -> crate::Result<Endpoint> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let expected = format!("Bearer {token}");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let runtime = runtime.clone();
            let on_frame = on_frame.clone();
            let expected = expected.clone();
            tokio::spawn(async move {
                let _ = serve_connection(stream, &runtime, &on_frame, &expected).await;
            });
        }
    });
    Ok(Endpoint { url: format!("http://127.0.0.1:{port}/mcp"), token })
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 64 * 1024 {
            return Ok(None);
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(8 * 1024 * 1024);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Some(Request { method, path, headers, body }))
}

async fn respond(stream: &mut TcpStream, status: &str, body: Option<&Value>) -> std::io::Result<()> {
    let text = body.map(|b| b.to_string()).unwrap_or_default();
    let content_type = if body.is_some() { "Content-Type: application/json\r\n" } else { "" };
    let head = format!(
        "HTTP/1.1 {status}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(text.as_bytes()).await?;
    stream.flush().await
}

async fn serve_connection(mut stream: TcpStream, runtime: &Runtime, on_frame: &FrameSink, expected: &str) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await? else { return Ok(()) };
    if request.header("authorization") != Some(expected) {
        return respond(&mut stream, "401 Unauthorized", None).await;
    }
    // A browser page must not be able to reach this server (DNS rebinding).
    if let Some(origin) = request.header("origin") {
        if !origin.starts_with("http://127.0.0.1") && !origin.starts_with("http://localhost") {
            return respond(&mut stream, "403 Forbidden", None).await;
        }
    }
    if request.path.split('?').next() != Some("/mcp") {
        return respond(&mut stream, "404 Not Found", None).await;
    }
    match request.method.as_str() {
        "POST" => {}
        "DELETE" => return respond(&mut stream, "200 OK", None).await,
        _ => return respond(&mut stream, "405 Method Not Allowed", None).await,
    }
    let Ok(message) = serde_json::from_slice::<Value>(&request.body) else {
        let error = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}});
        return respond(&mut stream, "400 Bad Request", Some(&error)).await;
    };
    let Some(id) = message.get("id").cloned() else {
        // Notifications (notifications/initialized and the like) need no answer.
        return respond(&mut stream, "202 Accepted", None).await;
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let modern = params
        .pointer("/_meta/io.modelcontextprotocol~1protocolVersion")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(version) = &modern {
        if version != MODERN && !LEGACY.contains(&version.as_str()) {
            let error = json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32022, "message": "Unsupported protocol version",
                "data": {"supported": supported(), "requested": version}
            }});
            return respond(&mut stream, "400 Bad Request", Some(&error)).await;
        }
    }
    let reply = match handle(method, &params, runtime, on_frame).await {
        Ok(mut result) => {
            if modern.is_some() {
                result["resultType"] = json!("complete");
            }
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        Err((code, text)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}}),
    };
    let status = if reply.get("error").and_then(|e| e.get("code")).and_then(Value::as_i64) == Some(-32601) {
        "404 Not Found"
    } else {
        "200 OK"
    };
    respond(&mut stream, status, Some(&reply)).await
}

fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY.iter().copied()).collect()
}

const INSTRUCTIONS: &str = "Serika's browser: a real browser with JavaScript that the user watches live in the Serika Code panel. \
Use browser_open to visit a page, then browser_snapshot for its text and numbered elements; click and type by element number.";

async fn handle(method: &str, params: &Value, runtime: &Runtime, on_frame: &FrameSink) -> Result<Value, (i64, String)> {
    let info = json!({"name": SERVER_NAME, "title": "Serika Code", "version": crate::client::VERSION});
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-11-25");
            let version = if LEGACY.contains(&asked) { asked } else { "2025-11-25" };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": info,
                "instructions": INSTRUCTIONS,
            }))
        }
        "server/discover" => Ok(json!({
            "supportedVersions": supported(),
            "capabilities": {"tools": {}},
            "_meta": {"io.modelcontextprotocol/serverInfo": info},
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            Ok(call_tool(runtime, on_frame, name, &args).await)
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

pub fn tool_list() -> Vec<Value> {
    vec![
        json!({"name": "browser_open", "description": "Open a web address (or search for a phrase) in the current tab and return the page snapshot.",
               "inputSchema": schema(json!({"url": {"type": "string", "description": "Address or search phrase"}}), &["url"])}),
        json!({"name": "browser_snapshot", "description": "Text of the current page with numbered interactive elements, plus a screenshot.",
               "inputSchema": schema(json!({}), &[])}),
        json!({"name": "browser_click", "description": "Click element number `id` from the latest snapshot.",
               "inputSchema": schema(json!({"id": {"type": "integer"}}), &["id"])}),
        json!({"name": "browser_type", "description": "Type text into element `id` (or the focused field), optionally pressing Enter.",
               "inputSchema": schema(json!({"id": {"type": "integer"}, "text": {"type": "string"}, "submit": {"type": "boolean"}, "clear": {"type": "boolean"}}), &["text"])}),
        json!({"name": "browser_press", "description": "Press a key: Enter, Tab, Escape, Backspace, ArrowDown, ArrowUp, PageDown, PageUp, Home, End.",
               "inputSchema": schema(json!({"key": {"type": "string"}}), &["key"])}),
        json!({"name": "browser_scroll", "description": "Scroll the page.",
               "inputSchema": schema(json!({"amount": {"type": "integer"}, "direction": {"type": "string", "enum": ["down", "up"]}}), &[])}),
        json!({"name": "browser_back", "description": "Go back one page.", "inputSchema": schema(json!({}), &[])}),
        json!({"name": "browser_new_tab", "description": "Open a new tab, optionally at an address.",
               "inputSchema": schema(json!({"url": {"type": "string"}}), &[])}),
        json!({"name": "browser_switch_tab", "description": "Show another tab by its number in the tab list.",
               "inputSchema": schema(json!({"tab": {"type": "integer"}}), &["tab"])}),
    ]
}

/// Names Claude Code may call without asking: `mcp__serika__browser_*`.
pub fn tool_names() -> Vec<String> {
    tool_list()
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .map(|n| format!("mcp__{SERVER_NAME}__{n}"))
        .collect()
}

async fn call_tool(runtime: &Runtime, on_frame: &FrameSink, name: &str, args: &Value) -> Value {
    let text_error = |message: String| json!({"content": [{"type": "text", "text": message}], "isError": true});
    if !runtime.live_config().browser_enabled {
        return text_error("The browser is turned off in Serika's settings (Settings → Browser).".into());
    }
    let browser = &runtime.browser;
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let outcome = match name {
        "browser_open" => browser.open(&s("url")).await,
        "browser_snapshot" => browser.snapshot().await,
        "browser_click" => match args.get("id").and_then(Value::as_u64) {
            Some(id) => browser.click(id as u32).await,
            None => return text_error("browser_click needs a numeric \"id\"".into()),
        },
        "browser_type" => {
            let id = args.get("id").and_then(Value::as_u64).map(|v| v as u32);
            let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
            let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(true);
            browser.type_text(id, &s("text"), submit, clear).await
        }
        "browser_press" => browser.press(&s("key")).await,
        "browser_scroll" => {
            let mut amount = args.get("amount").and_then(Value::as_f64).unwrap_or(600.0);
            if s("direction") == "up" {
                amount = -amount.abs();
            }
            browser.scroll(amount).await
        }
        "browser_back" => browser.back().await,
        "browser_new_tab" => browser.new_tab(args.get("url").and_then(Value::as_str)).await,
        "browser_switch_tab" => {
            let tab = args.get("tab").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
            browser.switch_tab(&tab).await
        }
        other => return text_error(format!("unknown tool {other}")),
    };
    match outcome {
        Ok((snapshot, frame)) => {
            let mut text = snapshot.to_model_text();
            if frame.tabs.len() > 1 {
                let list: Vec<String> = frame
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| format!("[{}] {}{} {}", i + 1, t.title, if t.active { " (current)" } else { "" }, t.url))
                    .collect();
                text = format!("Open tabs:\n{}\n\n{text}", list.join("\n"));
            }
            let mut content = vec![json!({"type": "text", "text": text})];
            // A picture helps on pages where the text alone is not enough.
            if matches!(name, "browser_snapshot" | "browser_open") {
                if let Some(image) = &frame.image {
                    content.push(json!({"type": "image", "data": image, "mimeType": "image/jpeg"}));
                }
            }
            on_frame(frame);
            json!({"content": content, "isError": false})
        }
        Err(err) => text_error(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_prefixed_browser_tools() {
        let names = tool_names();
        assert!(names.contains(&"mcp__serika__browser_open".to_string()));
        assert_eq!(names.len(), tool_list().len());
        for tool in tool_list() {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}
