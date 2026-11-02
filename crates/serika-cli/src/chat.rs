//! The interactive `serika` session: banner, input box, slash commands, and turns
//! that keep listening to the keyboard (Esc interrupts, Enter queues the next message).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use futures_util::StreamExt;
use serika_runtime::agents::{self, CliAgent, ProviderStatus};
use serika_runtime::attachments::{self, Attachment, AttachmentKind};
use serika_runtime::providers::ProviderKind;
use serika_runtime::{AgentEvent, Error, PermissionMode, Runtime, TaskRequest};
use tokio_util::sync::CancellationToken;

use crate::editor::{self, Editor, Input, PickItem, Prompt, RawGuard};
use crate::tui::{self, Banner, Term, TurnRenderer, AMBER, BOLD, DIM, GREEN, RED, RESET, VIOLET_LIGHT};
use crate::outln;

const COMPACT_PROMPT: &str = "Summarise our conversation so far for a fresh session: the goal, decisions made, \
files touched and why, what is done, what is left, and anything I asked you to remember. Be complete but brief. \
Do not use tools.";

struct Queued {
    text: String,
    attachments: Vec<Attachment>,
}

enum TurnEnd {
    Completed(String),
    Failed,
    Cancelled,
}

enum Flow {
    Continue,
    Exit,
}

pub struct Chat<'a> {
    runtime: &'a Runtime,
    project: PathBuf,
    term: Term,
    model: String,
    permission: PermissionMode,
    yes: bool,
    session: Option<String>,
    queue: VecDeque<Queued>,
    attachments: Vec<Attachment>,
    carry: Option<String>,
    names: HashMap<String, String>,
    usage_note: String,
    editor: Editor,
}

pub async fn run(
    runtime: &Runtime,
    project: &Path,
    model: Option<String>,
    permission: Option<PermissionMode>,
    yes: bool,
    session: Option<String>,
    term: Term,
) -> serika_runtime::Result<i32> {
    let config = runtime.live_config();
    if let Some(theme) = load_cli_prefs(runtime).get("theme").and_then(|t| t.as_str()) {
        tui::set_theme(theme);
    }
    let model = model.or(config.model.clone()).unwrap_or_else(|| "local/workspace".into());
    let mut chat = Chat {
        runtime,
        project: project.to_path_buf(),
        term,
        model,
        permission: permission.unwrap_or(config.permission),
        yes,
        session,
        queue: VecDeque::new(),
        attachments: Vec::new(),
        carry: None,
        names: HashMap::new(),
        usage_note: String::new(),
        editor: Editor::new(&runtime.paths.data_dir, project, config.context_exclusions.clone()),
    };
    chat.learn_cached_names();
    chat.refresh_usage().await;
    chat.banner();
    chat.run_loop().await
}

impl<'a> Chat<'a> {
    async fn run_loop(&mut self) -> serika_runtime::Result<i32> {
        loop {
            let prompt = self.prompt();
            let input = tokio::task::block_in_place(|| self.editor.read(&self.term, &prompt));
            match input {
                Input::Exit => break,
                Input::ClearScreen => {
                    tui::clear_screen(&self.term);
                    self.banner();
                }
                Input::Expand => tui::print_expanded(&self.term),
                Input::Submit(text) => {
                    let trimmed = text.trim().to_string();
                    if trimmed.is_empty() {
                        // Enter on an empty box resumes a held queue.
                        if let Some(next) = self.queue.pop_front() {
                            self.send(next.text, next.attachments).await?;
                        }
                        continue;
                    }
                    if trimmed.starts_with('/') && !trimmed.starts_with("//") {
                        match self.command(&trimmed).await? {
                            Flow::Exit => break,
                            Flow::Continue => continue,
                        }
                    }
                    let files = std::mem::take(&mut self.attachments);
                    self.send(text.trim_end().to_string(), files).await?;
                }
            }
        }
        if self.term.fancy {
            outln!("  {DIM}Bye. Resume this chat with{RESET} {VIOLET_LIGHT}serika resume{RESET}");
        }
        Ok(0)
    }

    // ---------------------------------------------------------------- display

    fn provider_label(&self, model: &str) -> String {
        match CliAgent::from_model(model) {
            Some(CliAgent::Claude) => "Claude Code · your subscription".into(),
            Some(CliAgent::Codex) => "Codex · your subscription".into(),
            None => match ProviderKind::from_model_id(model) {
                ProviderKind::Local => "local".into(),
                ProviderKind::Serika => "Serika Cloud".into(),
                other => format!("{} API", other.label()),
            },
        }
    }

    fn model_name(&self, model: &str) -> String {
        if let Some(name) = self.names.get(model) {
            return name.clone();
        }
        match CliAgent::from_model(model) {
            Some(agent) => {
                let rest = agent.model_arg(model).unwrap_or_else(|| "default".into());
                format!("{} {}", agent.label(), rest)
            }
            None => model.to_string(),
        }
    }

    fn learn_cached_names(&mut self) {
        for agent in [CliAgent::Claude, CliAgent::Codex] {
            for m in agents::cached_models(&self.runtime.paths.data_dir, agent) {
                let name = if m.name.starts_with("Default") {
                    format!("{} (default)", agent.label())
                } else {
                    m.name.clone()
                };
                self.names.insert(m.id, name);
            }
        }
    }

    fn account_label(&self) -> String {
        let serika = self
            .runtime
            .signed_in()
            .ok()
            .flatten()
            .map(|s| format!("{} on Serika", s.username))
            .unwrap_or_else(|| "not signed in to Serika (/login)".into());
        match CliAgent::from_model(&self.model) {
            Some(agent) => format!("{} login · {serika}", agent.label()),
            None => serika,
        }
    }

    fn permission_label(mode: PermissionMode) -> &'static str {
        match mode {
            PermissionMode::ReadOnly => "Read-only",
            PermissionMode::Standard => "Auto-accept edits",
            PermissionMode::Restricted => "Supervised",
            PermissionMode::Full => "Full access",
        }
    }

    fn branch(&self) -> Option<String> {
        let mut dir = Some(self.project.as_path());
        while let Some(d) = dir {
            let head = d.join(".git").join("HEAD");
            if let Ok(text) = std::fs::read_to_string(&head) {
                let text = text.trim();
                return Some(match text.strip_prefix("ref: refs/heads/") {
                    Some(name) => name.to_string(),
                    None => text.chars().take(7).collect(),
                });
            }
            if d.join(".git").is_file() {
                return None;
            }
            dir = d.parent();
        }
        None
    }

    fn banner(&self) {
        let branch = self.branch();
        tui::print_banner(
            &self.term,
            &Banner {
                version: serika_runtime::client::VERSION,
                model: &self.model_name(&self.model),
                provider: &self.provider_label(&self.model),
                directory: &self.project,
                branch: branch.as_deref(),
                permission: Self::permission_label(self.permission),
                account: &self.account_label(),
                session: self.session.as_deref(),
            },
        );
    }

    fn prompt(&self) -> Prompt {
        let mut left = tui::pretty_path(&self.project);
        if let Some(branch) = self.branch() {
            left.push_str(&format!(" {VIOLET_LIGHT}⎇ {branch}{RESET}"));
        }
        left.push_str(&format!(
            "  {DIM}·{RESET}  {}  {DIM}·{RESET}  {}",
            self.model_name(&self.model),
            Self::permission_label(self.permission)
        ));
        if !self.attachments.is_empty() {
            left.push_str(&format!("  {DIM}·{RESET}  📎 {}", self.attachments.len()));
        }
        let notice = if self.queue.is_empty() {
            None
        } else {
            Some(format!(
                "{} queued · Enter on an empty box sends the next · /queue to manage",
                self.queue.len()
            ))
        };
        Prompt {
            footer_left: left,
            footer_right: if self.usage_note.is_empty() { "/ for commands".into() } else { self.usage_note.clone() },
            notice,
            placeholder: "Ask Serika to build, fix, or explain something…   / commands · @ files".into(),
        }
    }

    /// One short line about the limit that matters for the current model.
    async fn refresh_usage(&mut self) {
        self.usage_note = match CliAgent::from_model(&self.model) {
            Some(agent) => match agents::load_usage(&self.runtime.paths.data_dir, agent.key()) {
                Some(usage) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let blocked = usage
                        .windows
                        .iter()
                        .find(|w| (w.status == "rejected" || w.used >= 1.0) && w.resets_at.map(|r| r > now).unwrap_or(true));
                    match blocked {
                        Some(w) => format!(
                            "{AMBER}⚠ {} limit reached{}{RESET}",
                            w.label,
                            w.resets_at.map(|r| format!(" · resets in {}", countdown(r - now))).unwrap_or_default()
                        ),
                        None => usage
                            .windows
                            .iter()
                            .take(2)
                            .map(|w| format!("{} {:.0}%", short_window(&w.key), window_used(w, now) * 100.0))
                            .collect::<Vec<_>>()
                            .join(" · "),
                    }
                }
                None => String::new(),
            },
            None if matches!(ProviderKind::from_model_id(&self.model), ProviderKind::Serika) => {
                match tokio::time::timeout(Duration::from_secs(3), self.runtime.usage()).await {
                    Ok(Ok(snapshot)) => snapshot
                        .plan
                        .and_then(|p| p.weekly)
                        .and_then(|w| match (w.used, w.limit) {
                            (Some(used), Some(limit)) if limit > 0 => {
                                let pct = used as f64 * 100.0 / limit as f64;
                                Some(if pct >= 100.0 {
                                    format!("{AMBER}⚠ weekly limit reached{RESET}")
                                } else {
                                    format!("weekly {} used", pct_text(pct))
                                })
                            }
                            _ => None,
                        })
                        .unwrap_or_default(),
                    _ => String::new(),
                }
            }
            None => String::new(),
        };
    }

    // ---------------------------------------------------------------- sending

    /// Files named with @path, and image paths dropped into the message.
    fn mentioned_files(&self, text: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for raw in text.split_whitespace() {
            let token = raw.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ')' || c == '(');
            let candidate = if let Some(rel) = token.strip_prefix('@') {
                let path = self.project.join(rel);
                path.is_file().then_some(path)
            } else {
                let path = PathBuf::from(token);
                let image = matches!(
                    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
                    Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
                );
                (image && path.is_absolute() && path.is_file()).then_some(path)
            };
            if let Some(path) = candidate {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        out
    }

    fn stage(&self, paths: &[PathBuf], existing: &[Attachment]) -> Vec<Attachment> {
        if paths.is_empty() {
            return Vec::new();
        }
        let dir = self.runtime.paths.data_dir.join("attachments");
        let (staged, errors) = attachments::stage(paths, &dir, existing);
        for error in errors {
            tui::error_line(&self.term, &error);
        }
        staged
    }

    async fn send(&mut self, text: String, mut files: Vec<Attachment>) -> serika_runtime::Result<()> {
        let mentioned = self.mentioned_files(&text);
        let mut staged = self.stage(&mentioned, &files);
        files.append(&mut staged);
        let labels: Vec<String> = files
            .iter()
            .map(|a| match a.kind {
                AttachmentKind::Image => format!("{} (image)", a.name),
                _ => a.name.clone(),
            })
            .collect();
        tui::user_cell(&self.term, &text, &labels);

        let mut prompt = text;
        if let Some(context) = self.carry.take() {
            prompt = format!("Context from earlier in this conversation (summary):\n{context}\n\n---\n\n{prompt}");
        }
        let mut next = Some((prompt, files));
        while let Some((prompt, files)) = next.take() {
            match self.turn(prompt, files, None).await? {
                TurnEnd::Completed(_) => {
                    if let Some(item) = self.queue.pop_front() {
                        let labels: Vec<String> = item.attachments.iter().map(|a| a.name.clone()).collect();
                        tui::user_cell(&self.term, &item.text, &labels);
                        next = Some((item.text, item.attachments));
                    }
                }
                TurnEnd::Failed | TurnEnd::Cancelled => {
                    if !self.queue.is_empty() {
                        tui::note_line(
                            &self.term,
                            &format!("{} queued message(s) are held. Press Enter on an empty box to send the next.", self.queue.len()),
                        );
                    }
                }
            }
        }
        self.refresh_usage().await;
        Ok(())
    }

    /// Run one turn. Keys stay live: Esc or Ctrl+C interrupts, typing and Enter queue a message.
    async fn turn(
        &mut self,
        prompt: String,
        files: Vec<Attachment>,
        permission: Option<PermissionMode>,
    ) -> serika_runtime::Result<TurnEnd> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let approvals = self.runtime.approvals.clone();
        let task = self.runtime.run_task(TaskRequest {
            project: self.project.clone(),
            prompt,
            session_id: self.session.clone(),
            model: Some(self.model.clone()),
            permission: Some(permission.unwrap_or(self.permission)),
            yes: self.yes,
            events: tx,
            cancel: cancel.clone(),
            attachments: files,
        });
        tokio::pin!(task);
        let term = self.term;
        let mut renderer = TurnRenderer::new(term).interactive();
        let raw = if term.fancy { Some(RawGuard::new()) } else { None };
        let mut keys = raw.as_ref().map(|_| EventStream::new());
        let mut draft = String::new();
        let mut failed = false;
        let mut stopping = false;
        let mut ticker = tokio::time::interval(Duration::from_millis(80));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let end = loop {
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    cancel.cancel();
                    approvals.deny_all();
                }
                outcome = &mut task => {
                    while let Ok(event) = rx.try_recv() {
                        self.event(event, &mut renderer, &mut failed);
                    }
                    renderer.finish();
                    break match outcome {
                        Ok(_) if !failed => TurnEnd::Completed(std::mem::take(&mut renderer.summary)),
                        Ok(_) => TurnEnd::Failed,
                        Err(Error::Cancelled) => TurnEnd::Cancelled,
                        Err(err) => {
                            if !failed {
                                tui::error_line(&term, &format!("serika: {err}"));
                            }
                            if matches!(err, Error::AuthRequired | Error::Auth(_)) {
                                tui::note_line(&term, "Sign in with /login, or pick a local or subscription model with /model.");
                            }
                            TurnEnd::Failed
                        }
                    };
                }
                Some(event) = rx.recv() => {
                    if let AgentEvent::ApprovalRequired { id, title, detail, destructive } = event {
                        // The prompt reads keys itself, so the key stream pauses meanwhile.
                        keys = None;
                        renderer.suspend();
                        let allow = tokio::task::block_in_place(|| tui::ask_approval(&term, &title, &detail, destructive));
                        approvals.resolve(&id, allow);
                        renderer.resume();
                        if raw.is_some() {
                            keys = Some(EventStream::new());
                        }
                    } else {
                        self.event(event, &mut renderer, &mut failed);
                    }
                }
                Some(Ok(event)) = async {
                    match keys.as_mut() {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match event {
                        Event::Key(key) if editor::is_press(&key) => {
                            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                            match key.code {
                                KeyCode::Esc => {
                                    if !stopping {
                                        stopping = true;
                                        cancel.cancel();
                                        approvals.deny_all();
                                        renderer.set_status("Stopping");
                                    }
                                }
                                KeyCode::Char('c') if ctrl => {
                                    if draft.is_empty() {
                                        stopping = true;
                                        cancel.cancel();
                                        approvals.deny_all();
                                        renderer.set_status("Stopping");
                                    } else {
                                        draft.clear();
                                    }
                                }
                                KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => draft.push('\n'),
                                KeyCode::Enter => {
                                    let text = draft.trim().to_string();
                                    draft.clear();
                                    if !text.is_empty() {
                                        let mentioned = self.mentioned_files(&text);
                                        let attachments = self.stage(&mentioned, &[]);
                                        self.queue.push_back(Queued { text, attachments });
                                    }
                                }
                                KeyCode::Backspace => { draft.pop(); }
                                KeyCode::Char(c) if !ctrl || key.modifiers.contains(KeyModifiers::ALT) => draft.push(c),
                                _ => {}
                            }
                            renderer.set_queue(self.queue.len(), &draft);
                        }
                        Event::Paste(text) => {
                            draft.push_str(&text.replace("\r\n", "\n"));
                            renderer.set_queue(self.queue.len(), &draft);
                        }
                        _ => {}
                    }
                }
                _ = ticker.tick() => renderer.tick(),
            }
        };
        drop(keys);
        drop(raw);
        if !draft.trim().is_empty() {
            self.editor.prefill(draft.trim_end());
        }
        Ok(end)
    }

    fn event(&mut self, event: AgentEvent, renderer: &mut TurnRenderer, failed: &mut bool) {
        match event {
            AgentEvent::Started { session_id } => {
                self.session = Some(session_id.clone());
                renderer.session_started(&session_id);
            }
            AgentEvent::Status(text) => renderer.set_status(&text),
            AgentEvent::MessageChunk { text } => renderer.message_chunk(&text),
            AgentEvent::ReasoningChunk { .. } => renderer.set_status("Thinking"),
            AgentEvent::MessageCompleted { .. } => renderer.message_completed(),
            AgentEvent::ToolRequested { id, name, arguments } => renderer.tool_requested(&id, &name, &arguments),
            AgentEvent::ApprovalRequired { id, .. } => {
                self.runtime.approvals.resolve(&id, false);
            }
            AgentEvent::ToolStarted { .. } => {}
            AgentEvent::ToolOutput { id, stream } => renderer.tool_output(&id, &stream),
            AgentEvent::ToolFinished { id, name, ok, summary } => renderer.tool_finished(&id, &name, ok, &summary),
            AgentEvent::FileChanged(diff) => renderer.file_changed(&diff.path, &diff.diff),
            AgentEvent::UsageUpdated(usage) => renderer.usage(usage.summary_line()),
            AgentEvent::Browser { frame } => renderer.set_status(&format!("Browsing {}", frame.url)),
            AgentEvent::ProviderUsage { usage, .. } => {
                if let Some(message) = usage.message {
                    renderer.set_status(&message);
                }
            }
            AgentEvent::Completed { .. } => {}
            AgentEvent::Failed { message } => {
                renderer.failed(&message);
                *failed = true;
            }
            AgentEvent::Cancelled => renderer.cancelled(),
        }
    }

    // ---------------------------------------------------------------- commands

    async fn command(&mut self, line: &str) -> serika_runtime::Result<Flow> {
        let (name, arg) = match line.split_once(char::is_whitespace) {
            Some((name, rest)) => (name.to_ascii_lowercase(), rest.trim().to_string()),
            None => (line.to_ascii_lowercase(), String::new()),
        };
        let term = self.term;
        match name.as_str() {
            "/quit" | "/exit" | "/q" => return Ok(Flow::Exit),
            "/help" | "/?" => tui::print_help(&term),
            "/new" => {
                self.session = None;
                self.carry = None;
                tui::note_line(&term, "Started a fresh conversation.");
            }
            "/clear" => {
                self.session = None;
                self.carry = None;
                tui::clear_screen(&term);
                self.banner();
            }
            "/status" => self.status(),
            "/model" | "/models" => self.choose_model(&arg).await?,
            "/provider" | "/providers" => self.providers().await,
            "/permissions" | "/approvals" => self.choose_permission(&arg),
            "/effort" => self.choose_effort(&arg),
            "/usage" => self.usage().await,
            "/sessions" | "/resume" => self.choose_session(&arg)?,
            "/compact" => self.compact().await?,
            "/diff" => {
                let diff = serika_runtime::gitinfo::diff(&self.project, None).await?;
                if diff.trim().is_empty() {
                    tui::note_line(&term, "No uncommitted changes.");
                } else {
                    tui::print_diff(&term, &diff);
                }
            }
            "/review" => {
                let focus: Vec<String> = arg.split_whitespace().map(str::to_string).collect();
                crate::review(self.runtime, &self.project, true, &focus, Some(self.model.clone()), self.yes, &term).await?;
            }
            "/browser" => self.browser(&arg).await,
            "/attach" => {
                let paths: Vec<PathBuf> = shell_words(&arg)
                    .into_iter()
                    .map(|p| {
                        let path = PathBuf::from(&p);
                        if path.is_absolute() { path } else { self.project.join(p) }
                    })
                    .collect();
                if paths.is_empty() {
                    tui::note_line(&term, "Usage: /attach <path> [more paths]");
                } else {
                    let staged = self.stage(&paths, &self.attachments.clone());
                    for item in &staged {
                        tui::note_line(&term, &format!("📎 {} will go with your next message", item.name));
                    }
                    self.attachments.extend(staged);
                }
            }
            "/queue" => {
                if arg == "clear" {
                    self.queue.clear();
                    tui::note_line(&term, "Queue cleared.");
                } else if self.queue.is_empty() {
                    tui::note_line(&term, "Nothing queued. Type while a reply runs and press Enter to queue.");
                } else {
                    for (i, item) in self.queue.iter().enumerate() {
                        outln!("  {VIOLET_LIGHT}{}.{RESET} {}", i + 1, tui::ellipsize_end(&item.text.replace('\n', " ⏎ "), term.width().saturating_sub(10)));
                    }
                    tui::info_line(&term, "Enter on an empty box sends the next. /queue clear drops them.");
                }
            }
            "/expand" => tui::print_expanded(&term),
            "/theme" => self.choose_theme(&arg),
            "/doctor" => {
                crate::doctor(self.runtime, &term).await?;
            }
            "/login" => {
                crate::login(self.runtime).await?;
            }
            "/logout" => {
                self.runtime.logout().await?;
                tui::note_line(&term, "Signed out of Serika.");
            }
            other => tui::error_line(&term, &format!("Unknown command {other}. /help lists them.")),
        }
        Ok(Flow::Continue)
    }

    fn status(&self) {
        let rows = [
            ("model", format!("{} {DIM}({}){RESET}", self.model_name(&self.model), self.model)),
            ("provider", self.provider_label(&self.model)),
            ("directory", tui::pretty_path(&self.project)),
            ("branch", self.branch().unwrap_or_else(|| "-".into())),
            ("permissions", Self::permission_label(self.permission).to_string()),
            ("account", self.account_label()),
            ("session", self.session.clone().unwrap_or_else(|| "new".into())),
            ("theme", tui::theme().name.to_string()),
        ];
        outln!();
        for (label, value) in rows {
            outln!("  {DIM}{label:<12}{RESET} {value}");
        }
        outln!();
    }

    fn save_config(&self, key: &str, value: &str) -> bool {
        let mut config = self.runtime.live_config();
        match config.set_field(key, value).and_then(|_| config.save(&self.runtime.paths.config_file)) {
            Ok(()) => true,
            Err(err) => {
                tui::error_line(&self.term, &format!("could not save {key}: {err}"));
                false
            }
        }
    }

    async fn choose_model(&mut self, arg: &str) -> serika_runtime::Result<()> {
        let term = self.term;
        if !arg.is_empty() {
            self.set_model(arg.to_string());
            return Ok(());
        }
        tui::info_line(&term, "Loading models…");
        let runtime = self.runtime;
        let signed_in = runtime.signed_in().ok().flatten().is_some();
        let serika = async {
            if signed_in {
                tokio::time::timeout(Duration::from_secs(8), runtime.models()).await.ok().and_then(|r| r.ok())
            } else {
                None
            }
        };
        let local = tokio::time::timeout(
            Duration::from_secs(6),
            serika_runtime::providers::discover_all_models(runtime.client.http(), &runtime.config),
        );
        let (catalog, statuses, discovered) = tokio::join!(serika, runtime.provider_statuses(false), local);

        let mut items: Vec<PickItem> = Vec::new();
        let mut ids: Vec<Option<String>> = Vec::new();
        let push = |items: &mut Vec<PickItem>, ids: &mut Vec<Option<String>>, section: &str, id: Option<String>, label: String, detail: String, marker: String| {
            items.push(PickItem { section: section.to_string(), label, detail, marker });
            ids.push(id);
        };
        if let Some(catalog) = &catalog {
            for (section, list) in [("Serika · included", &catalog.free), ("Serika · Serika+", &catalog.premium)] {
                for m in list {
                    let mult = m.usage_multiplier.filter(|x| *x > 1.0).map(|x| format!("{x}x")).unwrap_or_default();
                    push(&mut items, &mut ids, section, Some(m.id.clone()), m.name.clone(), m.description.clone().unwrap_or_else(|| m.id.clone()), format!("{DIM}{mult}{RESET}"));
                }
            }
        } else {
            push(&mut items, &mut ids, "Serika", None, "Sign in to use Serika models".into(), "/login".into(), String::new());
        }
        for status in &statuses {
            let section = format!("{} · {}", status.name, status.plan.clone().unwrap_or_else(|| "subscription".into()));
            if status.installed && status.signed_in == Some(true) {
                let limited = provider_limited(status);
                for m in &status.models {
                    let marker = if limited { format!("{AMBER}⚠{RESET}") } else { String::new() };
                    let efforts = if m.efforts.is_empty() { String::new() } else { format!(" · effort {}", m.efforts.join("/")) };
                    push(&mut items, &mut ids, &section, Some(m.id.clone()), m.name.clone(), format!("{}{efforts}", m.description.clone().unwrap_or_default()), marker);
                    self.names.insert(m.id.clone(), m.name.clone());
                }
            } else {
                let hint = if status.installed { status.login_hint.clone() } else { status.install_hint.clone() };
                push(&mut items, &mut ids, &section, None, format!("Set up {}", status.name), hint, String::new());
            }
        }
        if let Ok(report) = discovered {
            for m in report.models {
                let section = if m.provider.as_deref() == Some("local") { "Local".to_string() } else { "API keys".to_string() };
                push(&mut items, &mut ids, &section, Some(m.id.clone()), m.name.clone(), m.id.clone(), String::new());
            }
        }
        if let Some(catalog) = &catalog {
            for m in catalog.free.iter().chain(catalog.premium.iter()) {
                self.names.insert(m.id.clone(), m.name.clone());
            }
        }
        let current = ids.iter().position(|id| id.as_deref() == Some(self.model.as_str())).unwrap_or(0);
        let picked = tokio::task::block_in_place(|| editor::pick(&term, "Choose a model", &items, current));
        match picked.and_then(|i| ids.get(i).cloned()) {
            Some(Some(id)) => self.set_model(id),
            Some(None) => {
                let item = &items[picked.unwrap_or(0)];
                tui::note_line(&term, &format!("{}: run `{}` in a terminal, then /provider to check.", item.label, item.detail));
            }
            None => {}
        }
        Ok(())
    }

    fn set_model(&mut self, id: String) {
        self.model = id.clone();
        if self.save_config("model", &id) {
            tui::note_line(&self.term, &format!(
                "Model: {BOLD}{}{RESET} {DIM}· {}{RESET}",
                self.model_name(&id),
                self.provider_label(&id)
            ));
        }
        // Another provider cannot resume this provider's conversation.
        self.session = None;
        self.usage_note.clear();
    }

    fn choose_permission(&mut self, arg: &str) {
        let modes = [
            (PermissionMode::Restricted, "Asks before every edit and command"),
            (PermissionMode::Standard, "Edits and dev commands run, risky ones ask"),
            (PermissionMode::Full, "Runs without asking, except destructive actions"),
            (PermissionMode::ReadOnly, "Can look around, cannot change anything"),
        ];
        let chosen = if arg.is_empty() {
            let items: Vec<PickItem> = modes
                .iter()
                .map(|(mode, help)| PickItem {
                    section: "Permissions".into(),
                    label: Self::permission_label(*mode).into(),
                    detail: help.to_string(),
                    marker: String::new(),
                })
                .collect();
            let current = modes.iter().position(|(m, _)| *m == self.permission).unwrap_or(1);
            tokio::task::block_in_place(|| editor::pick(&self.term, "Permissions", &items, current)).map(|i| modes[i].0)
        } else {
            let found = PermissionMode::parse(arg).or_else(|| {
                modes.iter().map(|(m, _)| *m).find(|m| Self::permission_label(*m).eq_ignore_ascii_case(arg))
            });
            if found.is_none() {
                tui::error_line(&self.term, "Use read_only, standard, restricted, or full.");
            }
            found
        };
        if let Some(mode) = chosen {
            self.permission = mode;
            self.save_config("permission", mode.as_str());
            tui::note_line(&self.term, &format!("Permissions: {BOLD}{}{RESET}", Self::permission_label(mode)));
        }
    }

    fn choose_effort(&mut self, arg: &str) {
        let Some(agent) = CliAgent::from_model(&self.model) else {
            tui::note_line(&self.term, "Effort applies to Claude Code and Codex models. Pick one with /model.");
            return;
        };
        let wanted = match agent.model_arg(&self.model) {
            Some(_) => self.model.clone(),
            None => format!("{}/default", agent.provider_id()),
        };
        let efforts = agents::cached_models(&self.runtime.paths.data_dir, agent)
            .into_iter()
            .find(|m| m.id == wanted)
            .map(|m| m.efforts)
            .unwrap_or_default();
        if efforts.is_empty() {
            tui::note_line(&self.term, "This model has no effort setting.");
            return;
        }
        let key = match agent {
            CliAgent::Claude => "claude_effort",
            CliAgent::Codex => "codex_effort",
        };
        let value = if arg.is_empty() {
            let mut options = vec!["default".to_string()];
            options.extend(efforts.iter().cloned());
            let items: Vec<PickItem> = options
                .iter()
                .map(|e| PickItem { section: "Reasoning effort".into(), label: e.clone(), detail: String::new(), marker: String::new() })
                .collect();
            match tokio::task::block_in_place(|| editor::pick(&self.term, "Reasoning effort", &items, 0)) {
                Some(i) => options[i].clone(),
                None => return,
            }
        } else {
            arg.to_ascii_lowercase()
        };
        if value != "default" && !efforts.contains(&value) {
            tui::error_line(&self.term, &format!("This model accepts {}.", efforts.join(", ")));
            return;
        }
        if self.save_config(key, &value) {
            tui::note_line(&self.term, &format!("Effort: {BOLD}{value}{RESET}"));
        }
    }

    async fn providers(&mut self) {
        tui::info_line(&self.term, "Checking Claude Code and Codex…");
        let statuses = self.runtime.provider_statuses(true).await;
        outln!();
        for status in &statuses {
            let state = if !status.installed {
                format!("{DIM}not installed{RESET}")
            } else if status.signed_in == Some(true) {
                format!("{GREEN}connected{RESET}")
            } else {
                format!("{AMBER}sign-in needed{RESET}")
            };
            let mark = if provider_limited(status) { format!(" {AMBER}⚠ limit reached{RESET}") } else { String::new() };
            outln!("  {BOLD}{}{RESET}  {state}{mark}", status.name);
            if let Some(account) = &status.account {
                outln!("    {DIM}account{RESET}   {account}{}", status.plan.as_ref().map(|p| format!(" · {p}")).unwrap_or_default());
            }
            if let Some(version) = &status.version {
                outln!("    {DIM}version{RESET}   {version}");
            }
            if status.installed {
                let names: Vec<&str> = status.models.iter().map(|m| m.name.as_str()).take(8).collect();
                outln!("    {DIM}models{RESET}    {}{}", names.join(", "), if status.models_live { "" } else { " (built-in list)" });
            } else {
                outln!("    {DIM}install{RESET}   {VIOLET_LIGHT}{}{RESET}", status.install_hint);
            }
            if status.installed && status.signed_in != Some(true) {
                outln!("    {DIM}sign in{RESET}   {VIOLET_LIGHT}{}{RESET}", status.login_hint);
            }
            print_windows(status);
            outln!();
        }
        self.learn_cached_names();
    }

    async fn usage(&mut self) {
        outln!();
        match tokio::time::timeout(Duration::from_secs(6), self.runtime.usage()).await {
            Ok(Ok(snapshot)) => {
                outln!("  {BOLD}Serika{RESET}  {DIM}{}{RESET}", snapshot.summary_line());
                if let Some(plan) = &snapshot.plan {
                    for (label, window) in [("session", &plan.session), ("weekly", &plan.weekly)] {
                        if let Some(w) = window {
                            if let (Some(used), Some(limit)) = (w.used, w.limit) {
                                let fraction = if limit > 0 { used as f64 / limit as f64 } else { 0.0 };
                                outln!(
                                    "    {DIM}{label:<10}{RESET}{} {}  {DIM}{} of {} tokens{}{RESET}",
                                    tui::bar(fraction, 24),
                                    pct_text(fraction * 100.0),
                                    compact(used),
                                    compact(limit),
                                    w.resets_at.as_ref().map(|r| format!(" · resets {r}")).unwrap_or_default()
                                );
                            }
                        }
                    }
                }
            }
            Ok(Err(err)) => outln!("  {BOLD}Serika{RESET}  {DIM}{err}{RESET}"),
            Err(_) => outln!("  {BOLD}Serika{RESET}  {DIM}no answer{RESET}"),
        }
        for agent in [CliAgent::Claude, CliAgent::Codex] {
            if let Some(usage) = agents::load_usage(&self.runtime.paths.data_dir, agent.key()) {
                outln!();
                outln!("  {BOLD}{}{RESET}  {DIM}updated {} ago{RESET}", agent.label(), countdown(now_secs() - usage.updated_at));
                let status = ProviderStatus {
                    id: agent.key().into(),
                    name: agent.label().into(),
                    installed: true,
                    binary: None,
                    version: None,
                    signed_in: Some(true),
                    account: None,
                    plan: None,
                    detail: String::new(),
                    install_hint: String::new(),
                    login_hint: String::new(),
                    docs_url: String::new(),
                    usage_url: String::new(),
                    usage: Some(usage),
                    models: Vec::new(),
                    models_live: false,
                    effort: None,
                };
                print_windows(&status);
            }
        }
        outln!();
        self.refresh_usage().await;
    }

    fn choose_session(&mut self, arg: &str) -> serika_runtime::Result<()> {
        let list = self.runtime.sessions.list(Some(&self.project))?;
        if list.is_empty() {
            tui::note_line(&self.term, "No saved conversations in this folder yet.");
            return Ok(());
        }
        let id = if !arg.is_empty() {
            Some(arg.to_string())
        } else {
            let now = now_secs();
            let items: Vec<PickItem> = list
                .iter()
                .map(|s| PickItem {
                    section: "Conversations in this folder".into(),
                    label: s.title.clone(),
                    detail: format!("{} ago · {}", countdown(now - s.updated_at), self.model_name(&s.model_id)),
                    marker: String::new(),
                })
                .collect();
            tokio::task::block_in_place(|| editor::pick(&self.term, "Resume a conversation", &items, 0)).map(|i| list[i].id.clone())
        };
        let Some(id) = id else { return Ok(()) };
        let Some(summary) = self.runtime.sessions.get(&id)? else {
            tui::error_line(&self.term, &format!("No conversation {id}."));
            return Ok(());
        };
        self.session = Some(id.clone());
        self.model = summary.model_id.clone();
        outln!();
        outln!("  {BOLD}{}{RESET}  {DIM}{}{RESET}", summary.title, self.model_name(&summary.model_id));
        let messages = self.runtime.sessions.messages(&id)?;
        let visible: Vec<_> = messages
            .iter()
            .filter(|m| !serika_runtime::sessions::is_tool_result(&m.role, &m.content) && !m.content.trim().is_empty())
            .collect();
        for message in visible.iter().rev().take(6).rev() {
            let (text, _) = serika_runtime::protocol::extract_tool_calls(&message.content);
            let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            let line = tui::ellipsize_end(&first, self.term.width().saturating_sub(10));
            if message.role == "user" {
                outln!("  {VIOLET_LIGHT}›{RESET} {line}");
            } else {
                outln!("  {DIM}● {line}{RESET}");
            }
        }
        outln!();
        tui::note_line(&self.term, "Resumed. Your next message continues this conversation.");
        Ok(())
    }

    async fn compact(&mut self) -> serika_runtime::Result<()> {
        if self.session.is_none() {
            tui::note_line(&self.term, "Nothing to compact yet.");
            return Ok(());
        }
        tui::note_line(&self.term, "Summarising the conversation…");
        match self.turn(COMPACT_PROMPT.to_string(), Vec::new(), Some(PermissionMode::ReadOnly)).await? {
            TurnEnd::Completed(summary) if !summary.trim().is_empty() => {
                self.carry = Some(summary);
                self.session = None;
                tui::note_line(&self.term, "Compacted. Your next message starts a new conversation that carries this summary.");
            }
            _ => tui::error_line(&self.term, "Could not compact; the conversation is unchanged."),
        }
        Ok(())
    }

    async fn browser(&mut self, arg: &str) {
        let term = self.term;
        match arg {
            "" => tui::note_line(&term, "Usage: /browser <url or search> · /browser on|off · /browser close"),
            "on" | "off" => {
                if self.save_config("browser_enabled", if arg == "on" { "true" } else { "false" }) {
                    tui::note_line(&term, &format!("Serika's browser is {arg}."));
                }
            }
            "close" => {
                self.runtime.browser.shutdown().await;
                tui::note_line(&term, "Browser closed.");
            }
            target => {
                tui::info_line(&term, &format!("Opening {target}…"));
                match self.runtime.browser.open(target).await {
                    Ok((snapshot, frame)) => {
                        outln!();
                        outln!("  🌐 {BOLD}{}{RESET}  {DIM}{}{RESET}", frame.title, frame.url);
                        let width = term.width().saturating_sub(6);
                        for line in snapshot.text.lines().filter(|l| !l.trim().is_empty()).take(14) {
                            outln!("  {DIM}│{RESET} {}", tui::ellipsize_end(line.trim(), width));
                        }
                        outln!("  {DIM}{} links and fields · ask Serika to use this page, or /browser close{RESET}", snapshot.elements.len());
                        outln!();
                    }
                    Err(err) => tui::error_line(&term, &format!("{err}")),
                }
            }
        }
    }

    fn choose_theme(&mut self, arg: &str) {
        let name = if arg.is_empty() {
            let items: Vec<PickItem> = tui::THEMES
                .iter()
                .map(|t| {
                    let swatch: String = (0..12).map(|i| {
                        let (r, g, b) = mix3(t.stops, i as f32 / 11.0);
                        format!("\x1b[38;2;{r};{g};{b}m█")
                    }).collect::<String>() + RESET;
                    PickItem { section: "Themes".into(), label: t.name.into(), detail: String::new(), marker: swatch }
                })
                .collect();
            let current = tui::THEMES.iter().position(|t| t.name == tui::theme().name).unwrap_or(0);
            match tokio::task::block_in_place(|| editor::pick(&self.term, "Theme", &items, current)) {
                Some(i) => tui::THEMES[i].name.to_string(),
                None => return,
            }
        } else {
            arg.to_string()
        };
        if tui::set_theme(&name) {
            let mut prefs = load_cli_prefs(self.runtime);
            prefs["theme"] = serde_json::Value::String(tui::theme().name.into());
            let _ = std::fs::write(cli_prefs_path(self.runtime), prefs.to_string());
            tui::note_line(&self.term, &format!("Theme: {}", tui::gradient_text(tui::theme().name)));
        } else {
            let names: Vec<&str> = tui::THEMES.iter().map(|t| t.name).collect();
            tui::error_line(&self.term, &format!("Themes: {}", names.join(", ")));
        }
    }
}

fn mix3(stops: [(u8, u8, u8); 3], t: f32) -> (u8, u8, u8) {
    let lerp = |a: (u8, u8, u8), b: (u8, u8, u8), t: f32| {
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
    };
    if t < 0.5 { lerp(stops[0], stops[1], t * 2.0) } else { lerp(stops[1], stops[2], (t - 0.5) * 2.0) }
}

fn print_windows(status: &ProviderStatus) {
    let Some(usage) = &status.usage else { return };
    let now = now_secs();
    for w in &usage.windows {
        let reset = w
            .resets_at
            .filter(|r| *r > now)
            .map(|r| format!(" · resets in {}", countdown(r - now)))
            .unwrap_or_default();
        let live = w.resets_at.map(|r| r > now).unwrap_or(true);
        let blocked = if w.status == "rejected" && live { format!(" {RED}limit reached{RESET}") } else { String::new() };
        let used = window_used(w, now);
        outln!(
            "    {DIM}{:<10}{RESET}{} {}{DIM}{reset}{RESET}{blocked}",
            short_window(&w.key),
            tui::bar(used, 24),
            pct_text(used * 100.0)
        );
    }
    if let Some(message) = &usage.message {
        outln!("    {AMBER}{message}{RESET}");
    }
}

/// A limit in force now: a rejected window whose reset is still ahead.
fn provider_limited(status: &ProviderStatus) -> bool {
    let now = now_secs();
    status
        .usage
        .as_ref()
        .map(|u| {
            u.windows
                .iter()
                .any(|w| (w.status == "rejected" || w.used >= 1.0) && w.resets_at.map(|r| r > now).unwrap_or(true))
        })
        .unwrap_or(false)
}

/// Share of a window in use. Once its reset time has passed it starts from zero again.
fn window_used(w: &agents::UsageWindowInfo, now: i64) -> f64 {
    match w.resets_at {
        Some(reset) if reset <= now => 0.0,
        _ => w.used,
    }
}

fn short_window(key: &str) -> String {
    match key {
        "five_hour" => "5h".into(),
        "seven_day" => "week".into(),
        "seven_day_opus" => "week opus".into(),
        "seven_day_sonnet" => "week sonnet".into(),
        other => other.replace('_', " "),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "3d 2h", "4h 12m", "9m".
fn countdown(secs: i64) -> String {
    let secs = secs.max(0);
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// Small shares keep a decimal so 0.4% does not read as 0%.
fn pct_text(pct: f64) -> String {
    if pct > 0.0 && pct < 10.0 {
        format!("{pct:.1}%")
    } else {
        format!("{pct:.0}%")
    }
}

fn compact(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
        n if n >= 1_000 => format!("{:.0}K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// Split on spaces, keeping "quoted paths" together.
fn shell_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            (_, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn cli_prefs_path(runtime: &Runtime) -> PathBuf {
    runtime.paths.data_dir.join("cli.json")
}

fn load_cli_prefs(runtime: &Runtime) -> serde_json::Value {
    std::fs::read_to_string(cli_prefs_path(runtime))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_paths() {
        assert_eq!(shell_words(r#"a "b c" 'd e' f"#), vec!["a", "b c", "d e", "f"]);
    }

    #[test]
    fn formats_numbers_and_times() {
        assert_eq!(pct_text(0.36), "0.4%");
        assert_eq!(pct_text(62.0), "62%");
        assert_eq!(compact(45_000_000), "45M");
        assert_eq!(compact(72_400), "72K");
        assert_eq!(countdown(3 * 86400 + 7200), "3d 2h");
        assert_eq!(countdown(30), "1m");
    }
}
