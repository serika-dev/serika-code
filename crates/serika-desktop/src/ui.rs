use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use serika_runtime::gitinfo::{self, GitRepoSnapshot};
use serika_runtime::setup::{self, SetupOptions};
use serika_runtime::updater::{self, UpdateInfo};
use serika_runtime::{
    AgentEvent, FileDiff, LoginOutcome, PermissionMode, Runtime, SessionSummary, TaskRequest,
    UsageSnapshot,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

pub fn launch(smoke: bool) {
    let runtime = match Runtime::open() {
        Ok(runtime) => Arc::new(runtime),
        Err(err) => {
            eprintln!("serika-desktop: {err}");
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([960.0, 620.0])
            .with_title("Serika Code"),
        ..Default::default()
    };
    if let Err(err) = eframe::run_native(
        "Serika Code",
        options,
        Box::new(move |cc| Ok(Box::new(SerikaApp::new(cc, runtime, smoke)))),
    ) {
        eprintln!("serika-desktop: {err}");
        std::process::exit(1);
    }
}

#[derive(Clone)]
enum MessageSegment {
    Text(String),
    Code { lang: String, code: String },
}

#[derive(Clone)]
struct UiMessage {
    role: String,
    raw: String,
    segments: Vec<MessageSegment>,
}

impl UiMessage {
    fn new(role: impl Into<String>, raw: impl Into<String>) -> Self {
        let role = role.into();
        let raw = raw.into();
        let segments = parse_segments(&raw);
        Self {
            role,
            raw,
            segments,
        }
    }
}

#[derive(Clone)]
struct PendingApproval {
    id: String,
    title: String,
    detail: String,
    destructive: bool,
}

enum AppMsg {
    GitSnapshot(GitRepoSnapshot),
    GitActionDone { notice: String, snapshot: GitRepoSnapshot },
    WorktreeCreated { path: PathBuf, snapshot: GitRepoSnapshot },
    FileDiffLoaded { title: String, diff: String },
    UpdateChecked(serika_runtime::Result<UpdateInfo>),
    UpdateInstalled(serika_runtime::Result<updater::UpdateOutcome>),
    SetupCompleted(serika_runtime::Result<setup::SetupReport>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightTab {
    Diff,
    Git,
    Activity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffScope {
    Session,
    Unstaged,
    Staged,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffViewMode {
    Unified,
    Split,
}

enum SidebarAction {
    OpenSession(String),
    DeleteSession(String),
    SwitchProject(String),
}

struct SerikaApp {
    rt: tokio::runtime::Runtime,
    core: Arc<Runtime>,
    events_tx: UnboundedSender<AgentEvent>,
    events_rx: UnboundedReceiver<AgentEvent>,
    app_tx: UnboundedSender<AppMsg>,
    app_rx: UnboundedReceiver<AppMsg>,
    cancel: Option<CancellationToken>,
    smoke: bool,
    project: String,
    recent_projects: Vec<String>,
    sessions: Vec<SessionSummary>,
    session_filter: String,
    session_id: Option<String>,
    messages: Vec<UiMessage>,
    streaming: String,
    streaming_segments: Vec<MessageSegment>,
    active_tool: Option<String>,
    input: String,
    activity: Vec<String>,
    diffs: Vec<FileDiff>,
    diff_index: usize,
    diff_scope: DiffScope,
    diff_view_mode: DiffViewMode,
    custom_diff_title: String,
    custom_diff_text: String,
    usage: Option<UsageSnapshot>,
    model_ids: Vec<String>,
    model_labels: Vec<String>,
    model_index: usize,
    permission: PermissionMode,
    account: String,
    status: String,
    running: bool,
    approval: Option<PendingApproval>,
    show_settings: bool,
    show_account: bool,
    show_palette: bool,
    show_inspector: bool,
    palette_query: String,
    right: RightTab,
    git: GitRepoSnapshot,
    git_loading: bool,
    git_commit_msg: String,
    git_new_branch: String,
    update_info: Option<UpdateInfo>,
    updating: bool,
    login_email: String,
    login_password: String,
    login_code: String,
    login_magic: String,
    login_note: String,
    timeout: String,
    retention: String,
}

impl SerikaApp {
    fn new(cc: &eframe::CreationContext<'_>, core: Arc<Runtime>, smoke: bool) -> Self {
        apply_theme(&cc.egui_ctx, &core.config.theme);
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (app_tx, app_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio");
        let project = std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| core.paths.scratch_dir().display().to_string());
        let permission = core.config.permission;
        let timeout = core.config.command_timeout_secs.to_string();
        let retention = core.config.session_retention_days.to_string();
        let account = core
            .signed_in()
            .ok()
            .flatten()
            .map(|session| {
                format!(
                    "{} · {}",
                    session.username,
                    if session.is_premium { "premium" } else { "free" }
                )
            })
            .unwrap_or_else(|| "Not signed in".into());
        let mut app = Self {
            rt,
            core,
            events_tx,
            events_rx,
            app_tx,
            app_rx,
            cancel: None,
            smoke,
            project,
            recent_projects: Vec::new(),
            sessions: Vec::new(),
            session_filter: String::new(),
            session_id: None,
            messages: Vec::new(),
            streaming: String::new(),
            streaming_segments: Vec::new(),
            active_tool: None,
            input: String::new(),
            activity: Vec::new(),
            diffs: Vec::new(),
            diff_index: 0,
            diff_scope: DiffScope::Session,
            diff_view_mode: DiffViewMode::Unified,
            custom_diff_title: String::new(),
            custom_diff_text: String::new(),
            usage: None,
            model_ids: Vec::new(),
            model_labels: Vec::new(),
            model_index: 0,
            permission,
            account,
            status: "Ready".into(),
            running: false,
            approval: None,
            show_settings: false,
            show_account: false,
            show_palette: false,
            show_inspector: true,
            palette_query: String::new(),
            right: RightTab::Git,
            git: GitRepoSnapshot::default(),
            git_loading: false,
            git_commit_msg: String::new(),
            git_new_branch: String::new(),
            update_info: None,
            updating: false,
            login_email: String::new(),
            login_password: String::new(),
            login_code: String::new(),
            login_magic: String::new(),
            login_note: String::new(),
            timeout,
            retention,
        };
        if !smoke {
            app.reload_sessions();
            app.bootstrap(&cc.egui_ctx);
            app.refresh_git(&cc.egui_ctx);
        }
        app
    }

    fn is_scratch(&self) -> bool {
        let scratch = self.core.paths.scratch_dir();
        let current = PathBuf::from(&self.project);
        std::fs::canonicalize(&current).unwrap_or(current)
            == std::fs::canonicalize(&scratch).unwrap_or(scratch)
    }

    fn bootstrap(&self, ctx: &egui::Context) {
        self.load_models(ctx);
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            if let Ok(usage) = core.usage().await {
                let _ = tx.send(AgentEvent::UsageUpdated(usage));
                egui_ctx.request_repaint();
            }
        });
        self.check_updates(ctx, false);
    }

    fn check_updates(&self, ctx: &egui::Context, manual: bool) {
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            let res = updater::check_for_update().await;
            if manual || res.as_ref().map(|u| u.update_available).unwrap_or(false) {
                let _ = tx.send(AppMsg::UpdateChecked(res));
                egui_ctx.request_repaint();
            }
        });
    }

    fn trigger_self_update(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_info.clone() else {
            self.check_updates(ctx, true);
            return;
        };
        self.updating = true;
        self.status = format!("Downloading update {}…", info.tag_name);
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            let res = updater::install_update(&info, None, true).await;
            let _ = tx.send(AppMsg::UpdateInstalled(res));
            egui_ctx.request_repaint();
        });
    }

    fn trigger_system_setup(&mut self, ctx: &egui::Context) {
        self.status = "Running system setup…".into();
        let paths = self.core.paths.clone();
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                setup::install_system(&paths, &SetupOptions::default())
            })
            .await
            .map_err(|err| serika_runtime::Error::msg(err.to_string()))
            .and_then(|r| r);
            let _ = tx.send(AppMsg::SetupCompleted(res));
            egui_ctx.request_repaint();
        });
    }

    fn load_models(&self, ctx: &egui::Context) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            match core.models().await {
                Ok(catalog) => {
                    let payload = catalog
                        .all()
                        .map(|model| {
                            format!(
                                "{}\t{}{}",
                                model.id,
                                model.name,
                                if model.premium { " · PRO" } else { " · FREE" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = tx.send(AgentEvent::Status(format!("catalog\n{payload}")));
                    egui_ctx.request_repaint();
                }
                Err(err) => {
                    let _ = tx.send(AgentEvent::Failed {
                        message: format!("Could not load models: {err}"),
                    });
                    egui_ctx.request_repaint();
                }
            }
        });
    }

    fn refresh_git(&mut self, ctx: &egui::Context) {
        let root = PathBuf::from(&self.project);
        if !root.is_dir() {
            return;
        }
        self.git_loading = true;
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            if let Ok(snap) = gitinfo::snapshot(&root).await {
                let _ = tx.send(AppMsg::GitSnapshot(snap));
                egui_ctx.request_repaint();
            }
        });
    }

    fn reload_sessions(&mut self) {
        let path = PathBuf::from(&self.project);
        self.sessions = self.core.sessions.list(Some(&path)).unwrap_or_default();
        self.recent_projects = self.core.sessions.recent_projects().unwrap_or_default();
    }

    fn new_thread(&mut self, scratch: bool, ctx: &egui::Context) {
        if scratch {
            let scratch_dir = self.core.paths.scratch_dir();
            let _ = std::fs::create_dir_all(&scratch_dir);
            self.project = scratch_dir.display().to_string();
        }
        self.session_id = None;
        self.messages.clear();
        self.diffs.clear();
        self.activity.clear();
        self.streaming.clear();
        self.streaming_segments.clear();
        self.active_tool = None;
        self.reload_sessions();
        self.refresh_git(ctx);
        self.status = if scratch {
            "New scratch thread ready".into()
        } else {
            "New thread ready".into()
        };
    }

    fn turn_into_project(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            let _ = self.core.sessions.remember_project(&path);
            if let Some(id) = &self.session_id {
                let _ = self.core.sessions.relocate_project(id, &path);
            }
            self.project = path.display().to_string();
            self.reload_sessions();
            self.refresh_git(ctx);
            self.status = format!("Thread bound to {}", path.display());
        }
    }

    fn isolate_in_worktree(&mut self, ctx: &egui::Context) {
        let root = PathBuf::from(&self.project);
        let worktrees_root = self.core.paths.worktrees_dir();
        let branch_name = if !self.git_new_branch.trim().is_empty() {
            self.git_new_branch.trim().to_string()
        } else if let Some(id) = &self.session_id {
            format!("thread-{}", &id[..id.len().min(8)])
        } else {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("thread-{secs:x}")
        };
        self.git_new_branch.clear();
        self.status = format!("Creating isolated worktree `{branch_name}`…");
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            match gitinfo::create_worktree(&root, &worktrees_root, &branch_name).await {
                Ok(wt_path) => {
                    let snap = gitinfo::snapshot(&wt_path).await.unwrap_or_default();
                    let _ = tx.send(AppMsg::WorktreeCreated {
                        path: wt_path,
                        snapshot: snap,
                    });
                    egui_ctx.request_repaint();
                }
                Err(err) => {
                    let snap = gitinfo::snapshot(&root).await.unwrap_or_default();
                    let _ = tx.send(AppMsg::GitActionDone {
                        notice: format!("Worktree error: {err}"),
                        snapshot: snap,
                    });
                    egui_ctx.request_repaint();
                }
            }
        });
    }

    fn open_session(&mut self, id: &str) {
        self.session_id = Some(id.to_string());
        self.messages = self
            .core
            .sessions
            .messages(id)
            .unwrap_or_default()
            .into_iter()
            .filter(|message| message.role != "user" || !message.content.starts_with("Tool results:"))
            .map(|message| UiMessage::new(message.role, strip_tools(&message.content)))
            .collect();
        self.diffs = self
            .core
            .sessions
            .changes(id)
            .unwrap_or_default()
            .into_iter()
            .map(|(path, diff)| FileDiff { path, diff })
            .collect();
        self.activity = self
            .core
            .sessions
            .tool_runs(id)
            .unwrap_or_default()
            .into_iter()
            .map(|run| {
                format!(
                    "{} {} — {}",
                    run.name,
                    if run.ok { "ok" } else { "failed" },
                    run.output.lines().next().unwrap_or("")
                )
            })
            .collect();
        self.diff_index = 0;
        self.streaming.clear();
        self.streaming_segments.clear();
        self.active_tool = None;
    }

    fn send(&mut self, ctx: &egui::Context) {
        if self.running {
            return;
        }
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let project = PathBuf::from(&self.project);
        if !project.is_dir() {
            self.status = "Choose a project directory that exists.".into();
            return;
        }
        self.input.clear();
        self.messages.push(UiMessage::new("user", prompt.clone()));
        self.streaming.clear();
        self.streaming_segments.clear();
        self.active_tool = None;
        self.running = true;
        self.status = "Streaming response…".into();
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let egui_ctx = ctx.clone();
        let model = self.model_ids.get(self.model_index).cloned();
        let permission = self.permission;
        let session_id = self.session_id.clone();
        self.rt.spawn(async move {
            let result = core
                .run_task(TaskRequest {
                    project,
                    prompt,
                    session_id,
                    model,
                    permission: Some(permission),
                    yes: false,
                    events: tx.clone(),
                    cancel,
                })
                .await;
            if let Err(err) = result {
                let _ = tx.send(AgentEvent::Failed {
                    message: err.to_string(),
                });
            }
            let _ = tx.send(AgentEvent::Status("__idle__".into()));
            egui_ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut dirty_stream = false;
        while let Ok(event) = self.events_rx.try_recv() {
            if matches!(event, AgentEvent::MessageChunk { .. }) {
                dirty_stream = true;
            }
            self.on_event(event, ctx);
        }
        if dirty_stream {
            self.streaming_segments = parse_segments(&self.streaming);
        }
        while let Ok(msg) = self.app_rx.try_recv() {
            self.on_app_msg(msg);
        }
        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn on_app_msg(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::GitSnapshot(snap) => {
                self.git = snap;
                self.git_loading = false;
            }
            AppMsg::GitActionDone { notice, snapshot } => {
                self.status = notice;
                self.git = snapshot;
                self.git_loading = false;
            }
            AppMsg::WorktreeCreated { path, snapshot } => {
                let _ = self.core.sessions.remember_project(&path);
                if let Some(id) = &self.session_id {
                    let _ = self.core.sessions.relocate_project(id, &path);
                }
                self.project = path.display().to_string();
                self.git = snapshot;
                self.git_loading = false;
                self.reload_sessions();
                self.status = format!("Isolated thread in worktree {}", path.display());
            }
            AppMsg::FileDiffLoaded { title, diff } => {
                self.custom_diff_title = title;
                self.custom_diff_text = diff;
                self.diff_scope = DiffScope::Unstaged;
                self.right = RightTab::Diff;
                self.show_inspector = true;
            }
            AppMsg::UpdateChecked(res) => match res {
                Ok(info) => {
                    if info.update_available {
                        self.status = format!("Update available: {} (current {})", info.tag_name, info.current_version);
                    } else {
                        self.status = format!("Serika Code v{} is up to date", info.current_version);
                    }
                    self.update_info = Some(info);
                }
                Err(err) => {
                    self.status = format!("Update check failed: {err}");
                }
            },
            AppMsg::UpdateInstalled(res) => {
                self.updating = false;
                match res {
                    Ok(outcome) => {
                        self.status = format!(
                            "Updated to v{} in {}. Restart Serika Code to run the new build.",
                            outcome.installed_version,
                            outcome.install_dir.display()
                        );
                        if let Some(info) = &mut self.update_info {
                            info.update_available = false;
                        }
                    }
                    Err(err) => {
                        self.status = format!("Update failed: {err}");
                    }
                }
            }
            AppMsg::SetupCompleted(res) => match res {
                Ok(report) => {
                    self.status = format!(
                        "Installed to {} ({} binaries, {} shortcuts)",
                        report.bin_dir.display(),
                        report.installed_binaries.len(),
                        report.shortcuts_created.len()
                    );
                }
                Err(err) => {
                    self.status = format!("Setup failed: {err}");
                }
            },
        }
    }

    fn on_event(&mut self, event: AgentEvent, ctx: &egui::Context) {
        match event {
            AgentEvent::Started { session_id } => {
                self.session_id = Some(session_id);
                self.reload_sessions();
            }
            AgentEvent::Status(text) if text == "__idle__" => {
                self.running = false;
                self.cancel = None;
                self.active_tool = None;
                if !self.streaming.is_empty() {
                    let text = std::mem::take(&mut self.streaming);
                    self.streaming_segments.clear();
                    self.messages.push(UiMessage::new("assistant", text));
                }
                self.status = "Ready".into();
                self.reload_sessions();
                self.refresh_git(ctx);
            }
            AgentEvent::Status(text) if text.starts_with("catalog\n") => {
                self.model_ids.clear();
                self.model_labels.clear();
                for line in text.lines().skip(1) {
                    if let Some((id, label)) = line.split_once('\t') {
                        self.model_ids.push(id.to_string());
                        self.model_labels.push(label.to_string());
                    }
                }
                if let Some(selected) = &self.core.config.model {
                    if let Some(index) = self.model_ids.iter().position(|id| id == selected) {
                        self.model_index = index;
                    }
                }
            }
            AgentEvent::Status(text) => {
                if text.starts_with("signed in as ") {
                    self.account = text.trim_start_matches("signed in as ").to_string();
                }
                self.status = text;
            }
            AgentEvent::MessageChunk { text } => {
                if !text.is_empty() {
                    self.streaming.push_str(&text);
                }
            }
            AgentEvent::MessageCompleted { text } => {
                self.streaming.clear();
                self.streaming_segments.clear();
                let cleaned = strip_tools(&text);
                if !cleaned.is_empty() {
                    self.messages.push(UiMessage::new("assistant", cleaned));
                }
            }
            AgentEvent::ToolRequested { name, .. } => {
                self.active_tool = Some(name.clone());
                self.activity.push(format!("▸ {name}"));
            }
            AgentEvent::ApprovalRequired {
                id,
                title,
                detail,
                destructive,
            } => {
                self.approval = Some(PendingApproval {
                    id,
                    title,
                    detail,
                    destructive,
                });
                self.status = "Waiting for approval".into();
            }
            AgentEvent::ToolOutput { stream, .. } => {
                if self.activity.len() > 120 {
                    self.activity.drain(0..60);
                }
                self.activity.push(stream);
            }
            AgentEvent::ToolFinished {
                name, ok, summary, ..
            } => {
                self.active_tool = None;
                self.activity.push(format!(
                    "{} {} — {summary}",
                    if ok { "✓" } else { "✗" },
                    name
                ));
            }
            AgentEvent::FileChanged(diff) => {
                self.activity.push(format!("✎ edited {}", diff.path));
                self.diffs.push(diff);
                self.diff_index = self.diffs.len().saturating_sub(1);
                self.diff_scope = DiffScope::Session;
                self.right = RightTab::Diff;
                self.refresh_git(ctx);
            }
            AgentEvent::UsageUpdated(usage) => {
                self.status = usage.summary_line();
                self.usage = Some(usage);
            }
            AgentEvent::Completed { summary } => {
                let cleaned = strip_tools(&summary);
                if self.messages.last().map(|m| m.raw.as_str()) != Some(cleaned.as_str())
                    && !cleaned.is_empty()
                {
                    self.messages.push(UiMessage::new("assistant", cleaned));
                }
            }
            AgentEvent::Failed { message } => {
                self.running = false;
                self.active_tool = None;
                self.status = message.clone();
                self.messages.push(UiMessage::new("error", message));
            }
            AgentEvent::Cancelled => {
                self.running = false;
                self.active_tool = None;
                self.status = "Cancelled".into();
            }
            AgentEvent::ReasoningChunk { .. } | AgentEvent::ToolStarted { .. } => {}
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (toggle_k, toggle_b, new_t, open_settings, esc) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::K),
                i.modifiers.command && i.key_pressed(egui::Key::B),
                i.modifiers.command && i.key_pressed(egui::Key::N),
                i.modifiers.command && i.key_pressed(egui::Key::Comma),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if toggle_k {
            self.show_palette = !self.show_palette;
            self.palette_query.clear();
        }
        if toggle_b {
            self.show_inspector = !self.show_inspector;
        }
        if new_t {
            self.new_thread(false, ctx);
        }
        if open_settings {
            self.show_settings = !self.show_settings;
        }
        if esc {
            if self.show_palette {
                self.show_palette = false;
            } else if self.running {
                if let Some(cancel) = &self.cancel {
                    cancel.cancel();
                }
                self.core.approvals.deny_all();
            }
        }
    }
}

impl eframe::App for SerikaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.smoke {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.poll(&ctx);
        self.handle_shortcuts(&ctx);
        self.top_bar(ui, &ctx);
        self.sidebar(ui, &ctx);
        if self.show_inspector {
            self.right_panel(ui, &ctx);
        }
        self.conversation(ui, &ctx);
        self.settings_window(&ctx);
        self.account_window(&ctx);
        self.command_palette_window(&ctx);
    }
}

impl SerikaApp {
    fn top_bar(&mut self, root_ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::top("top_header")
            .exact_size(44.0)
            .show(root_ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("◆ SERIKA CODE")
                            .strong()
                            .size(14.0)
                            .color(egui::Color32::from_rgb(244, 63, 94)),
                    );
                    ui.label(
                        egui::RichText::new(format!("v{}", serika_runtime::client::VERSION))
                            .small()
                            .color(egui::Color32::from_rgb(130, 134, 150)),
                    );
                    ui.separator();

                    // Project / Scratch indicator
                    let project_name = PathBuf::from(&self.project)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&self.project)
                        .to_string();

                    if self.is_scratch() {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(48, 24, 40))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("✦ Scratch Workspace")
                                        .small()
                                        .color(egui::Color32::from_rgb(251, 113, 133)),
                                );
                            });
                        if ui
                            .button(egui::RichText::new("Turn into Project…").small())
                            .on_hover_text("Bind this scratch thread to a real project directory")
                            .clicked()
                        {
                            self.turn_into_project(ctx);
                        }
                    } else {
                        if ui
                            .button(egui::RichText::new(format!("📁 {project_name}")).small())
                            .on_hover_text(&self.project)
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                let _ = self.core.sessions.remember_project(&path);
                                self.project = path.display().to_string();
                                self.session_id = None;
                                self.messages.clear();
                                self.reload_sessions();
                                self.refresh_git(ctx);
                            }
                        }
                    }

                    // Git Branch & Worktree Pill
                    if self.git.is_repo {
                        let dirty = self.git.entries.len();
                        let branch_label = if dirty > 0 {
                            format!("⎇ {} ({dirty}*)", self.git.branch)
                        } else {
                            format!("⎇ {}", self.git.branch)
                        };
                        let branch_color = if dirty > 0 {
                            egui::Color32::from_rgb(245, 158, 11)
                        } else {
                            egui::Color32::from_rgb(16, 185, 129)
                        };
                        if ui
                            .button(egui::RichText::new(branch_label).small().color(branch_color))
                            .on_hover_text("Open Source Control & Branch Manager")
                            .clicked()
                        {
                            self.show_inspector = true;
                            self.right = RightTab::Git;
                        }
                        if ui
                            .button(egui::RichText::new("⑂ Isolate Worktree").small())
                            .on_hover_text(
                                "T3-style isolated git worktree: spawn a dedicated branch & worktree for this thread",
                            )
                            .clicked()
                        {
                            self.isolate_in_worktree(ctx);
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let inspector_label = if self.show_inspector {
                            "◫ Inspector"
                        } else {
                            "▭ Inspector"
                        };
                        if ui
                            .button(egui::RichText::new(inspector_label).small())
                            .on_hover_text("Toggle Right Inspector (Ctrl+B)")
                            .clicked()
                        {
                            self.show_inspector = !self.show_inspector;
                        }

                        if ui
                            .button(egui::RichText::new("⌘K Palette").small())
                            .on_hover_text("Open Command Palette (Ctrl+K)")
                            .clicked()
                        {
                            self.show_palette = !self.show_palette;
                            self.palette_query.clear();
                        }

                        if let Some(info) = &self.update_info {
                            if info.update_available {
                                let btn = egui::Button::new(
                                    egui::RichText::new(format!("⬆ Update {}", info.tag_name))
                                        .small()
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(egui::Color32::from_rgb(225, 29, 72));
                                if ui.add_enabled(!self.updating, btn).clicked() {
                                    self.trigger_self_update(ctx);
                                }
                            }
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, root_ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut action: Option<SidebarAction> = None;

        egui::Panel::left("sidebar")
            .exact_size(270.0)
            .show(root_ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let new_btn = egui::Button::new(
                        egui::RichText::new("+ New Thread")
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(egui::Color32::from_rgb(225, 29, 72))
                    .min_size(egui::vec2(150.0, 30.0));
                    if ui.add(new_btn).on_hover_text("Start a new thread in this project (Ctrl+N)").clicked() {
                        self.new_thread(false, ctx);
                    }
                    let scratch_btn = egui::Button::new(
                        egui::RichText::new("✦ Scratch")
                            .small()
                            .color(egui::Color32::from_rgb(251, 113, 133)),
                    )
                    .min_size(egui::vec2(88.0, 30.0));
                    if ui
                        .add(scratch_btn)
                        .on_hover_text("Start a project-less scratch thread")
                        .clicked()
                    {
                        self.new_thread(true, ctx);
                    }
                });

                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.session_filter)
                        .hint_text("🔍 Filter threads…")
                        .desired_width(f32::INFINITY),
                );

                if !self.recent_projects.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("WORKSPACES")
                            .size(10.5)
                            .strong()
                            .color(egui::Color32::from_rgb(120, 124, 142)),
                    );
                    egui::ScrollArea::horizontal()
                        .id_salt("recent_workspaces")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for proj in self.recent_projects.iter().take(6) {
                                    let name = std::path::Path::new(proj.as_str())
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(proj.as_str());
                                    let active = proj == &self.project;
                                    if ui
                                        .selectable_label(
                                            active,
                                            egui::RichText::new(name).small(),
                                        )
                                        .on_hover_text(proj)
                                        .clicked()
                                    {
                                        action = Some(SidebarAction::SwitchProject(proj.clone()));
                                    }
                                }
                            });
                        });
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("THREADS")
                            .size(10.5)
                            .strong()
                            .color(egui::Color32::from_rgb(120, 124, 142)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{}", self.sessions.len()))
                                .size(10.5)
                                .color(egui::Color32::from_rgb(120, 124, 142)),
                        );
                    });
                });

                let filter = self.session_filter.trim().to_ascii_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("threads_scroll")
                    .max_height(ui.available_height() - 96.0)
                    .show(ui, |ui| {
                        if self.sessions.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new("No threads in this workspace yet.")
                                    .small()
                                    .weak(),
                            );
                        }
                        for session in &self.sessions {
                            if !filter.is_empty()
                                && !session.title.to_ascii_lowercase().contains(&filter)
                            {
                                continue;
                            }
                            let selected = self.session_id.as_deref() == Some(session.id.as_str());
                            let frame_fill = if selected {
                                egui::Color32::from_rgb(38, 22, 34)
                            } else {
                                egui::Color32::from_rgb(20, 21, 28)
                            };
                            let stroke_color = if selected {
                                egui::Color32::from_rgb(225, 29, 72)
                            } else {
                                egui::Color32::from_rgb(34, 36, 48)
                            };
                            egui::Frame::new()
                                .fill(frame_fill)
                                .stroke(egui::Stroke::new(1.0, stroke_color))
                                .corner_radius(egui::CornerRadius::same(7))
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let dot = if selected && self.running {
                                            egui::RichText::new("●")
                                                .small()
                                                .color(egui::Color32::from_rgb(16, 185, 129))
                                        } else if selected {
                                            egui::RichText::new("●")
                                                .small()
                                                .color(egui::Color32::from_rgb(244, 63, 94))
                                        } else {
                                            egui::RichText::new("○")
                                                .small()
                                                .color(egui::Color32::from_rgb(100, 104, 120))
                                        };
                                        ui.label(dot);
                                        let title_text = if session.title.chars().count() > 24 {
                                            let short: String =
                                                session.title.chars().take(24).collect();
                                            format!("{short}…")
                                        } else {
                                            session.title.clone()
                                        };
                                        if ui
                                            .selectable_label(
                                                selected,
                                                egui::RichText::new(title_text).size(13.0),
                                            )
                                            .clicked()
                                        {
                                            action =
                                                Some(SidebarAction::OpenSession(session.id.clone()));
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button("×")
                                                    .on_hover_text("Delete thread")
                                                    .clicked()
                                                {
                                                    action = Some(SidebarAction::DeleteSession(
                                                        session.id.clone(),
                                                    ));
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                    });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("⚙ Settings").clicked() {
                            self.show_settings = true;
                        }
                        if ui.button("👤 Account").clicked() {
                            self.show_account = true;
                        }
                        if ui
                            .button("⬇ Setup")
                            .on_hover_text("Install Serika Code to PATH & create system shortcuts")
                            .clicked()
                        {
                            self.trigger_system_setup(ctx);
                        }
                    });
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(egui::Color32::from_rgb(165, 170, 188)),
                    );
                    ui.label(
                        egui::RichText::new(&self.account)
                            .small()
                            .strong()
                            .color(egui::Color32::from_rgb(251, 113, 133)),
                    );
                    ui.separator();
                });
            });

        if let Some(act) = action {
            match act {
                SidebarAction::OpenSession(id) => self.open_session(&id),
                SidebarAction::DeleteSession(id) => {
                    let _ = self.core.sessions.delete(&id);
                    if self.session_id.as_deref() == Some(id.as_str()) {
                        self.session_id = None;
                        self.messages.clear();
                        self.diffs.clear();
                    }
                    self.reload_sessions();
                }
                SidebarAction::SwitchProject(proj) => {
                    self.project = proj;
                    self.session_id = None;
                    self.messages.clear();
                    self.diffs.clear();
                    self.reload_sessions();
                    self.refresh_git(ctx);
                }
            }
        }
    }

    fn conversation(&mut self, root_ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CentralPanel::default().show(root_ui, |ui| {
            let avail_h = ui.available_height();
            let composer_h = if self.approval.is_some() { 215.0 } else { 145.0 };
            let scroll_h = (avail_h - composer_h).max(160.0);

            let mut starter_prompt: Option<String> = None;

            egui::ScrollArea::vertical()
                .id_salt("conversation_scroll")
                .max_height(scroll_h)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.messages.is_empty() && self.streaming.is_empty() {
                        ui.add_space(36.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("◆ Serika Code")
                                    .size(26.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(244, 63, 94)),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Fast native coding agent harness · Isolated worktrees · Granular turn diffs",
                                )
                                .color(egui::Color32::from_rgb(150, 155, 172)),
                            );
                            ui.add_space(20.0);
                        });

                        ui.columns(2, |cols| {
                            let cards = [
                                (
                                    "🧭 Architecture Overview",
                                    "Map the crates, entrypoints, and data flow in this repository.",
                                ),
                                (
                                    "🔍 Review Uncommitted Changes",
                                    "Inspect the current git status and diff, and point out any bugs or edge cases.",
                                ),
                                (
                                    "🧪 Run & Verify Tests",
                                    "Run the test suite in this workspace and fix any failing tests or warnings.",
                                ),
                                (
                                    "⚡ Performance & UX Audit",
                                    "Find any blocking calls or hot-path allocations and optimize them.",
                                ),
                            ];
                            for (i, (title, prompt)) in cards.iter().enumerate() {
                                let col = &mut cols[i % 2];
                                col.add_space(6.0);
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(22, 23, 31))
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgb(42, 44, 58),
                                    ))
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .inner_margin(egui::Margin::same(12))
                                    .show(col, |ui| {
                                        ui.label(egui::RichText::new(*title).strong().size(14.0));
                                        ui.add_space(2.0);
                                        ui.label(
                                            egui::RichText::new(*prompt)
                                                .small()
                                                .color(egui::Color32::from_rgb(155, 160, 178)),
                                        );
                                        ui.add_space(6.0);
                                        if ui.small_button("Use prompt →").clicked() {
                                            starter_prompt = Some((*prompt).to_string());
                                        }
                                    });
                            }
                        });
                    }

                    for message in &self.messages {
                        message_card(ui, &message.role, &message.raw, &message.segments);
                    }
                    if !self.streaming.is_empty() {
                        message_card(ui, "assistant", &self.streaming, &self.streaming_segments);
                    }
                    if let Some(tool) = &self.active_tool {
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(32, 24, 42))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(168, 85, 247)))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 5))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        egui::RichText::new(format!("Executing tool: {tool}"))
                                            .small()
                                            .monospace()
                                            .color(egui::Color32::from_rgb(216, 180, 254)),
                                    );
                                });
                            });
                    }
                });

            if let Some(prompt) = starter_prompt {
                self.input = prompt;
            }

            ui.add_space(6.0);
            if let Some(approval) = self.approval.clone() {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(44, 24, 30))
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgb(244, 63, 94),
                    ))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("⚠ Allow {}?", approval.title))
                                    .strong()
                                    .color(egui::Color32::from_rgb(253, 164, 175)),
                            );
                            if approval.destructive {
                                ui.colored_label(
                                    egui::Color32::from_rgb(245, 158, 11),
                                    "[DESTRUCTIVE]",
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Deny").clicked() {
                                        self.core.approvals.resolve(&approval.id, false);
                                        self.approval = None;
                                    }
                                    let allow_btn = egui::Button::new(
                                        egui::RichText::new("Allow").strong().color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(16, 185, 129));
                                    if ui.add(allow_btn).clicked() {
                                        self.core.approvals.resolve(&approval.id, true);
                                        self.approval = None;
                                        self.status = "Working…".into();
                                    }
                                },
                            );
                        });
                        ui.label(egui::RichText::new(approval.detail).small().monospace());
                    });
                ui.add_space(4.0);
            }

            // Floating T3-style Composer Card
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(19, 20, 27))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(46, 48, 64)))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    let hint = if cfg!(target_os = "macos") {
                        "Ask Serika Code to inspect, edit, or test… (⌘+Enter to send, ⌘+K for palette)"
                    } else {
                        "Ask Serika Code to inspect, edit, or test… (Ctrl+Enter to send, Ctrl+K for palette)"
                    };
                    let editor = egui::TextEdit::multiline(&mut self.input)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .hint_text(hint)
                        .show(ui);

                    let send_shortcut = editor.response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && input.modifiers.command
                        });
                    if send_shortcut && self.input.ends_with('\n') {
                        self.input.pop();
                        if self.input.ends_with('\r') {
                            self.input.pop();
                        }
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        // Model selector pill right inside the composer
                        if !self.model_labels.is_empty() {
                            let current_model = self
                                .model_labels
                                .get(self.model_index)
                                .cloned()
                                .unwrap_or_else(|| "Select model".into());
                            egui::ComboBox::from_id_salt("composer_model")
                                .width(230.0)
                                .selected_text(egui::RichText::new(current_model).small())
                                .show_ui(ui, |ui| {
                                    for (index, label) in self.model_labels.iter().enumerate() {
                                        ui.selectable_value(&mut self.model_index, index, label);
                                    }
                                });
                        } else {
                            ui.label(egui::RichText::new("Loading models…").small().weak());
                        }

                        // Permission pill
                        egui::ComboBox::from_id_salt("composer_permission")
                            .width(120.0)
                            .selected_text(
                                egui::RichText::new(format!("🛡 {}", self.permission.as_str()))
                                    .small(),
                            )
                            .show_ui(ui, |ui| {
                                for (mode, label) in [
                                    (PermissionMode::ReadOnly, "read_only"),
                                    (PermissionMode::Standard, "standard"),
                                    (PermissionMode::Restricted, "restricted"),
                                    (PermissionMode::Full, "full"),
                                ] {
                                    ui.selectable_value(&mut self.permission, mode, label);
                                }
                            });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let send_btn = egui::Button::new(
                                egui::RichText::new("Send ⏎")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(225, 29, 72))
                            .min_size(egui::vec2(84.0, 28.0));

                            if ui.add_enabled(!self.running, send_btn).clicked() || send_shortcut {
                                self.send(ctx);
                            }
                            if self.running {
                                let stop_btn = egui::Button::new(
                                    egui::RichText::new("■ Stop")
                                        .color(egui::Color32::from_rgb(253, 164, 175)),
                                )
                                .min_size(egui::vec2(72.0, 28.0));
                                if ui.add(stop_btn).clicked() {
                                    if let Some(cancel) = &self.cancel {
                                        cancel.cancel();
                                    }
                                    self.core.approvals.deny_all();
                                }
                            }
                        });
                    });
                });
        });
    }

    fn right_panel(&mut self, root_ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::right("inspector")
            .exact_size(400.0)
            .show(root_ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let git_label = if self.git.entries.is_empty() {
                        "Source Control".to_string()
                    } else {
                        format!("Source Control ({})", self.git.entries.len())
                    };
                    let diff_label = if self.diffs.is_empty() {
                        "Diffs".to_string()
                    } else {
                        format!("Diffs ({})", self.diffs.len())
                    };
                    if ui
                        .selectable_label(self.right == RightTab::Git, git_label)
                        .clicked()
                    {
                        self.right = RightTab::Git;
                        self.refresh_git(ctx);
                    }
                    if ui
                        .selectable_label(self.right == RightTab::Diff, diff_label)
                        .clicked()
                    {
                        self.right = RightTab::Diff;
                    }
                    if ui
                        .selectable_label(self.right == RightTab::Activity, "Activity")
                        .clicked()
                    {
                        self.right = RightTab::Activity;
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("inspector_scroll")
                    .show(ui, |ui| match self.right {
                        RightTab::Git => self.git_panel(ui, ctx),
                        RightTab::Diff => self.diff_view(ui),
                        RightTab::Activity => {
                            if let Some(usage) = &self.usage {
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(22, 23, 31))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("ACCOUNT USAGE")
                                                .size(10.5)
                                                .strong()
                                                .color(egui::Color32::from_rgb(244, 63, 94)),
                                        );
                                        ui.label(egui::RichText::new(usage.summary_line()).small());
                                    });
                                ui.add_space(6.0);
                            }
                            if self.activity.is_empty() {
                                ui.label(
                                    egui::RichText::new("Tool runs and telemetry appear here.")
                                        .small()
                                        .weak(),
                                );
                            }
                            for line in &self.activity {
                                ui.label(egui::RichText::new(line).monospace().size(12.0));
                            }
                        }
                    });
            });
    }

    fn git_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.git.is_repo {
            ui.label("Current directory is not a Git repository.");
            if ui.button("↻ Check Again").clicked() {
                self.refresh_git(ctx);
            }
            return;
        }

        // Branch & Sync Bar
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(20, 21, 28))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 40, 52)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("⎇ {}", self.git.branch))
                            .strong()
                            .color(egui::Color32::from_rgb(16, 185, 129)),
                    );
                    if self.git.ahead > 0 || self.git.behind > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "↑{} ↓{}",
                                self.git.ahead, self.git.behind
                            ))
                            .small()
                            .color(egui::Color32::from_rgb(245, 158, 11)),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("↻").on_hover_text("Refresh Git status").clicked() {
                            self.refresh_git(ctx);
                        }
                        if ui.small_button("↑ Push").clicked() {
                            self.spawn_git_op(ctx, "Pushed to remote", |root| async move {
                                gitinfo::push(&root).await
                            });
                        }
                        if ui.small_button("↓ Pull").clicked() {
                            self.spawn_git_op(ctx, "Pulled from remote", |root| async move {
                                gitinfo::pull(&root).await
                            });
                        }
                    });
                });

                // Branch switcher & new branch / worktree
                if !self.git.branches.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Switch:").small().weak());
                        let current = self.git.branch.clone();
                        let branches = self.git.branches.clone();
                        egui::ComboBox::from_id_salt("git_branch_combo")
                            .selected_text(egui::RichText::new(&current).small())
                            .show_ui(ui, |ui| {
                                for b in branches {
                                    if ui.selectable_label(b == current, &b).clicked()
                                        && b != current
                                    {
                                        let target = b.clone();
                                        self.spawn_git_op(
                                            ctx,
                                            &format!("Switched to {target}"),
                                            move |root| async move {
                                                gitinfo::checkout_branch(&root, &target, false).await
                                            },
                                        );
                                    }
                                }
                            });
                    });
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.git_new_branch)
                            .hint_text("New branch / worktree name…")
                            .desired_width(180.0),
                    );
                    if ui.small_button("+ Branch").clicked()
                        && !self.git_new_branch.trim().is_empty()
                    {
                        let name = self.git_new_branch.trim().to_string();
                        self.git_new_branch.clear();
                        self.spawn_git_op(
                            ctx,
                            &format!("Created branch {name}"),
                            move |root| async move {
                                gitinfo::checkout_branch(&root, &name, true).await
                            },
                        );
                    }
                    if ui
                        .small_button("⑂ Worktree")
                        .on_hover_text("Create an isolated git worktree and move this thread to it")
                        .clicked()
                    {
                        self.isolate_in_worktree(ctx);
                    }
                });
            });

        ui.add_space(8.0);

        // Commit Composer
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(20, 21, 28))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 40, 52)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.git_commit_msg)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("Commit message (e.g. feat: add worktree isolation)…"),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has_msg = !self.git_commit_msg.trim().is_empty();
                    let commit_btn = egui::Button::new(
                        egui::RichText::new("✓ Commit Staged")
                            .small()
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(egui::Color32::from_rgb(225, 29, 72));
                    if ui.add_enabled(has_msg, commit_btn).clicked() {
                        let msg = self.git_commit_msg.trim().to_string();
                        self.git_commit_msg.clear();
                        self.spawn_git_op(ctx, "Committed staged changes", move |root| async move {
                            gitinfo::commit_staged(&root, &msg).await
                        });
                    }
                    if ui
                        .add_enabled(
                            has_msg,
                            egui::Button::new(egui::RichText::new("✓ Stage All & Commit").small()),
                        )
                        .clicked()
                    {
                        let msg = self.git_commit_msg.trim().to_string();
                        self.git_commit_msg.clear();
                        self.spawn_git_op(ctx, "Committed all changes", move |root| async move {
                            gitinfo::commit_all(&root, &msg).await
                        });
                    }
                });
            });

        ui.add_space(8.0);

        // Staged & Unstaged Changes
        let entries = self.git.entries.clone();
        let staged: Vec<_> = entries.iter().filter(|e| e.staged).cloned().collect();
        let unstaged: Vec<_> = entries
            .iter()
            .filter(|e| e.has_unstaged_changes())
            .cloned()
            .collect();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("STAGED CHANGES ({})", staged.len()))
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(16, 185, 129)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !staged.is_empty() && ui.small_button("Unstage All").clicked() {
                    self.spawn_git_op(ctx, "Unstaged all files", |root| async move {
                        gitinfo::unstage_all(&root).await
                    });
                }
            });
        });
        for item in &staged {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(16, 185, 129), item.badge());
                if ui
                    .selectable_label(false, egui::RichText::new(&item.path).monospace().size(12.0))
                    .on_hover_text("Click to inspect staged diff")
                    .clicked()
                {
                    self.open_git_file_diff(ctx, &item.path, true);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let path = item.path.clone();
                    if ui.small_button("−").on_hover_text("Unstage file").clicked() {
                        self.spawn_git_op(ctx, "Unstaged file", move |root| async move {
                            gitinfo::unstage_file(&root, &path).await
                        });
                    }
                });
            });
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("WORKING TREE ({})", unstaged.len()))
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(245, 158, 11)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !unstaged.is_empty() && ui.small_button("Stage All").clicked() {
                    self.spawn_git_op(ctx, "Staged all files", |root| async move {
                        gitinfo::stage_all(&root).await
                    });
                }
            });
        });
        if unstaged.is_empty() && staged.is_empty() {
            ui.label(egui::RichText::new("✓ Working tree is clean").small().weak());
        }
        for item in &unstaged {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(245, 158, 11), item.badge());
                if ui
                    .selectable_label(false, egui::RichText::new(&item.path).monospace().size(12.0))
                    .on_hover_text("Click to inspect working diff")
                    .clicked()
                {
                    self.open_git_file_diff(ctx, &item.path, false);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let path_stage = item.path.clone();
                    if ui.small_button("+").on_hover_text("Stage file").clicked() {
                        self.spawn_git_op(ctx, "Staged file", move |root| async move {
                            gitinfo::stage_file(&root, &path_stage).await
                        });
                    }
                    let path_discard = item.path.clone();
                    let untracked = item.untracked;
                    if ui
                        .small_button("↺")
                        .on_hover_text("Discard changes in file")
                        .clicked()
                    {
                        self.spawn_git_op(ctx, "Discarded file changes", move |root| async move {
                            gitinfo::discard_file(&root, &path_discard, untracked).await
                        });
                    }
                });
            });
        }

        // Worktrees section
        if self.git.worktrees.len() > 1 {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("ISOLATED WORKTREES")
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(168, 85, 247)),
            );
            let wts = self.git.worktrees.clone();
            for wt in wts {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("⑂ {}", wt.branch))
                            .small()
                            .strong(),
                    );
                    if ui
                        .small_button("Open")
                        .on_hover_text(&wt.path)
                        .clicked()
                    {
                        self.project = wt.path;
                        self.reload_sessions();
                        self.refresh_git(ctx);
                    }
                });
            }
        }

        // Commit log section
        if !self.git.commits.is_empty() {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("RECENT COMMITS")
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(130, 134, 150)),
            );
            for commit in &self.git.commits {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&commit.sha)
                            .monospace()
                            .size(11.5)
                            .color(egui::Color32::from_rgb(244, 63, 94)),
                    );
                    ui.label(egui::RichText::new(&commit.subject).size(12.0));
                });
                ui.label(
                    egui::RichText::new(format!("  {} · {}", commit.author, commit.relative_time))
                        .size(10.5)
                        .weak(),
                );
            }
        }
    }

    fn spawn_git_op<F, Fut>(&mut self, ctx: &egui::Context, label: &str, op: F)
    where
        F: FnOnce(PathBuf) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = serika_runtime::Result<String>> + Send + 'static,
    {
        let root = PathBuf::from(&self.project);
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        let label = label.to_string();
        self.rt.spawn(async move {
            let outcome = op(root.clone()).await;
            let notice = match outcome {
                Ok(out) if out == "(no output)" => label,
                Ok(out) => format!("{label}: {}", out.lines().next().unwrap_or("")),
                Err(err) => format!("Git error: {err}"),
            };
            let snapshot = gitinfo::snapshot(&root).await.unwrap_or_default();
            let _ = tx.send(AppMsg::GitActionDone { notice, snapshot });
            egui_ctx.request_repaint();
        });
    }

    fn open_git_file_diff(&mut self, ctx: &egui::Context, path: &str, staged: bool) {
        let root = PathBuf::from(&self.project);
        let path_owned = path.to_string();
        let tx = self.app_tx.clone();
        let egui_ctx = ctx.clone();
        self.rt.spawn(async move {
            let diff = gitinfo::file_diff(&root, &path_owned, staged)
                .await
                .unwrap_or_default();
            let title = format!(
                "{} ({})",
                path_owned,
                if staged { "staged" } else { "working tree" }
            );
            let _ = tx.send(AppMsg::FileDiffLoaded { title, diff });
            egui_ctx.request_repaint();
        });
    }

    fn diff_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.diff_scope, DiffScope::Session, "Turn Edits");
            ui.selectable_value(&mut self.diff_scope, DiffScope::Unstaged, "Unstaged");
            ui.selectable_value(&mut self.diff_scope, DiffScope::Staged, "Staged");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.diff_view_mode, DiffViewMode::Split, "Split");
                ui.selectable_value(&mut self.diff_view_mode, DiffViewMode::Unified, "Unified");
            });
        });
        ui.separator();

        let active_diff: &str = match self.diff_scope {
            DiffScope::Session => {
                if self.diffs.is_empty() {
                    ui.label(
                        egui::RichText::new("No agent file edits in this session turn yet.")
                            .small()
                            .weak(),
                    );
                    return;
                }
                let names: Vec<String> = self.diffs.iter().map(|d| d.path.clone()).collect();
                egui::ComboBox::from_id_salt("diff-file")
                    .selected_text(names.get(self.diff_index).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (index, name) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.diff_index, index, name);
                        }
                    });
                self.diffs
                    .get(self.diff_index)
                    .map(|d| d.diff.as_str())
                    .unwrap_or("")
            }
            DiffScope::Unstaged => {
                if !self.custom_diff_text.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&self.custom_diff_title).small().strong());
                        if ui.small_button("Show All Unstaged").clicked() {
                            self.custom_diff_text.clear();
                        }
                    });
                }
                if !self.custom_diff_text.is_empty() {
                    &self.custom_diff_text
                } else {
                    &self.git.unstaged_diff
                }
            }
            DiffScope::Staged => &self.git.staged_diff,
        };

        if active_diff.trim().is_empty() {
            ui.label(egui::RichText::new("No diff lines in this scope.").small().weak());
            return;
        }

        match self.diff_view_mode {
            DiffViewMode::Unified => render_unified_diff(ui, active_diff),
            DiffViewMode::Split => render_split_diff(ui, active_diff),
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        let mut trigger_update = false;
        let mut trigger_check = false;
        let mut trigger_setup = false;

        egui::Window::new("Settings & System Setup")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Default Model").strong());
                if !self.model_labels.is_empty() {
                    egui::ComboBox::from_id_salt("settings_model")
                        .selected_text(
                            self.model_labels
                                .get(self.model_index)
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for (index, label) in self.model_labels.iter().enumerate() {
                                ui.selectable_value(&mut self.model_index, index, label);
                            }
                        });
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Permission Mode").strong());
                for (mode, label) in [
                    (PermissionMode::ReadOnly, "Read only (no writes or commands)"),
                    (PermissionMode::Standard, "Standard (ask before edits & shell)"),
                    (PermissionMode::Restricted, "Restricted (workspace edits allowed, ask for shell)"),
                    (PermissionMode::Full, "Full access (autonomous inside workspace)"),
                ] {
                    ui.radio_value(&mut self.permission, mode, label);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Command timeout (s):");
                    ui.add(egui::TextEdit::singleline(&mut self.timeout).desired_width(64.0));
                    ui.label("Retention (days):");
                    ui.add(egui::TextEdit::singleline(&mut self.retention).desired_width(64.0));
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    if ui.button("T3 Obsidian").clicked() {
                        apply_theme(ctx, "dark");
                    }
                    if ui.button("Daylight").clicked() {
                        apply_theme(ctx, "light");
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("Updater & System Installation").strong());
                ui.horizontal(|ui| {
                    if ui.button("Check for Updates").clicked() {
                        trigger_check = true;
                    }
                    if ui.button("Self-Update Now").clicked() {
                        trigger_update = true;
                    }
                    if ui.button("Run System Setup (PATH & Shortcuts)").clicked() {
                        trigger_setup = true;
                    }
                });
                ui.add_space(6.0);
                if ui
                    .button(egui::RichText::new("Save Configuration").strong())
                    .clicked()
                {
                    self.persist_settings(ctx);
                }
            });
        self.show_settings = open;
        if trigger_check {
            self.check_updates(ctx, true);
        }
        if trigger_update {
            self.trigger_self_update(ctx);
        }
        if trigger_setup {
            self.trigger_system_setup(ctx);
        }
    }

    fn command_palette_window(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        let mut open = self.show_palette;
        let mut chosen_action: Option<&str> = None;

        egui::Window::new("⌘K Command Palette")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0))
            .fixed_size(egui::vec2(460.0, 320.0))
            .show(ctx, |ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .hint_text("Type a command (e.g. worktree, commit, update, setup, theme)…")
                        .desired_width(f32::INFINITY),
                );
                resp.request_focus();
                ui.separator();

                let q = self.palette_query.trim().to_ascii_lowercase();
                let commands = [
                    ("new_thread", "+ Start New Thread in Current Project (Ctrl+N)"),
                    ("scratch_thread", "✦ Start New Scratch Thread (No Project Needed)"),
                    ("turn_into_project", "📁 Turn Scratch Thread into Project…"),
                    ("isolate_worktree", "⑂ Isolate Thread in Dedicated Git Worktree"),
                    ("refresh_git", "↻ Refresh Git Status & Commit Log"),
                    ("stage_all", "+ Git: Stage All Changes"),
                    ("toggle_split", "◫ Diff: Toggle Unified / Split Side-by-Side View"),
                    ("check_updates", "⬆ Check for Serika Code Updates"),
                    ("run_setup", "⬇ Install Serika Code to System PATH & Shortcuts"),
                    ("open_settings", "⚙ Open Settings"),
                    ("open_account", "👤 Open Serika Account & Usage"),
                ];

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, label) in commands {
                        if !q.is_empty() && !label.to_ascii_lowercase().contains(&q) {
                            continue;
                        }
                        if ui
                            .selectable_label(false, egui::RichText::new(label).size(13.5))
                            .clicked()
                        {
                            chosen_action = Some(id);
                        }
                    }
                });
            });

        self.show_palette = open;
        if let Some(cmd) = chosen_action {
            self.show_palette = false;
            match cmd {
                "new_thread" => self.new_thread(false, ctx),
                "scratch_thread" => self.new_thread(true, ctx),
                "turn_into_project" => self.turn_into_project(ctx),
                "isolate_worktree" => self.isolate_in_worktree(ctx),
                "refresh_git" => self.refresh_git(ctx),
                "stage_all" => {
                    self.spawn_git_op(ctx, "Staged all files", |root| async move {
                        gitinfo::stage_all(&root).await
                    });
                }
                "toggle_split" => {
                    self.diff_view_mode = match self.diff_view_mode {
                        DiffViewMode::Unified => DiffViewMode::Split,
                        DiffViewMode::Split => DiffViewMode::Unified,
                    };
                    self.show_inspector = true;
                    self.right = RightTab::Diff;
                }
                "check_updates" => self.check_updates(ctx, true),
                "run_setup" => self.trigger_system_setup(ctx),
                "open_settings" => self.show_settings = true,
                "open_account" => self.show_account = true,
                _ => {}
            }
        }
    }

    fn persist_settings(&mut self, ctx: &egui::Context) {
        let mut config = self.core.config.clone();
        config.permission = self.permission;
        config.model = self.model_ids.get(self.model_index).cloned();
        if let Ok(timeout) = self.timeout.parse() {
            config.command_timeout_secs = timeout;
        }
        if let Ok(days) = self.retention.parse() {
            config.session_retention_days = days;
        }
        config.theme = if ctx.global_style().visuals.dark_mode {
            "dark"
        } else {
            "light"
        }
        .into();
        if let Err(err) = config.save(&self.core.paths.config_file) {
            self.status = err.to_string();
        } else {
            self.status = "Settings saved.".into();
        }
    }

    fn account_window(&mut self, ctx: &egui::Context) {
        if !self.show_account {
            return;
        }
        let mut open = self.show_account;
        egui::Window::new("Serika Account")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(&self.account).strong());
                if let Some(usage) = &self.usage {
                    ui.label(usage.summary_line());
                }
                ui.separator();
                ui.label("Email");
                ui.text_edit_singleline(&mut self.login_email);
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut self.login_password).password(true));
                ui.label("2FA Code (optional)");
                ui.text_edit_singleline(&mut self.login_code);
                ui.horizontal(|ui| {
                    if ui.button("Sign in").clicked() {
                        self.do_login();
                    }
                    if ui.button("Send Email Magic Link").clicked() {
                        self.do_magic_request();
                    }
                    if ui.button("Sign out").clicked() {
                        self.do_logout();
                    }
                });
                ui.separator();
                ui.label("Magic-link token");
                ui.text_edit_singleline(&mut self.login_magic);
                if ui.button("Use magic-link token").clicked() {
                    self.do_magic_consume();
                }
                if !self.login_note.is_empty() {
                    ui.label(&self.login_note);
                }
            });
        self.show_account = open;
    }

    fn do_login(&mut self) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let email = self.login_email.clone();
        let mut password = self.login_password.clone();
        let code = {
            let code = self.login_code.trim().to_string();
            if code.is_empty() { None } else { Some(code) }
        };
        self.login_password.clear();
        self.rt.spawn(async move {
            let outcome = core.login_password(&email, &password, code.as_deref()).await;
            password.zeroize();
            report_login(outcome, &tx);
        });
    }

    fn do_magic_request(&mut self) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let email = self.login_email.clone();
        self.rt.spawn(async move {
            match core.request_magic_link(&email).await {
                Ok(message) => {
                    let _ = tx.send(AgentEvent::Status(message));
                }
                Err(err) => {
                    let _ = tx.send(AgentEvent::Failed {
                        message: err.to_string(),
                    });
                }
            }
        });
    }

    fn do_magic_consume(&mut self) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        let token = self.login_magic.trim().to_string();
        let code = {
            let code = self.login_code.trim().to_string();
            if code.is_empty() { None } else { Some(code) }
        };
        self.rt.spawn(async move {
            report_login(core.consume_magic_link(&token, code.as_deref()).await, &tx);
        });
    }

    fn do_logout(&mut self) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        self.rt.spawn(async move {
            match core.logout().await {
                Ok(()) => {
                    let _ = tx.send(AgentEvent::Status("signed out".into()));
                }
                Err(err) => {
                    let _ = tx.send(AgentEvent::Failed {
                        message: err.to_string(),
                    });
                }
            }
        });
        self.account = "Not signed in".into();
    }
}

fn report_login(outcome: serika_runtime::Result<LoginOutcome>, tx: &UnboundedSender<AgentEvent>) {
    match outcome {
        Ok(LoginOutcome::SignedIn(session)) => {
            let _ = tx.send(AgentEvent::Status(format!(
                "signed in as {}",
                session.username
            )));
        }
        Ok(LoginOutcome::NeedsSecondFactor { message }) => {
            let _ = tx.send(AgentEvent::Status(message));
        }
        Err(err) => {
            let _ = tx.send(AgentEvent::Failed {
                message: err.to_string(),
            });
        }
    }
}

fn message_card(ui: &mut egui::Ui, role: &str, raw: &str, segments: &[MessageSegment]) {
    let (badge, fill, border, badge_color) = match role {
        "user" => (
            "YOU",
            egui::Color32::from_rgb(28, 22, 34),
            egui::Color32::from_rgb(88, 36, 62),
            egui::Color32::from_rgb(251, 113, 133),
        ),
        "error" => (
            "ERROR",
            egui::Color32::from_rgb(46, 20, 24),
            egui::Color32::from_rgb(185, 28, 28),
            egui::Color32::from_rgb(252, 165, 165),
        ),
        _ => (
            "SERIKA",
            egui::Color32::from_rgb(20, 21, 28),
            egui::Color32::from_rgb(38, 40, 54),
            egui::Color32::from_rgb(168, 85, 247),
        ),
    };
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, border))
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(badge)
                        .size(10.5)
                        .strong()
                        .color(badge_color),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(raw.to_string());
                    }
                });
            });
            ui.add_space(4.0);
            for segment in segments {
                match segment {
                    MessageSegment::Text(text) => {
                        ui.label(egui::RichText::new(text).size(14.5));
                    }
                    MessageSegment::Code { lang, code } => {
                        ui.add_space(4.0);
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(12, 13, 18))
                            .stroke(egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgb(36, 38, 50),
                            ))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let tag = if lang.is_empty() { "code" } else { lang.as_str() };
                                    ui.label(
                                        egui::RichText::new(tag)
                                            .size(11.0)
                                            .monospace()
                                            .color(egui::Color32::from_rgb(140, 145, 165)),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("Copy snippet").clicked() {
                                                ui.ctx().copy_text(code.clone());
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    egui::RichText::new(code)
                                        .monospace()
                                        .size(12.5)
                                        .color(egui::Color32::from_rgb(226, 232, 240)),
                                );
                            });
                        ui.add_space(4.0);
                    }
                }
            }
        });
}

fn parse_segments(text: &str) -> Vec<MessageSegment> {
    let mut segments = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let before = rest[..start].trim();
        if !before.is_empty() {
            segments.push(MessageSegment::Text(before.to_string()));
        }
        let after = &rest[start + 3..];
        if let Some(end) = after.find("```") {
            let block = &after[..end];
            let (lang, code) = match block.find('\n') {
                Some(nl) => (block[..nl].trim().to_string(), block[nl + 1..].trim_end().to_string()),
                None => (String::new(), block.trim_end().to_string()),
            };
            segments.push(MessageSegment::Code { lang, code });
            rest = &after[end + 3..];
        } else {
            segments.push(MessageSegment::Text(rest.trim().to_string()));
            return segments;
        }
    }
    let tail = rest.trim();
    if !tail.is_empty() {
        segments.push(MessageSegment::Text(tail.to_string()));
    }
    segments
}

fn render_unified_diff(ui: &mut egui::Ui, diff: &str) {
    for line in diff.lines() {
        let (color, bg) = if line.starts_with('+') && !line.starts_with("+++") {
            (
                egui::Color32::from_rgb(110, 231, 183),
                egui::Color32::from_rgb(14, 38, 28),
            )
        } else if line.starts_with('-') && !line.starts_with("---") {
            (
                egui::Color32::from_rgb(252, 165, 165),
                egui::Color32::from_rgb(46, 18, 24),
            )
        } else if line.starts_with("@@") {
            (
                egui::Color32::from_rgb(196, 181, 253),
                egui::Color32::from_rgb(28, 22, 44),
            )
        } else {
            (
                egui::Color32::from_rgb(176, 180, 196),
                egui::Color32::TRANSPARENT,
            )
        };
        egui::Frame::new()
            .fill(bg)
            .inner_margin(egui::Margin::symmetric(4, 1))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(line).monospace().size(11.5).color(color));
            });
    }
}

fn render_split_diff(ui: &mut egui::Ui, diff: &str) {
    let mut left_lines = Vec::new();
    let mut right_lines = Vec::new();
    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        if line.starts_with("@@") {
            left_lines.push((line, egui::Color32::from_rgb(196, 181, 253)));
            right_lines.push((line, egui::Color32::from_rgb(196, 181, 253)));
        } else if line.starts_with('-') {
            left_lines.push((line, egui::Color32::from_rgb(252, 165, 165)));
        } else if line.starts_with('+') {
            right_lines.push((line, egui::Color32::from_rgb(110, 231, 183)));
        } else {
            left_lines.push((line, egui::Color32::from_rgb(165, 170, 186)));
            right_lines.push((line, egui::Color32::from_rgb(165, 170, 186)));
        }
    }
    ui.columns(2, |cols| {
        cols[0].label(egui::RichText::new("BEFORE").size(10.0).weak());
        for (line, color) in &left_lines {
            cols[0].label(egui::RichText::new(*line).monospace().size(11.0).color(*color));
        }
        cols[1].label(egui::RichText::new("AFTER").size(10.0).weak());
        for (line, color) in &right_lines {
            cols[1].label(egui::RichText::new(*line).monospace().size(11.0).color(*color));
        }
    });
}

fn strip_tools(text: &str) -> String {
    let (visible, calls) = serika_runtime::protocol::extract_tool_calls(text);
    if calls.is_empty() {
        text.to_string()
    } else if visible.is_empty() {
        format!("{} tool step(s)", calls.len())
    } else {
        visible
    }
}

fn apply_theme(ctx: &egui::Context, theme: &str) {
    let dark = theme != "light";
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if dark {
        // T3 Code carbon-obsidian palette with rose-violet highlights
        visuals.window_fill = egui::Color32::from_rgb(13, 14, 19);
        visuals.panel_fill = egui::Color32::from_rgb(15, 16, 22);
        visuals.extreme_bg_color = egui::Color32::from_rgb(9, 10, 14);
        visuals.faint_bg_color = egui::Color32::from_rgb(24, 25, 34);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(18, 19, 26);
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgb(36, 38, 50));
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 30, 40);
        visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgb(46, 48, 64));
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 44, 58);
        visuals.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgb(225, 29, 72));
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(225, 29, 72);
        visuals.selection.bg_fill = egui::Color32::from_rgb(159, 18, 57);
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(251, 113, 133));
        visuals.hyperlink_color = egui::Color32::from_rgb(251, 113, 133);
        visuals.override_text_color = Some(egui::Color32::from_rgb(236, 238, 245));
    }
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(19.0));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
    });
}
