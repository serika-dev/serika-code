//! JSON-lines host for the Qt desktop. The CLI stays the only Rust interface.
//!
//! One JSON object per line on stdin, one per line on stdout. Requests carry an
//! `id` and get one response with the same `id`. Chat output is pushed as
//! `event` lines while a task runs, so a slow request never blocks streaming.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use serika_runtime::agents::ProviderStatus;
use serika_runtime::attachments::{self, Attachment};
use serika_runtime::browser::Frame;
use serika_runtime::i18n::{self, Key, Locale};
use serika_runtime::model::UsageWindow;
use serika_runtime::{AgentEvent, Config, PermissionMode, Runtime, StoredMessage, TaskRequest, ToolRun};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

struct Host {
    runtime: Arc<Runtime>,
    project: Mutex<PathBuf>,
    stdout: Mutex<std::io::Stdout>,
    cancel: Mutex<Option<CancellationToken>>,
    providers: Mutex<Option<Vec<ProviderStatus>>>,
}

/// The project to open first: the folder the app was started in when it is a git checkout,
/// otherwise the most recent project, otherwise the scratch workspace. Install folders and
/// system folders (where a shortcut starts the app) never count.
fn initial_project(runtime: &Runtime) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let lowered = cwd.to_string_lossy().to_ascii_lowercase();
    let system_dir = lowered.contains("programs\\serika code")
        || lowered.contains("appdata\\local")
        || lowered.ends_with("\\bin")
        || lowered.contains("windows\\system32");
    if !system_dir && cwd.join(".git").exists() {
        return cwd;
    }
    if let Ok(recents) = runtime.sessions.recent_projects() {
        if let Some(recent) = recents.into_iter().map(PathBuf::from).find(|p| p.is_dir()) {
            return recent;
        }
    }
    if !system_dir {
        return cwd;
    }
    let scratch = runtime.paths.scratch_dir();
    let _ = std::fs::create_dir_all(&scratch);
    scratch
}

pub async fn serve(runtime: Runtime) -> serika_runtime::Result<i32> {
    let project = initial_project(&runtime);
    let _ = runtime.sessions.remember_project(&project);
    let host = Arc::new(Host {
        runtime: Arc::new(runtime),
        project: Mutex::new(project),
        stdout: Mutex::new(std::io::stdout()),
        cancel: Mutex::new(None),
        providers: Mutex::new(None),
    });
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|err| serika_runtime::Error::msg(err.to_string()))?
    {
        // Some hosts (Windows PowerShell among them) open the pipe with a byte order mark.
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => {
                host.emit(&json!({"ok": false, "error": format!("invalid json: {err}")}));
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let host = host.clone();
        tokio::spawn(async move {
            let payload = match handle(host.clone(), &method, params).await {
                Ok(result) => json!({"id": id, "ok": true, "result": result}),
                Err(err) => json!({"id": id, "ok": false, "error": err}),
            };
            host.emit(&payload);
        });
    }
    if let Some(cancel) = host.cancel.lock().unwrap_or_else(|e| e.into_inner()).take() {
        cancel.cancel();
    }
    host.runtime.approvals.deny_all();
    host.runtime.browser.shutdown().await;
    Ok(0)
}

impl Host {
    fn emit(&self, value: &Value) {
        use std::io::Write;
        if let Ok(mut out) = self.stdout.lock() {
            let _ = writeln!(out, "{value}");
            let _ = out.flush();
        }
    }

    fn project(&self) -> PathBuf {
        self.project.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_project(&self, path: PathBuf) {
        // Store one spelling per folder: `//server/share/x`, `\\server\share\x` and a mapped
        // drive letter are the same place.
        let path = PathBuf::from(resolve_folder(&path.display().to_string()));
        let _ = self.runtime.sessions.remember_project(&path);
        *self.project.lock().unwrap_or_else(|e| e.into_inner()) = path;
    }
}

async fn handle(host: Arc<Host>, method: &str, params: Value) -> Result<Value, String> {
    let runtime = host.runtime.clone();
    match method {
        "auth.status" | "account" => account(&runtime).await,
        "auth.login" => {
            let outcome = runtime
                .login_password(
                    &text(&params, "email"),
                    &raw(&params, "password"),
                    optional_text(&params, "twoFactor").as_deref(),
                )
                .await
                .map_err(|err| err.to_string())?;
            login_result(&runtime, outcome).await
        }
        "auth.register" => {
            let outcome = runtime
                .register(
                    &text(&params, "username"),
                    &text(&params, "email"),
                    &raw(&params, "password"),
                    &text(&params, "dateOfBirth"),
                )
                .await
                .map_err(|err| err.to_string())?;
            login_result(&runtime, outcome).await
        }
        "auth.magic" => {
            let message = runtime
                .request_magic_link(&text(&params, "email"))
                .await
                .map_err(|err| err.to_string())?;
            Ok(json!({"message": message}))
        }
        "auth.magicConsume" => {
            let outcome = runtime
                .consume_magic_link(
                    &text(&params, "token"),
                    optional_text(&params, "twoFactor").as_deref(),
                )
                .await
                .map_err(|err| err.to_string())?;
            login_result(&runtime, outcome).await
        }
        "auth.logout" => {
            runtime.logout().await.map_err(|err| err.to_string())?;
            Ok(json!({"signedIn": false}))
        }
        "auth.background" => background(&runtime).await,
        "settings.get" => Ok(settings(&host)),
        "settings.set" => {
            set_setting(&host, &text(&params, "key"), &raw(&params, "value"))?;
            Ok(settings(&host))
        }
        "ui.get" => Ok(ui_prefs(&runtime.paths.data_dir)),
        "ui.set" => {
            set_ui_pref(&runtime.paths.data_dir, &text(&params, "key"), params.get("value"))?;
            Ok(ui_prefs(&runtime.paths.data_dir))
        }
        "i18n" => Ok(dictionary(&text(&params, "locale"))),
        "models" => models(&host).await,
        "providers.status" => {
            let refresh = params.get("refresh").and_then(|v| v.as_bool()).unwrap_or(false);
            let statuses = runtime.provider_statuses(refresh).await;
            let value = providers_json(&statuses);
            *host.providers.lock().unwrap_or_else(|e| e.into_inner()) = Some(statuses);
            Ok(value)
        }
        "providers.probe" => {
            let usage = runtime.probe_claude_usage().await.map_err(|err| err.to_string())?;
            Ok(serde_json::to_value(usage).unwrap_or(Value::Null))
        }
        "projects.list" => Ok(projects(&host)),
        "projects.remove" => {
            let wanted = text(&params, "path");
            // The list may hold the same folder under more than one spelling.
            let wanted_key = canon_key(&wanted);
            for (known, _) in runtime.sessions.projects().unwrap_or_default() {
                if canon_key(&known) == wanted_key {
                    runtime
                        .sessions
                        .forget_project(&PathBuf::from(known))
                        .map_err(|err| err.to_string())?;
                }
            }
            Ok(projects(&host))
        }
        "project.set" => {
            let path = PathBuf::from(text(&params, "path"));
            if !path.is_dir() {
                return Err(format!("{} is not a folder", path.display()));
            }
            host.set_project(path);
            Ok(settings(&host))
        }
        "sessions.list" => sessions(&host),
        "sessions.open" => open_session(&host, &text(&params, "id")),
        "sessions.children" => {
            let children = runtime
                .sessions
                .children(&text(&params, "id"))
                .map_err(|err| err.to_string())?;
            let list: Vec<Value> = children
                .iter()
                .map(|child| {
                    let messages = runtime.sessions.messages(&child.id).unwrap_or_default();
                    let task = messages
                        .iter()
                        .find(|m| m.role == "user")
                        .map(|m| m.content.split("\n\nYou are a sub-agent").next().unwrap_or("").to_string())
                        .unwrap_or_default();
                    let summary = messages
                        .iter()
                        .rev()
                        .find(|m| m.role == "assistant")
                        .map(|m| serika_runtime::protocol::extract_tool_calls(&m.content).0.trim().to_string())
                        .unwrap_or_default();
                    let tools = runtime.sessions.tool_runs(&child.id).map(|t| t.len()).unwrap_or(0);
                    json!({
                        "id": child.id, "name": child.title, "model": child.model_id, "task": task,
                        "summary": summary, "tools": tools, "status": if summary.is_empty() { "failed" } else { "done" },
                        "updatedAt": child.updated_at,
                    })
                })
                .collect();
            Ok(json!({"agents": list}))
        }
        "sessions.rename" => {
            runtime
                .sessions
                .rename(&text(&params, "id"), &text(&params, "title"))
                .map_err(|err| err.to_string())?;
            Ok(json!({"renamed": true}))
        }
        "sessions.delete" => {
            runtime
                .sessions
                .delete(&text(&params, "id"))
                .map_err(|err| err.to_string())?;
            Ok(json!({"deleted": true}))
        }
        "sessions.rewind" => {
            let id = text(&params, "id");
            let index = number(&params, "index").max(0.0) as usize;
            let message = runtime
                .sessions
                .user_turn_id(&id, index)
                .map_err(|err| err.to_string())?;
            let outcome = runtime
                .sessions
                .rewind(&id, message)
                .map_err(|err| err.to_string())?;
            let files = attachments::parse_json(outcome.attachments.as_deref());
            Ok(json!({
                "prompt": outcome.prompt,
                "attachments": files.iter().map(attachment_ui).collect::<Vec<_>>(),
                "restored": outcome.restored_files,
                "session": open_session(&host, &id)?,
            }))
        }
        "sessions.deleteTurn" => {
            let id = text(&params, "id");
            let index = number(&params, "index").max(0.0) as usize;
            runtime
                .sessions
                .delete_turn(&id, index)
                .map_err(|err| err.to_string())?;
            open_session(&host, &id)
        }
        "sessions.fork" => {
            let id = text(&params, "id");
            let index = number(&params, "index").max(0.0) as usize;
            let new_id = serika_runtime::sessions::new_id();
            runtime
                .sessions
                .fork(&id, index, &new_id)
                .map_err(|err| err.to_string())?;
            open_session(&host, &new_id)
        }
        "chat.send" => {
            spawn_chat(host, &params)?;
            Ok(json!({"accepted": true}))
        }
        "chat.cancel" => {
            if let Some(cancel) = host.cancel.lock().unwrap_or_else(|e| e.into_inner()).take() {
                cancel.cancel();
            }
            runtime.approvals.deny_all();
            Ok(json!({"cancelled": true}))
        }
        "approval.resolve" => {
            let allow = params.get("allow").and_then(|item| item.as_bool()).unwrap_or(false);
            let found = runtime.approvals.resolve(&text(&params, "id"), allow);
            Ok(json!({"resolved": found}))
        }
        "browser.open" => frame_json(runtime.browser.open(&text(&params, "url")).await.map(|(_, f)| f)),
        "browser.back" => frame_json(runtime.browser.back().await.map(|(_, f)| f)),
        "browser.forward" => frame_json(runtime.browser.forward().await.map(|(_, f)| f)),
        "browser.reload" => frame_json(runtime.browser.reload().await.map(|(_, f)| f)),
        "browser.frame" => match runtime.browser.frame_if_running().await {
            Ok(Some(frame)) => Ok(frame_value(&frame)),
            Ok(None) => Ok(json!({"running": false})),
            Err(err) => Err(err.to_string()),
        },
        "browser.newTab" => frame_json(
            runtime
                .browser
                .new_tab(optional_text(&params, "url").as_deref())
                .await
                .map(|(_, f)| f),
        ),
        "browser.switchTab" => frame_json(
            runtime.browser.switch_tab(&text(&params, "id")).await.map(|(_, f)| f),
        ),
        "browser.closeTab" => frame_json(runtime.browser.close_tab(&text(&params, "id")).await),
        "browser.resize" => {
            let width = number(&params, "width").round().max(0.0) as u32;
            let height = number(&params, "height").round().max(0.0) as u32;
            match runtime.browser.resize(width, height).await {
                Ok(Some(frame)) => Ok(frame_value(&frame)),
                Ok(None) => Ok(json!({"unchanged": true})),
                Err(err) => Err(err.to_string()),
            }
        }
        "browser.click" => frame_json(
            runtime
                .browser
                .click_at(number(&params, "x"), number(&params, "y"))
                .await,
        ),
        "browser.wheel" => frame_json(
            runtime
                .browser
                .wheel_at(number(&params, "x"), number(&params, "y"), number(&params, "dy"))
                .await,
        ),
        "browser.input" => frame_json(
            runtime
                .browser
                .input(
                    optional_text(&params, "key").as_deref(),
                    Some(raw(&params, "text").as_str()),
                )
                .await,
        ),
        "browser.close" => {
            runtime.browser.shutdown().await;
            Ok(json!({"closed": true}))
        }
        "attachments.add" => {
            let sources: Vec<PathBuf> = params
                .get("paths")
                .and_then(|item| item.as_array())
                .map(|list| list.iter().filter_map(|p| p.as_str()).map(PathBuf::from).collect())
                .unwrap_or_default();
            let existing = parse_attachments(&params, "existing");
            let (staged, errors) = attachments::stage(&sources, &attachments_dir(&runtime), &existing);
            Ok(json!({
                "attachments": staged.iter().map(attachment_ui).collect::<Vec<_>>(),
                "errors": errors,
            }))
        }
        "update.check" => {
            let info = serika_runtime::updater::check_for_update()
                .await
                .map_err(|err| err.to_string())?;
            Ok(json!({
                "current": info.current_version,
                "latest": info.latest_version,
                "available": info.update_available,
                "url": info.release_url,
                "notes": info.notes,
            }))
        }
        other => Err(format!("unknown method {other}")),
    }
}

fn attachments_dir(runtime: &Runtime) -> PathBuf {
    runtime.paths.data_dir.join("attachments")
}

fn file_url(path: &str) -> String {
    format!("file:///{}", plain(path).replace('\\', "/"))
}

fn attachment_ui(item: &Attachment) -> Value {
    json!({
        "name": item.name,
        "kind": item.kind.as_str(),
        "size": item.size,
        "path": item.path,
        "lang": item.lang,
        "fileUrl": file_url(&item.path),
    })
}

fn parse_attachments(params: &Value, key: &str) -> Vec<Attachment> {
    params
        .get(key)
        .and_then(|item| item.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| serde_json::from_value::<Attachment>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn frame_json(frame: serika_runtime::Result<Frame>) -> Result<Value, String> {
    let frame = frame.map_err(|err| err.to_string())?;
    Ok(frame_value(&frame))
}

fn frame_value(frame: &Frame) -> Value {
    json!({
        "url": frame.url,
        "title": frame.title,
        "image": frame.image,
        "width": frame.width,
        "height": frame.height,
        "canGoBack": frame.can_go_back,
        "canGoForward": frame.can_go_forward,
        "loading": frame.loading,
        "running": true,
        "activeTab": frame.active_tab,
        "tabs": frame.tabs.iter().map(|tab| json!({
            "id": tab.id,
            "title": tab.title,
            "url": tab.url,
            "active": tab.active,
        })).collect::<Vec<_>>(),
    })
}

fn number(params: &Value, key: &str) -> f64 {
    params.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn selected_model(config: &Config) -> String {
    config
        .model
        .clone()
        .filter(|model| {
            !model.is_empty() && !model.starts_with("local/") && !model.starts_with("cli/")
        })
        .unwrap_or_else(|| "openrouter/free".to_string())
}

fn project_name(runtime: &Runtime, path: &Path) -> String {
    if runtime.paths.scratch_dir() == path {
        return "Scratch".to_string();
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn settings(host: &Host) -> Value {
    let paths = &host.runtime.paths;
    let config = Config::load_or_create(paths).unwrap_or_else(|_| host.runtime.config.clone());
    let project = host.project();
    let browser = host
        .runtime
        .browser
        .detected_binary()
        .map(|p| p.display().to_string());
    json!({
        "locale": config.locale,
        "theme": config.theme,
        "model": selected_model(&config),
        "project": plain(&project.display().to_string()),
        "projectName": project_name(&host.runtime, &project),
        "version": serika_runtime::client::VERSION,
        "permission": config.permission.as_str(),
        "commandTimeout": config.command_timeout_secs,
        "maxToolRounds": config.max_tool_rounds,
        "retentionDays": config.session_retention_days,
        "stream": config.stream,
        "claudePath": config.claude_path.clone().unwrap_or_default(),
        "claudeConfigDir": config.claude_config_dir.clone().unwrap_or_default(),
        "codexPath": config.codex_path.clone().unwrap_or_default(),
        "codexHome": config.codex_home.clone().unwrap_or_default(),
        "claudeEffort": config.claude_effort.clone().unwrap_or_default(),
        "codexEffort": config.codex_effort.clone().unwrap_or_default(),
        "browserPath": config.browser_path.clone().unwrap_or_default(),
        "browserEnabled": config.browser_enabled,
        "browserHome": config.browser_home,
        "browserDetected": browser,
        "configFile": paths.config_file.display().to_string(),
        "dataDir": paths.data_dir.display().to_string(),
        "logsDir": paths.logs_dir.display().to_string(),
        "database": paths.database.display().to_string(),
    })
}

fn set_setting(host: &Host, key: &str, value: &str) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "locale",
        "theme",
        "model",
        "permission",
        "command_timeout_secs",
        "max_tool_rounds",
        "session_retention_days",
        "stream",
        "claude_path",
        "claude_config_dir",
        "codex_path",
        "codex_home",
        "claude_effort",
        "codex_effort",
        "browser_path",
        "browser_enabled",
        "browser_home",
    ];
    if !ALLOWED.contains(&key) {
        return Err(format!("setting {key} cannot be changed here"));
    }
    let mut config = Config::load_or_create(&host.runtime.paths).map_err(|err| err.to_string())?;
    config.set_field(key, value).map_err(|err| err.to_string())?;
    config
        .save(&host.runtime.paths.config_file)
        .map_err(|err| err.to_string())
}

fn ui_file(data_dir: &Path) -> PathBuf {
    data_dir.join("ui.json")
}

/// Appearance and behavior preferences that only the interface uses.
fn ui_prefs(data_dir: &Path) -> Value {
    std::fs::read_to_string(ui_file(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({}))
}

fn set_ui_pref(data_dir: &Path, key: &str, value: Option<&Value>) -> Result<(), String> {
    if key.is_empty() || key.len() > 48 {
        return Err("invalid preference name".into());
    }
    let mut prefs = ui_prefs(data_dir);
    let map = prefs.as_object_mut().ok_or("preferences are corrupt")?;
    match value {
        Some(Value::Null) | None => {
            map.remove(key);
        }
        Some(value @ (Value::String(_) | Value::Number(_) | Value::Bool(_))) => {
            if map.len() >= 80 && !map.contains_key(key) {
                return Err("too many preferences".into());
            }
            map.insert(key.to_string(), value.clone());
        }
        Some(_) => return Err("preferences hold text, numbers, and switches".into()),
    }
    std::fs::create_dir_all(data_dir).map_err(|err| err.to_string())?;
    std::fs::write(
        ui_file(data_dir),
        serde_json::to_string_pretty(&prefs).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn dictionary(locale: &str) -> Value {
    let locale = Locale::parse(locale);
    let mut strings = Map::new();
    for key in Key::ALL {
        strings.insert(key.id(), Value::String(i18n::t(locale, *key).to_string()));
    }
    let locales: Vec<Value> = Locale::ALL
        .iter()
        .map(|item| json!({"code": item.code(), "name": item.native_name()}))
        .collect();
    json!({"locale": locale.code(), "strings": strings, "locales": locales})
}

fn permission_mode(host: &Host, params: &Value) -> PermissionMode {
    optional_text(params, "permission")
        .and_then(|value| PermissionMode::parse(&value))
        .unwrap_or(host.runtime.config.permission)
}

fn spawn_chat(host: Arc<Host>, params: &Value) -> Result<(), String> {
    let mut prompt = text(params, "prompt");
    let files = parse_attachments(params, "attachments");
    attachments::verify(&files, &attachments_dir(&host.runtime)).map_err(|err| err.to_string())?;
    if prompt.is_empty() {
        if files.is_empty() {
            return Err("the message is empty".into());
        }
        prompt = "Please take a look at the attached file(s).".to_string();
    }
    let model = optional_text(params, "model");
    let session_id = optional_text(params, "sessionId");
    let plan = text(params, "mode") == "plan";
    let yes = params.get("yes").and_then(|item| item.as_bool()).unwrap_or(false);
    let prompt = if plan {
        format!(
            "[PLAN MODE] Inspect the relevant files and produce a clear, step-by-step engineering plan before making edits:\n\n{prompt}"
        )
    } else {
        prompt
    };

    let cancel = CancellationToken::new();
    *host.cancel.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let forward = host.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Some(value) = event_json(event) {
                forward.emit(&value);
            }
        }
    });

    let permission = if plan {
        PermissionMode::ReadOnly
    } else {
        permission_mode(&host, params)
    };
    let request = TaskRequest {
        project: host.project(),
        prompt,
        session_id,
        model,
        permission: Some(permission),
        yes,
        events: tx,
        cancel,
        attachments: files,
    };
    tokio::spawn(async move {
        let outcome = host.runtime.run_task(request).await;
        match outcome {
            Ok(done) => host.emit(&json!({
                "event": "completed",
                "sessionId": done.session_id,
                "summary": done.summary,
                "finished": done.finished,
            })),
            Err(serika_runtime::Error::Cancelled) => host.emit(&json!({"event": "cancelled"})),
            Err(err) => host.emit(&json!({"event": "failed", "message": err.to_string()})),
        }
    });
    Ok(())
}

fn event_json(event: AgentEvent) -> Option<Value> {
    let value = match event {
        AgentEvent::Started { session_id } => json!({"event": "started", "sessionId": session_id}),
        AgentEvent::Status(text) => json!({"event": "status", "text": text}),
        AgentEvent::MessageChunk { text } => json!({"event": "chunk", "text": text}),
        AgentEvent::ReasoningChunk { text } => json!({"event": "reasoning", "text": text}),
        AgentEvent::ToolRequested { id, name, arguments } => json!({
            "event": "tool", "phase": "requested", "id": id, "name": name, "arguments": arguments
        }),
        AgentEvent::ToolStarted { id, name } => {
            json!({"event": "tool", "phase": "start", "id": id, "name": name})
        }
        AgentEvent::ToolOutput { id, stream } => {
            json!({"event": "toolOutput", "id": id, "text": stream})
        }
        AgentEvent::ToolFinished { id, name, ok, summary } => json!({
            "event": "tool", "phase": "done", "id": id, "name": name, "ok": ok, "summary": summary
        }),
        AgentEvent::ApprovalRequired { id, title, detail, destructive } => json!({
            "event": "approval", "id": id, "title": title, "detail": detail, "destructive": destructive
        }),
        AgentEvent::FileChanged(diff) => {
            json!({"event": "diff", "path": diff.path, "diff": diff.diff})
        }
        AgentEvent::Browser { frame } => {
            let mut value = frame_value(&frame);
            value["event"] = json!("browser");
            value["agent"] = json!(true);
            value
        }
        AgentEvent::Handoff { from, to, messages } => json!({
            "event": "handoff", "from": from, "to": to, "messages": messages
        }),
        AgentEvent::Subagent { id, name, task, model, status, activity, tools, summary } => json!({
            "event": "subagent", "id": id, "name": name, "task": task, "model": model,
            "status": status, "activity": activity, "tools": tools, "summary": summary
        }),
        AgentEvent::ProviderUsage { provider, usage } => json!({
            "event": "providerUsage", "provider": provider,
            "usage": serde_json::to_value(usage).unwrap_or(Value::Null)
        }),
        AgentEvent::UsageUpdated(usage) => json!({"event": "usage", "summary": usage.summary_line()}),
        // The task result line carries completion, failure, and cancellation exactly once.
        AgentEvent::MessageCompleted { .. }
        | AgentEvent::Completed { .. }
        | AgentEvent::Failed { .. }
        | AgentEvent::Cancelled => return None,
    };
    Some(value)
}

async fn login_result(
    runtime: &Runtime,
    outcome: serika_runtime::LoginOutcome,
) -> Result<Value, String> {
    match outcome {
        serika_runtime::LoginOutcome::SignedIn(_) => account(runtime).await,
        serika_runtime::LoginOutcome::NeedsSecondFactor { message } => {
            Ok(json!({"needsSecondFactor": true, "message": message}))
        }
    }
}

/// Share of the window that is used. The exact ratio wins over the API's own percent, which
/// is rounded to a whole number and shows 0 for anything under half a percent.
fn percent(window: Option<&UsageWindow>) -> f64 {
    let Some(window) = window else { return 0.0 };
    match (window.used, window.limit) {
        (Some(used), Some(limit)) if limit > 0 => used as f64 / limit as f64 * 100.0,
        _ => window.percent.unwrap_or(0.0),
    }
    .clamp(0.0, 100.0)
}

async fn account(runtime: &Runtime) -> Result<Value, String> {
    let Some(session) = runtime.signed_in().map_err(|err| err.to_string())? else {
        return Ok(json!({"signedIn": false}));
    };
    // The profile request can fail right after start-up. Retry once, then fall back to the last
    // picture we saw, so the avatar never vanishes because of one bad request.
    let mut profile = runtime.account().await.ok();
    if profile.is_none() {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        profile = runtime.account().await.ok();
    }
    let cache_path = runtime.paths.data_dir.join("account-cache.json");
    let cached: Value = std::fs::read_to_string(&cache_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(|value: &Value| {
            value.get("username").and_then(|u| u.as_str()) == Some(session.username.as_str())
        })
        .unwrap_or(Value::Null);
    let live_avatar = profile
        .as_ref()
        .and_then(|item| item.avatar.clone())
        .and_then(|url| serika_runtime::model::absolute_avatar_url(&url));
    let avatar = live_avatar
        .clone()
        .or_else(|| cached.get("avatar").and_then(|a| a.as_str()).map(str::to_string));
    if let Some(profile) = &profile {
        let _ = std::fs::write(
            &cache_path,
            json!({
                "username": session.username,
                "avatar": live_avatar,
                "displayName": profile.display_name,
            })
            .to_string(),
        );
    }
    let usage = runtime.usage().await.ok();
    let plan_block = usage.as_ref().and_then(|item| item.plan.as_ref());
    let is_premium = profile
        .as_ref()
        .map(|item| item.is_premium)
        .unwrap_or(session.is_premium)
        || plan_block.and_then(|plan| plan.is_premium).unwrap_or(false);
    let plan = plan_block
        .and_then(|plan| plan.tier.as_ref())
        .and_then(|tier| tier.name.clone())
        .filter(|name| !name.trim().is_empty())
        .or_else(|| profile.as_ref().and_then(|item| item.subscription_plan.clone()))
        .unwrap_or_else(|| {
            if is_premium {
                "Serika+".to_string()
            } else {
                String::new()
            }
        });
    let session_window = plan_block.and_then(|plan| plan.session.as_ref());
    let weekly_window = plan_block.and_then(|plan| plan.weekly.as_ref());
    Ok(json!({
        "signedIn": true,
        "username": profile.as_ref().map(|item| item.username.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| session.username.clone()),
        "email": profile.as_ref().map(|item| item.email.clone()).unwrap_or_else(|| session.email.clone()),
        "displayName": profile.as_ref().and_then(|item| item.display_name.clone()),
        "avatar": avatar,
        "isPremium": is_premium,
        "plan": plan,
        "periodEnd": profile.as_ref().and_then(|item| item.subscription_period_end.clone()),
        "sessionPercent": percent(session_window),
        "sessionUsed": session_window.and_then(|item| item.used),
        "sessionLimit": session_window.and_then(|item| item.limit),
        "weeklyUsed": weekly_window.and_then(|item| item.used),
        "weeklyLimit": weekly_window.and_then(|item| item.limit),
        "sessionResetsAt": session_window.and_then(|item| item.resets_at.clone()),
        "weeklyPercent": percent(weekly_window),
        "weeklyResetsAt": weekly_window.and_then(|item| item.resets_at.clone()),
        "usage": usage.as_ref().map(|item| item.summary_line()).unwrap_or_default(),
    }))
}

fn providers_json(statuses: &[ProviderStatus]) -> Value {
    Value::Array(
        statuses
            .iter()
            .map(|status| serde_json::to_value(status).unwrap_or(Value::Null))
            .collect(),
    )
}

async fn models(host: &Host) -> Result<Value, String> {
    let runtime = &host.runtime;
    let catalog = runtime.models().await.map_err(|err| err.to_string())?;
    let config = Config::load_or_create(&runtime.paths).unwrap_or_else(|_| runtime.config.clone());
    let cached = host.providers.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let statuses = match cached {
        Some(list) => list,
        None => {
            let list = runtime.provider_statuses(false).await;
            *host.providers.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
            list
        }
    };
    Ok(json!({
        "selected": selected_model(&config),
        "free": catalog.free,
        "premium": catalog.premium,
        "providers": providers_json(&statuses),
    }))
}

fn projects(host: &Host) -> Value {
    let runtime = &host.runtime;
    let current = host.project();
    let current_key = canon_key(&current.display().to_string());
    let all = runtime.sessions.list(None).unwrap_or_default();

    // Merge every spelling of the same folder into one row.
    struct Row {
        path: String,
        last: i64,
        count: usize,
    }
    let mut order: Vec<String> = Vec::new();
    let mut rows: std::collections::HashMap<String, Row> = std::collections::HashMap::new();
    let mut add = |path: &str, last: i64| {
        let key = canon_key(path);
        let shown = resolve_folder(path);
        let entry = rows.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Row { path: shown, last: 0, count: 0 }
        });
        entry.last = entry.last.max(last);
    };
    for (path, last) in runtime.sessions.projects().unwrap_or_default() {
        add(&path, last);
    }
    add(&current.display().to_string(), 0);
    for session in &all {
        let key = canon_key(&session.project_path);
        if let Some(row) = rows.get_mut(&key) {
            row.count += 1;
        }
    }
    let mut list: Vec<(&String, &Row)> = order.iter().filter_map(|k| rows.get(k).map(|r| (k, r))).collect();
    list.sort_by(|a, b| b.1.last.cmp(&a.1.last));
    let out: Vec<Value> = list
        .into_iter()
        .map(|(key, row)| {
            let buf = PathBuf::from(&row.path);
            json!({
                "key": key,
                "path": row.path,
                "name": project_name(runtime, &buf),
                "exists": buf.is_dir(),
                "lastUsed": row.last,
                "sessionCount": row.count,
                "current": *key == current_key,
            })
        })
        .collect();
    json!({"projects": out})
}
/// Windows can hand back `\\?\C:\...` or `\\?\UNC\server\share`. Show and compare the plain form.
fn plain(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Resolve a folder to one stable spelling. Network paths are only normalized as text: asking
/// the file system about an unreachable share can block for many seconds.
fn resolve_folder(path: &str) -> String {
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    let shown = plain(path);
    let normalized = shown.replace('/', "\\");
    let is_network = normalized.starts_with("\\\\");
    if is_network {
        return normalized;
    }
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(found) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&shown) {
        return found.clone();
    }
    let resolved = std::fs::canonicalize(&shown)
        .map(|found| plain(&found.display().to_string()))
        .unwrap_or(shown.clone());
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(shown, resolved.clone());
    resolved
}

/// One key per folder, whatever way it was written down.
fn canon_key(path: &str) -> String {
    let resolved = resolve_folder(path);
    if cfg!(windows) {
        resolved
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    } else {
        resolved.trim_end_matches('/').to_string()
    }
}

fn sessions(host: &Host) -> Result<Value, String> {
    let list = host
        .runtime
        .sessions
        .list(None)
        .map_err(|err| err.to_string())?;
    let rows: Vec<Value> = list
        .into_iter()
        .map(|item| {
            json!({
                "id": item.id,
                "title": item.title,
                "model": item.model_id,
                "updatedAt": item.updated_at,
                "project": plain(&item.project_path),
                "key": canon_key(&item.project_path),
            })
        })
        .collect();
    Ok(json!({"sessions": rows, "current": plain(&host.project().display().to_string())}))
}

/// Opening a chat from another folder switches to that folder, so the next message
/// runs where the chat belongs.
fn open_session(host: &Host, id: &str) -> Result<Value, String> {
    let runtime = &host.runtime;
    let summary = runtime
        .sessions
        .get(id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "that chat no longer exists".to_string())?;
    let folder = PathBuf::from(&summary.project_path);
    if !folder.is_dir() {
        return Err(format!(
            "the folder for this chat is missing: {}",
            summary.project_path
        ));
    }
    host.set_project(folder);
    let messages = runtime.sessions.messages(id).map_err(|err| err.to_string())?;
    let tools = runtime.sessions.tool_runs(id).unwrap_or_default();
    let diffs = runtime.sessions.changes(id).unwrap_or_default();
    Ok(json!({
        "id": id,
        "model": summary.model_id,
        "project": summary.project_path,
        "settings": settings(host),
        "turns": build_turns(&messages, &tools),
        "diffs": diffs
            .into_iter()
            .map(|(path, diff)| json!({"path": path, "diff": diff}))
            .collect::<Vec<_>>(),
    }))
}

/// Tool results are stored as user messages and one reply can span several model
/// rounds. The desktop shows the chat the way the user lived it: their prompts and
/// one assistant turn per prompt, with the tools that ran during that turn.
fn build_turns(messages: &[StoredMessage], tools: &[ToolRun]) -> Vec<Value> {
    let mut turns: Vec<Value> = Vec::new();
    let mut last_model: Option<String> = None;
    for message in messages {
        if message.role == "user" && message.content.starts_with("Tool results:\n") {
            continue;
        }
        if message.role == "assistant" {
            let (visible, _) = serika_runtime::protocol::extract_tool_calls(&message.content);
            let reasoning = message.reasoning.clone().unwrap_or_default();
            if let Some(last) = turns
                .last_mut()
                .filter(|turn| turn.get("role").and_then(|r| r.as_str()) == Some("assistant"))
            {
                join_field(last, "content", visible.trim());
                join_field(last, "reasoning", reasoning.trim());
                continue;
            }
            // A reply from a different model than the previous one marks a switch.
            if let Some(model) = message.model.clone().filter(|m| !m.is_empty()) {
                if let Some(previous) = last_model.as_ref().filter(|p| **p != model) {
                    // Between your message and the reply, as it appeared live.
                    turns.push(json!({"role": "switch", "from": previous, "to": model, "content": ""}));
                }
                last_model = Some(model);
            }
            turns.push(json!({
                "role": "assistant",
                "content": visible.trim(),
                "reasoning": reasoning.trim(),
                "createdAt": message.created_at,
                "model": message.model.clone().unwrap_or_default(),
                "tools": [],
            }));
        } else {
            let files = attachments::parse_json(message.attachments.as_deref());
            turns.push(json!({
                "role": message.role,
                "content": message.content,
                "createdAt": message.created_at,
                "attachments": files.iter().map(attachment_ui).collect::<Vec<_>>(),
            }));
        }
    }
    for tool in tools {
        let owner = turns.iter_mut().rev().find(|turn| {
            turn.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && turn.get("createdAt").and_then(|t| t.as_i64()).unwrap_or(0) <= tool.created_at
        });
        if let Some(turn) = owner {
            if let Some(list) = turn.get_mut("tools").and_then(|t| t.as_array_mut()) {
                list.push(json!({
                    "name": tool.name,
                    "state": if tool.ok { "ok" } else { "failed" },
                    "summary": tool_detail(&tool.arguments),
                    "result": first_line(&tool.output),
                    "output": clip(&tool.output, 6000),
                }));
            }
        }
    }
    turns
}

fn join_field(turn: &mut Value, field: &str, addition: &str) {
    if addition.is_empty() {
        return;
    }
    let current = turn.get(field).and_then(|v| v.as_str()).unwrap_or("").to_string();
    if current.is_empty() {
        turn[field] = Value::String(addition.to_string());
    } else if !current.contains(addition) {
        turn[field] = Value::String(format!("{current}\n\n{addition}"));
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

fn first_line(text: &str) -> String {
    clip(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim(), 160)
}

/// The path, command, or query a tool call was about.
fn tool_detail(arguments: &str) -> String {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    for key in [
        "path", "file_path", "command", "query", "pattern", "url", "from", "text", "key",
    ] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return clip(text, 140);
            }
        }
    }
    if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
        return format!("element {id}");
    }
    String::new()
}

async fn background(runtime: &Runtime) -> Result<Value, String> {
    match runtime.client.random_auth_background().await {
        Ok((artist, url)) => Ok(json!({"artist": artist, "url": url})),
        Err(_) => Ok(json!({"artist": "", "url": ""})),
    }
}

fn raw(params: &Value, key: &str) -> String {
    params
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

fn text(params: &Value, key: &str) -> String {
    raw(params, key).trim().to_string()
}

fn optional_text(params: &Value, key: &str) -> Option<String> {
    let value = text(params, key);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
