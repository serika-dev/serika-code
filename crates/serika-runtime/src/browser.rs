//! A real browser the agent can drive and the user can watch.
//!
//! Serika starts a headless Chromium-based browser (Microsoft Edge ships with Windows)
//! and talks to it over the DevTools protocol. Pages run JavaScript, so sign-in flows and
//! single-page apps work. Every action returns a screenshot for the desktop's browser panel
//! and a text snapshot with numbered interactive elements for the model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::{Error, Result};

const VIEW_WIDTH: u32 = 1280;
const VIEW_HEIGHT: u32 = 800;
const MAX_TABS: usize = 12;

/// A tab as the browser panel lists it.
#[derive(Debug, Clone, Serialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub active: bool,
}

/// What the browser panel shows after an action.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub url: String,
    pub title: String,
    /// Base64 JPEG of the viewport.
    pub image: Option<String>,
    pub width: u32,
    pub height: u32,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub loading: bool,
    pub tabs: Vec<TabInfo>,
    pub active_tab: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Element {
    pub id: u32,
    pub tag: String,
    pub kind: Option<String>,
    pub label: String,
    pub href: Option<String>,
    pub placeholder: Option<String>,
}

/// Page text plus numbered elements the model can click or type into.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub url: String,
    pub title: String,
    pub text: String,
    pub elements: Vec<Element>,
    pub scroll_y: i64,
    pub scroll_height: i64,
}

impl Snapshot {
    pub fn to_model_text(&self) -> String {
        let mut out = format!("URL: {}\nTitle: {}\n", self.url, self.title);
        out.push_str(&format!(
            "Scroll: {} of {} px\n\nInteractive elements (use the number with browser_click / browser_type):\n",
            self.scroll_y, self.scroll_height
        ));
        if self.elements.is_empty() {
            out.push_str("(none visible)\n");
        }
        for el in &self.elements {
            let kind = el.kind.as_deref().map(|k| format!("({k})")).unwrap_or_default();
            let mut line = format!("[{}] {}{} \"{}\"", el.id, el.tag, kind, el.label);
            if let Some(href) = &el.href {
                line.push_str(&format!(" -> {href}"));
            }
            if let Some(ph) = &el.placeholder {
                if !ph.is_empty() && ph != &el.label {
                    line.push_str(&format!(" placeholder=\"{ph}\""));
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str("\nPage text:\n");
        out.push_str(&self.text);
        out
    }
}

struct TargetRow {
    id: String,
    kind: String,
    title: String,
    url: String,
    opener: Option<String>,
}

type Pending =Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>>;
type Sink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

struct Cdp {
    sink: Mutex<Sink>,
    pending: Pending,
    events: broadcast::Sender<Value>,
    next: AtomicU64,
}

impl Cdp {
    async fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.sink
            .lock()
            .await
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|err| Error::msg(format!("browser connection lost: {err}")))?;
        let response = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| Error::msg(format!("the browser did not answer {method}")))?
            .map_err(|_| Error::msg("the browser closed"))?;
        if let Some(error) = response.get("error") {
            let text = error.get("message").and_then(|m| m.as_str()).unwrap_or("error");
            return Err(Error::msg(format!("browser: {text} ({method})")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }
}

struct Tab {
    target: String,
    session: String,
}

struct Live {
    child: Child,
    cdp: Arc<Cdp>,
    tabs: Vec<Tab>,
    active: usize,
}

impl Live {
    fn session(&self) -> String {
        self.tabs[self.active.min(self.tabs.len() - 1)].session.clone()
    }
}

pub struct BrowserHost {
    profile_dir: PathBuf,
    binary: Option<String>,
    live: Mutex<Option<Live>>,
    /// Viewport size every tab renders at; the panel sets it to its own size.
    view: StdMutex<(u32, u32)>,
    /// The browser's user agent without "Headless", which some sites refuse.
    user_agent: StdMutex<Option<String>>,
}

/// Locate a Chromium-based browser: the configured path, then common install locations.
pub fn find_browser(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(var) {
                let root = PathBuf::from(root);
                candidates.push(root.join("Microsoft/Edge/Application/msedge.exe"));
                candidates.push(root.join("Google/Chrome/Application/chrome.exe"));
                candidates.push(root.join("BraveSoftware/Brave-Browser/Application/brave.exe"));
                candidates.push(root.join("Chromium/Application/chrome.exe"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for app in [
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ] {
            candidates.push(PathBuf::from(app));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for name in [
            "microsoft-edge",
            "microsoft-edge-stable",
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "brave-browser",
        ] {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    candidates.push(dir.join(name));
                }
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn normalize_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::msg("the address is empty"));
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(raw.to_string());
    }
    if lower.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") {
        return Err(Error::msg("only http and https pages can be opened"));
    }
    // A bare phrase is a search; a host-like word is a site.
    if raw.contains(' ') || !raw.contains('.') {
        let query: String = raw
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    vec![b as char].into_iter().collect::<Vec<_>>()
                }
                b' ' => vec!['+'],
                other => format!("%{other:02X}").chars().collect(),
            })
            .collect();
        return Ok(format!("https://duckduckgo.com/?q={query}"));
    }
    Ok(format!("https://{raw}"))
}

struct KeySpec {
    key: &'static str,
    code: &'static str,
    vk: u32,
    text: Option<&'static str>,
}

fn key_spec(name: &str) -> Option<KeySpec> {
    let spec = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => KeySpec { key: "Enter", code: "Enter", vk: 13, text: Some("\r") },
        "tab" => KeySpec { key: "Tab", code: "Tab", vk: 9, text: None },
        "escape" | "esc" => KeySpec { key: "Escape", code: "Escape", vk: 27, text: None },
        "backspace" => KeySpec { key: "Backspace", code: "Backspace", vk: 8, text: None },
        "delete" => KeySpec { key: "Delete", code: "Delete", vk: 46, text: None },
        "arrowup" | "up" => KeySpec { key: "ArrowUp", code: "ArrowUp", vk: 38, text: None },
        "arrowdown" | "down" => KeySpec { key: "ArrowDown", code: "ArrowDown", vk: 40, text: None },
        "arrowleft" | "left" => KeySpec { key: "ArrowLeft", code: "ArrowLeft", vk: 37, text: None },
        "arrowright" | "right" => KeySpec { key: "ArrowRight", code: "ArrowRight", vk: 39, text: None },
        "pagedown" => KeySpec { key: "PageDown", code: "PageDown", vk: 34, text: None },
        "pageup" => KeySpec { key: "PageUp", code: "PageUp", vk: 33, text: None },
        "home" => KeySpec { key: "Home", code: "Home", vk: 36, text: None },
        "end" => KeySpec { key: "End", code: "End", vk: 35, text: None },
        "space" => KeySpec { key: " ", code: "Space", vk: 32, text: Some(" ") },
        _ => return None,
    };
    Some(spec)
}

const SNAPSHOT_JS: &str = r#"(() => {
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 1 && r.height > 1 && s.visibility !== 'hidden' && s.display !== 'none'
      && r.bottom > 0 && r.right > 0 && r.top < innerHeight * 2.5 && r.left < innerWidth;
  };
  document.querySelectorAll('[data-serika-i]').forEach((e) => e.removeAttribute('data-serika-i'));
  const sel = 'a[href],button,input,textarea,select,summary,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[onclick],[contenteditable=true]';
  const elements = [];
  let n = 1;
  for (const el of document.querySelectorAll(sel)) {
    if (!visible(el)) continue;
    if (n > 90) break;
    el.setAttribute('data-serika-i', String(n));
    const tag = el.tagName.toLowerCase();
    const label = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || el.title || el.alt || '')
      .replace(/\s+/g, ' ').trim().slice(0, 90);
    elements.push({ id: n, tag, kind: el.type || null, label, href: el.href || null, placeholder: el.placeholder || null });
    n++;
  }
  const text = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').trim().slice(0, 7000);
  return JSON.stringify({ url: location.href, title: document.title, text, elements,
    scroll_y: Math.round(scrollY), scroll_height: document.documentElement.scrollHeight });
})()"#;

impl BrowserHost {
    pub fn new(profile_dir: PathBuf, binary: Option<String>) -> Self {
        Self {
            profile_dir,
            binary,
            live: Mutex::new(None),
            view: StdMutex::new((VIEW_WIDTH, VIEW_HEIGHT)),
            user_agent: StdMutex::new(None),
        }
    }

    fn view(&self) -> (u32, u32) {
        *self.view.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn detected_binary(&self) -> Option<PathBuf> {
        find_browser(self.binary.as_deref())
    }

    pub async fn is_running(&self) -> bool {
        self.live.lock().await.is_some()
    }

    pub async fn shutdown(&self) {
        if let Some(mut live) = self.live.lock().await.take() {
            let _ = live.cdp.call("Browser.close", json!({}), None).await;
            let _ = live.child.kill().await;
        }
    }

    /// The connection and the active tab, starting the browser when it is not running.
    async fn live(&self) -> Result<(Arc<Cdp>, String)> {
        let mut guard = self.live.lock().await;
        if let Some(live) = guard.as_mut() {
            if matches!(live.child.try_wait(), Ok(None)) && !live.tabs.is_empty() {
                return Ok((live.cdp.clone(), live.session()));
            }
            *guard = None;
        }
        let live = self.launch().await?;
        let out = (live.cdp.clone(), live.session());
        *guard = Some(live);
        Ok(out)
    }

    /// Attach to a page target and give it the shared viewport.
    async fn attach(&self, cdp: &Cdp, target: &str) -> Result<Tab> {
        let attached = cdp
            .call("Target.attachToTarget", json!({"targetId": target, "flatten": true}), None)
            .await?;
        let session = attached
            .get("sessionId")
            .and_then(|id| id.as_str())
            .map(str::to_string)
            .ok_or_else(|| Error::msg("could not attach to the browser tab"))?;
        cdp.call("Page.enable", json!({}), Some(&session)).await?;
        let agent = self.user_agent.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(agent) = agent {
            let _ = cdp
                .call("Emulation.setUserAgentOverride", json!({"userAgent": agent}), Some(&session))
                .await;
        }
        let (width, height) = self.view();
        cdp.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
            Some(&session),
        )
        .await?;
        Ok(Tab {
            target: target.to_string(),
            session,
        })
    }

    /// Targets the browser reports.
    async fn targets(cdp: &Cdp) -> Vec<TargetRow> {
        let Ok(list) = cdp.call("Target.getTargets", json!({}), None).await else {
            return Vec::new();
        };
        list.get("targetInfos")
            .and_then(|l| l.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|t| {
                        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let id = s("targetId");
                        (!id.is_empty()).then(|| TargetRow {
                            kind: s("type"),
                            title: s("title"),
                            url: s("url"),
                            opener: t.get("openerId").and_then(|v| v.as_str()).map(str::to_string),
                            id,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Pages opened by one of our tabs (target=_blank links, window.open) become tabs too,
    /// and the newest one is shown.
    async fn adopt_popups(&self) {
        let mut guard = self.live.lock().await;
        let Some(live) = guard.as_mut() else { return };
        let cdp = live.cdp.clone();
        for row in Self::targets(&cdp).await {
            let ours = live.tabs.iter().any(|t| t.target == row.id);
            let from_us = row
                .opener
                .as_ref()
                .map(|o| live.tabs.iter().any(|t| &t.target == o))
                .unwrap_or(false);
            if row.kind != "page" || ours || !from_us || live.tabs.len() >= MAX_TABS {
                continue;
            }
            if let Ok(tab) = self.attach(&cdp, &row.id).await {
                live.tabs.push(tab);
                live.active = live.tabs.len() - 1;
            }
        }
    }

    async fn tab_infos(&self, cdp: &Cdp) -> (Vec<TabInfo>, String) {
        let (ids, active) = {
            let guard = self.live.lock().await;
            match guard.as_ref() {
                Some(live) => (
                    live.tabs.iter().map(|t| t.target.clone()).collect::<Vec<_>>(),
                    live.tabs.get(live.active).map(|t| t.target.clone()).unwrap_or_default(),
                ),
                None => (Vec::new(), String::new()),
            }
        };
        let known = Self::targets(cdp).await;
        let tabs = ids
            .iter()
            .map(|id| {
                let found = known.iter().find(|t| &t.id == id);
                TabInfo {
                    id: id.clone(),
                    title: found.map(|t| t.title.clone()).unwrap_or_default(),
                    url: found.map(|t| t.url.clone()).unwrap_or_default(),
                    active: *id == active,
                }
            })
            .collect();
        (tabs, active)
    }

    /// Open a tab, optionally at an address, and show it.
    pub async fn new_tab(&self, url: Option<&str>) -> Result<(Snapshot, Frame)> {
        let target = url
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(normalize_url)
            .transpose()?;
        let (cdp, _) = self.live().await?;
        {
            let mut guard = self.live.lock().await;
            let live = guard.as_mut().ok_or_else(|| Error::msg("the browser closed"))?;
            if live.tabs.len() >= MAX_TABS {
                return Err(Error::msg(format!("at most {MAX_TABS} tabs can be open")));
            }
            let created = cdp
                .call("Target.createTarget", json!({"url": "about:blank"}), None)
                .await?;
            let id = created
                .get("targetId")
                .and_then(|id| id.as_str())
                .ok_or_else(|| Error::msg("the browser did not open a tab"))?
                .to_string();
            let tab = self.attach(&cdp, &id).await?;
            live.tabs.push(tab);
            live.active = live.tabs.len() - 1;
        }
        match target {
            Some(target) => self.open(&target).await,
            None => self.finish().await,
        }
    }

    /// Show another tab. `id` is a tab id or its 1-based position.
    pub async fn switch_tab(&self, id: &str) -> Result<(Snapshot, Frame)> {
        let (cdp, _) = self.live().await?;
        {
            let mut guard = self.live.lock().await;
            let live = guard.as_mut().ok_or_else(|| Error::msg("the browser closed"))?;
            let index = live
                .tabs
                .iter()
                .position(|t| t.target == id)
                .or_else(|| id.trim().parse::<usize>().ok().filter(|n| *n >= 1).map(|n| n - 1))
                .filter(|i| *i < live.tabs.len())
                .ok_or_else(|| Error::msg(format!("there is no tab {id}")))?;
            live.active = index;
            let _ = cdp
                .call("Target.activateTarget", json!({"targetId": live.tabs[index].target}), None)
                .await;
        }
        self.finish().await
    }

    /// Close a tab. The last tab is replaced by an empty one, so the panel always has a page.
    pub async fn close_tab(&self, id: &str) -> Result<Frame> {
        let (cdp, _) = self.live().await?;
        let empty = {
            let mut guard = self.live.lock().await;
            let live = guard.as_mut().ok_or_else(|| Error::msg("the browser closed"))?;
            let index = live
                .tabs
                .iter()
                .position(|t| t.target == id)
                .ok_or_else(|| Error::msg(format!("there is no tab {id}")))?;
            let tab = live.tabs.remove(index);
            let _ = cdp
                .call("Target.closeTarget", json!({"targetId": tab.target}), None)
                .await;
            if live.active > index || live.active >= live.tabs.len() {
                live.active = live.active.saturating_sub(1);
            }
            live.tabs.is_empty()
        };
        if empty {
            return self.new_tab(None).await.map(|(_, frame)| frame);
        }
        self.frame().await
    }

    /// Render every tab at the panel's size. Sizes are clamped to what a page can use.
    pub async fn resize(&self, width: u32, height: u32) -> Result<Option<Frame>> {
        let size = (width.clamp(320, 3840), height.clamp(240, 2400));
        if self.view() == size {
            return Ok(None);
        }
        *self.view.lock().unwrap_or_else(|e| e.into_inner()) = size;
        let sessions: Vec<(Arc<Cdp>, String)> = {
            let guard = self.live.lock().await;
            match guard.as_ref() {
                Some(live) => live
                    .tabs
                    .iter()
                    .map(|t| (live.cdp.clone(), t.session.clone()))
                    .collect(),
                None => return Ok(None),
            }
        };
        for (cdp, session) in sessions {
            let _ = cdp
                .call(
                    "Emulation.setDeviceMetricsOverride",
                    json!({"width": size.0, "height": size.1, "deviceScaleFactor": 1, "mobile": false}),
                    Some(&session),
                )
                .await;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
        self.frame().await.map(Some)
    }

    /// A new frame when the browser is already open; never starts it.
    pub async fn frame_if_running(&self) -> Result<Option<Frame>> {
        if !self.is_running().await {
            return Ok(None);
        }
        self.frame().await.map(Some)
    }

    async fn launch(&self) -> Result<Live> {
        let binary = find_browser(self.binary.as_deref()).ok_or_else(|| {
            Error::msg(
                "no Chromium-based browser found. Install Microsoft Edge or Google Chrome, \
                 or set its path in Settings → Browser.",
            )
        })?;
        std::fs::create_dir_all(&self.profile_dir)?;
        let port_file = self.profile_dir.join("DevToolsActivePort");
        let mut attempt = 0;
        let (child, port, path) = loop {
            attempt += 1;
            let _ = std::fs::remove_file(&port_file);
            let mut child = self.spawn_browser(&binary)?;
            match wait_for_port(&port_file, &mut child).await {
                PortWait::Ready(port, path) => break (child, port, path),
                // A browser left over from a crashed or killed run still holds the profile:
                // the new one hands over to it and exits. Clear it out and try once more.
                PortWait::Exited if attempt == 1 => {
                    kill_profile_orphans(&self.profile_dir).await;
                    tokio::time::sleep(Duration::from_millis(600)).await;
                }
                PortWait::Exited => {
                    return Err(Error::msg(
                        "the browser closed right after starting. Another copy may be using Serika's browser profile; \
                         close it from Task Manager or restart the app.",
                    ))
                }
                PortWait::TimedOut => {
                    let _ = child.kill().await;
                    if attempt == 1 {
                        kill_profile_orphans(&self.profile_dir).await;
                        continue;
                    }
                    return Err(Error::msg("the browser did not open its debugging port"));
                }
            }
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}{path}"))
            .await
            .map_err(|err| Error::msg(format!("could not connect to the browser: {err}")))?;
        let (sink, mut stream) = socket.split();
        let pending: Pending = Arc::new(StdMutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(256);
        let cdp = Arc::new(Cdp {
            sink: Mutex::new(sink),
            pending: pending.clone(),
            events: events.clone(),
            next: AtomicU64::new(1),
        });
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                    continue;
                };
                if let Some(id) = value.get("id").and_then(|id| id.as_u64()) {
                    let sender = pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                    if let Some(sender) = sender {
                        let _ = sender.send(value);
                    }
                } else {
                    let _ = events.send(value);
                }
            }
        });

        if let Ok(version) = cdp.call("Browser.getVersion", json!({}), None).await {
            if let Some(agent) = version.get("userAgent").and_then(|a| a.as_str()) {
                *self.user_agent.lock().unwrap_or_else(|e| e.into_inner()) = Some(normal_user_agent(agent));
            }
        }
        let targets = cdp.call("Target.getTargets", json!({}), None).await?;
        let page = targets
            .get("targetInfos")
            .and_then(|list| list.as_array())
            .and_then(|list| {
                list.iter()
                    .find(|t| t.get("type").and_then(|k| k.as_str()) == Some("page"))
                    .and_then(|t| t.get("targetId").and_then(|id| id.as_str()).map(str::to_string))
            });
        let target_id = match page {
            Some(id) => id,
            None => cdp
                .call("Target.createTarget", json!({"url": "about:blank"}), None)
                .await?
                .get("targetId")
                .and_then(|id| id.as_str())
                .map(str::to_string)
                .ok_or_else(|| Error::msg("the browser did not open a tab"))?,
        };
        let tab = self.attach(&cdp, &target_id).await?;
        Ok(Live {
            child,
            cdp,
            tabs: vec![tab],
            active: 0,
        })
    }

    fn spawn_browser(&self, binary: &Path) -> Result<Child> {
        let mut command = Command::new(binary);
        command
            .args([
                "--headless=new",
                "--remote-debugging-port=0",
                "--remote-allow-origins=*",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-sync",
                "--hide-scrollbars",
                "--mute-audio",
            ])
            .arg({
                let (width, height) = self.view();
                format!("--window-size={width},{height}")
            })
            .arg(format!("--user-data-dir={}", self.profile_dir.display()))
            .arg("about:blank")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .spawn()
            .map_err(|err| Error::msg(format!("could not start {}: {err}", binary.display())))?;
        #[cfg(windows)]
        job::adopt(&child);
        Ok(child)
    }

    async fn eval(&self, cdp: &Cdp, session: &str, expression: &str) -> Result<Value> {
        let result = cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
                Some(session),
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            let text = exception
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(|d| d.as_str())
                .or_else(|| exception.get("text").and_then(|t| t.as_str()))
                .unwrap_or("script error");
            return Err(Error::msg(format!("page script failed: {text}")));
        }
        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn history(&self, cdp: &Cdp, session: &str) -> (bool, bool) {
        let Ok(history) = cdp.call("Page.getNavigationHistory", json!({}), Some(session)).await
        else {
            return (false, false);
        };
        let index = history.get("currentIndex").and_then(|i| i.as_i64()).unwrap_or(0);
        let count = history
            .get("entries")
            .and_then(|e| e.as_array())
            .map(|e| e.len() as i64)
            .unwrap_or(0);
        (index > 0, index + 1 < count)
    }

    async fn settle(&self) {
        tokio::time::sleep(Duration::from_millis(450)).await;
    }

    async fn take_frame(&self, cdp: &Cdp, session: &str) -> Result<Frame> {
        let info = self
            .eval(
                cdp,
                session,
                "({url: location.href, title: document.title, loading: document.readyState !== 'complete'})",
            )
            .await
            .unwrap_or(Value::Null);
        let image = cdp
            .call(
                "Page.captureScreenshot",
                json!({"format": "jpeg", "quality": 60}),
                Some(session),
            )
            .await
            .ok()
            .and_then(|shot| shot.get("data").and_then(|d| d.as_str()).map(str::to_string));
        let (back, forward) = self.history(cdp, session).await;
        let (tabs, active_tab) = self.tab_infos(cdp).await;
        let (width, height) = self.view();
        Ok(Frame {
            url: info.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            title: info.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(),
            image,
            width,
            height,
            can_go_back: back,
            can_go_forward: forward,
            loading: info.get("loading").and_then(|l| l.as_bool()).unwrap_or(false),
            tabs,
            active_tab,
        })
    }

    async fn take_snapshot(&self, cdp: &Cdp, session: &str) -> Result<Snapshot> {
        let value = self.eval(cdp, session, SNAPSHOT_JS).await?;
        let text = value
            .as_str()
            .ok_or_else(|| Error::msg("the page did not return a snapshot"))?;
        let parsed: Value = serde_json::from_str(text)?;
        let elements = parsed
            .get("elements")
            .and_then(|e| e.as_array())
            .map(|list| {
                list.iter()
                    .map(|el| Element {
                        id: el.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        tag: el.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        kind: el.get("kind").and_then(|v| v.as_str()).map(str::to_string),
                        label: el.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        href: el.get("href").and_then(|v| v.as_str()).map(str::to_string),
                        placeholder: el.get("placeholder").and_then(|v| v.as_str()).map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Snapshot {
            url: parsed.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            title: parsed.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            text: parsed.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            elements,
            scroll_y: parsed.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0),
            scroll_height: parsed.get("scroll_height").and_then(|v| v.as_i64()).unwrap_or(0),
        })
    }

    /// Frame and snapshot after an action that changed the page. A popup the action opened
    /// becomes the active tab first.
    async fn finish(&self) -> Result<(Snapshot, Frame)> {
        self.settle().await;
        self.adopt_popups().await;
        let (cdp, session) = self.live().await?;
        let snapshot = self.take_snapshot(&cdp, &session).await?;
        let frame = self.take_frame(&cdp, &session).await?;
        Ok((snapshot, frame))
    }

    pub async fn open(&self, url: &str) -> Result<(Snapshot, Frame)> {
        let url = normalize_url(url)?;
        let (cdp, session) = self.live().await?;
        let mut events = cdp.events.subscribe();
        let result = cdp
            .call("Page.navigate", json!({"url": url}), Some(&session))
            .await?;
        if let Some(error) = result.get("errorText").and_then(|e| e.as_str()) {
            return Err(Error::msg(format!("could not open {url}: {error}")));
        }
        let wait = async {
            while let Ok(event) = events.recv().await {
                if event.get("method").and_then(|m| m.as_str()) == Some("Page.loadEventFired") {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(20), wait).await;
        self.finish().await
    }

    pub async fn snapshot(&self) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        let snapshot = self.take_snapshot(&cdp, &session).await?;
        let frame = self.take_frame(&cdp, &session).await?;
        Ok((snapshot, frame))
    }

    pub async fn frame(&self) -> Result<Frame> {
        self.adopt_popups().await;
        let (cdp, session) = self.live().await?;
        self.take_frame(&cdp, &session).await
    }

    async fn element_center(&self, cdp: &Cdp, session: &str, id: u32) -> Result<(f64, f64)> {
        let script = format!(
            "(() => {{ const el = document.querySelector('[data-serika-i=\"{id}\"]'); if (!el) return null; \
             el.scrollIntoView({{block: 'center', inline: 'center'}}); const r = el.getBoundingClientRect(); \
             return [r.left + r.width / 2, r.top + r.height / 2]; }})()"
        );
        let value = self.eval(cdp, session, &script).await?;
        let point = value
            .as_array()
            .and_then(|p| Some((p.first()?.as_f64()?, p.get(1)?.as_f64()?)))
            .ok_or_else(|| {
                Error::msg(format!(
                    "element {id} is not on the page. Call browser_snapshot to get fresh numbers."
                ))
            })?;
        Ok(point)
    }

    async fn mouse_click(&self, cdp: &Cdp, session: &str, x: f64, y: f64) -> Result<()> {
        for (kind, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
            cdp.call(
                "Input.dispatchMouseEvent",
                json!({"type": kind, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": 1}),
                Some(session),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn click(&self, id: u32) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        let (x, y) = self.element_center(&cdp, &session, id).await?;
        self.mouse_click(&cdp, &session, x, y).await?;
        self.finish().await
    }

    pub async fn click_at(&self, x: f64, y: f64) -> Result<Frame> {
        let (cdp, session) = self.live().await?;
        self.mouse_click(&cdp, &session, x, y).await?;
        self.settle().await;
        self.frame().await
    }

    pub async fn type_text(&self, id: Option<u32>, text: &str, submit: bool, clear: bool) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        if let Some(id) = id {
            let (x, y) = self.element_center(&cdp, &session, id).await?;
            self.mouse_click(&cdp, &session, x, y).await?;
            if clear {
                let _ = self
                    .eval(
                        &cdp,
                        &session,
                        "(() => { const el = document.activeElement; if (el && 'value' in el) { el.select(); } })()",
                    )
                    .await;
            }
        }
        cdp.call("Input.insertText", json!({"text": text}), Some(&session)).await?;
        if submit {
            self.press_key_raw(&cdp, &session, "enter").await?;
        }
        self.finish().await
    }

    async fn press_key_raw(&self, cdp: &Cdp, session: &str, name: &str) -> Result<()> {
        let spec = key_spec(name).ok_or_else(|| Error::msg(format!("unknown key \"{name}\"")))?;
        let mut down = json!({
            "type": if spec.text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": spec.key, "code": spec.code,
            "windowsVirtualKeyCode": spec.vk, "nativeVirtualKeyCode": spec.vk,
        });
        if let Some(text) = spec.text {
            down["text"] = json!(text);
        }
        cdp.call("Input.dispatchKeyEvent", down, Some(session)).await?;
        cdp.call(
            "Input.dispatchKeyEvent",
            json!({"type": "keyUp", "key": spec.key, "code": spec.code,
                   "windowsVirtualKeyCode": spec.vk, "nativeVirtualKeyCode": spec.vk}),
            Some(session),
        )
        .await?;
        Ok(())
    }

    pub async fn press(&self, name: &str) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        self.press_key_raw(&cdp, &session, name).await?;
        self.finish().await
    }

    /// Keyboard input from the browser panel: a key name or literal text.
    pub async fn input(&self, key: Option<&str>, text: Option<&str>) -> Result<Frame> {
        let (cdp, session) = self.live().await?;
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            cdp.call("Input.insertText", json!({"text": text}), Some(&session)).await?;
        }
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            self.press_key_raw(&cdp, &session, key).await?;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.take_frame(&cdp, &session).await
    }

    pub async fn scroll(&self, delta_y: f64) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        self.wheel_raw(&cdp, &session, 640.0, 400.0, delta_y).await?;
        self.finish().await
    }

    async fn wheel_raw(&self, cdp: &Cdp, session: &str, x: f64, y: f64, delta_y: f64) -> Result<()> {
        cdp.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": delta_y}),
            Some(session),
        )
        .await?;
        Ok(())
    }

    pub async fn wheel_at(&self, x: f64, y: f64, delta_y: f64) -> Result<Frame> {
        let (cdp, session) = self.live().await?;
        self.wheel_raw(&cdp, &session, x, y, delta_y).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.take_frame(&cdp, &session).await
    }

    async fn go_history(&self, step: i64) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        let history = cdp
            .call("Page.getNavigationHistory", json!({}), Some(&session))
            .await?;
        let index = history.get("currentIndex").and_then(|i| i.as_i64()).unwrap_or(0) + step;
        let entry = history
            .get("entries")
            .and_then(|e| e.as_array())
            .and_then(|e| e.get(usize::try_from(index).ok()?))
            .and_then(|e| e.get("id").and_then(|id| id.as_i64()))
            .ok_or_else(|| Error::msg("there is no page in that direction"))?;
        cdp.call("Page.navigateToHistoryEntry", json!({"entryId": entry}), Some(&session))
            .await?;
        self.finish().await
    }

    pub async fn back(&self) -> Result<(Snapshot, Frame)> {
        self.go_history(-1).await
    }

    pub async fn forward(&self) -> Result<(Snapshot, Frame)> {
        self.go_history(1).await
    }

    pub async fn reload(&self) -> Result<(Snapshot, Frame)> {
        let (cdp, session) = self.live().await?;
        cdp.call("Page.reload", json!({}), Some(&session)).await?;
        tokio::time::sleep(Duration::from_millis(900)).await;
        self.finish().await
    }
}

/// "HeadlessChrome/131" → "Chrome/131", "HeadlessEdg/131" → "Edg/131".
fn normal_user_agent(agent: &str) -> String {
    agent.replace("HeadlessChrome", "Chrome").replace("HeadlessEdg", "Edg").replace("Headless", "")
}

enum PortWait {
    Ready(String, String),
    Exited,
    TimedOut,
}

/// Wait for the browser to write its DevTools endpoint, or to exit early.
async fn wait_for_port(port_file: &Path, child: &mut Child) -> PortWait {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(port_file) {
            let mut lines = text.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                return PortWait::Ready(port.trim().to_string(), path.trim().to_string());
            }
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            return PortWait::Exited;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    PortWait::TimedOut
}

/// Stop browsers that still use our profile after the app that started them is gone.
async fn kill_profile_orphans(profile: &Path) {
    let marker = profile.display().to_string();
    #[cfg(windows)]
    {
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"Name='msedge.exe' OR Name='chrome.exe' OR Name='brave.exe'\" | \
             Where-Object {{ $_.CommandLine -and $_.CommandLine.Contains('{}') }} | \
             ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            marker.replace('\'', "''")
        );
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        command.creation_flags(0x08000000);
        let _ = tokio::time::timeout(Duration::from_secs(15), command.output()).await;
    }
    #[cfg(not(windows))]
    {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            Command::new("pkill").args(["-f", &format!("user-data-dir={marker}")]).output(),
        )
        .await;
    }
}

/// On Windows the browser lives in a job object that is closed when Serika exits, however it
/// exits, so a killed or crashed app never leaves a headless browser holding the profile.
#[cfg(windows)]
mod job {
    use std::sync::OnceLock;

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimit {
        per_process_user_time: i64,
        per_job_user_time: i64,
        limit_flags: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimit {
        basic: BasicLimit,
        io: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    extern "system" {
        fn CreateJobObjectW(attributes: *mut core::ffi::c_void, name: *const u16) -> isize;
        fn SetInformationJobObject(job: isize, class: i32, info: *mut core::ffi::c_void, length: u32) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    }

    const EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const KILL_ON_JOB_CLOSE: u32 = 0x2000;

    fn handle() -> isize {
        static JOB: OnceLock<isize> = OnceLock::new();
        *JOB.get_or_init(|| {
            // SAFETY: creates an unnamed job object owned by this process and sets its limits.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if job != 0 {
                    let mut info = ExtendedLimit::default();
                    info.basic.limit_flags = KILL_ON_JOB_CLOSE;
                    SetInformationJobObject(
                        job,
                        EXTENDED_LIMIT_INFORMATION,
                        &mut info as *mut ExtendedLimit as *mut core::ffi::c_void,
                        std::mem::size_of::<ExtendedLimit>() as u32,
                    );
                }
                job
            }
        })
    }

    pub fn adopt(child: &tokio::process::Child) {
        let job = handle();
        if job == 0 {
            return;
        }
        if let Some(process) = child.raw_handle() {
            // SAFETY: the handle belongs to a child we just spawned and is still open.
            unsafe {
                AssignProcessToJobObject(job, process as isize);
            }
        }
    }
}

/// Where the headless browser keeps its profile (cookies survive between runs).
pub fn profile_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("browser-profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_hides_headless() {
        let ua = "Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/141.0.0.0 Safari/537.36 HeadlessEdg/141.0.0.0";
        let out = normal_user_agent(ua);
        assert!(!out.contains("Headless"));
        assert!(out.contains("Chrome/141.0.0.0") && out.contains("Edg/141.0.0.0"));
    }

    #[test]
    fn addresses_become_urls_or_searches() {
        assert_eq!(normalize_url("https://serika.dev").unwrap(), "https://serika.dev");
        assert_eq!(normalize_url("serika.dev/docs").unwrap(), "https://serika.dev/docs");
        assert_eq!(
            normalize_url("rust lifetimes").unwrap(),
            "https://duckduckgo.com/?q=rust+lifetimes"
        );
        assert!(normalize_url("file:///etc/passwd").is_err());
        assert!(normalize_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn snapshot_text_lists_numbered_elements() {
        let snapshot = Snapshot {
            url: "https://example.com".into(),
            title: "Example".into(),
            text: "Hello".into(),
            elements: vec![Element {
                id: 1,
                tag: "a".into(),
                kind: None,
                label: "More".into(),
                href: Some("https://example.com/more".into()),
                placeholder: None,
            }],
            scroll_y: 0,
            scroll_height: 900,
        };
        let text = snapshot.to_model_text();
        assert!(text.contains("[1] a \"More\" -> https://example.com/more"));
        assert!(text.contains("Page text:\nHello"));
    }
}
