use std::sync::Arc;

use eframe::egui;
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
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
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

struct UiMessage {
    role: String,
    text: String,
}

#[derive(Clone)]
struct PendingApproval {
    id: String,
    title: String,
    detail: String,
    destructive: bool,
}

struct SerikaApp {
    rt: tokio::runtime::Runtime,
    core: Arc<Runtime>,
    events_tx: UnboundedSender<AgentEvent>,
    events_rx: UnboundedReceiver<AgentEvent>,
    cancel: Option<CancellationToken>,
    smoke: bool,
    project: String,
    sessions: Vec<SessionSummary>,
    session_id: Option<String>,
    messages: Vec<UiMessage>,
    streaming: String,
    input: String,
    activity: Vec<String>,
    diffs: Vec<FileDiff>,
    diff_index: usize,
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
    right: RightTab,
    git_text: String,
    login_email: String,
    login_password: String,
    login_code: String,
    login_magic: String,
    login_note: String,
    timeout: String,
    retention: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightTab {
    Activity,
    Diff,
    Git,
}

impl SerikaApp {
    fn new(cc: &eframe::CreationContext<'_>, core: Arc<Runtime>, smoke: bool) -> Self {
        apply_theme(&cc.egui_ctx, &core.config.theme);
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio");
        let project = std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
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
            cancel: None,
            smoke,
            project,
            sessions: Vec::new(),
            session_id: None,
            messages: Vec::new(),
            streaming: String::new(),
            input: String::new(),
            activity: Vec::new(),
            diffs: Vec::new(),
            diff_index: 0,
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
            right: RightTab::Activity,
            git_text: String::new(),
            login_email: String::new(),
            login_password: String::new(),
            login_code: String::new(),
            login_magic: String::new(),
            login_note: String::new(),
            timeout,
            retention,
        };
        app.reload_sessions();
        app.bootstrap();
        app
    }

    fn bootstrap(&self) {
        self.load_models();
        let core = self.core.clone();
        let tx = self.events_tx.clone();
        self.rt.spawn(async move {
            if let Ok(usage) = core.usage().await {
                let _ = tx.send(AgentEvent::UsageUpdated(usage));
            }
        });
    }

    fn load_models(&self) {
        let core = self.core.clone();
        let tx = self.events_tx.clone();
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
                                if model.premium { " · premium" } else { "" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = tx.send(AgentEvent::Status(format!("catalog\n{payload}")));
                }
                Err(err) => {
                    let _ = tx.send(AgentEvent::Failed {
                        message: format!("Could not load models: {err}"),
                    });
                }
            }
        });
    }

    fn reload_sessions(&mut self) {
        let path = std::path::PathBuf::from(&self.project);
        self.sessions = self.core.sessions.list(Some(&path)).unwrap_or_default();
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
            .map(|message| UiMessage {
                role: message.role,
                text: strip_tools(&message.content),
            })
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
    }

    fn send(&mut self) {
        if self.running {
            return;
        }
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let project = std::path::PathBuf::from(&self.project);
        if !project.is_dir() {
            self.status = "Choose a project directory that exists.".into();
            return;
        }
        self.input.clear();
        self.messages.push(UiMessage { role: "user".into(), text: prompt.clone() });
        self.streaming.clear();
        self.running = true;
        self.status = "Working".into();
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let core = self.core.clone();
        let tx = self.events_tx.clone();
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
                let _ = tx.send(AgentEvent::Failed { message: err.to_string() });
            }
            let _ = tx.send(AgentEvent::Status("__idle__".into()));
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.on_event(event);
        }
        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started { session_id } => {
                self.session_id = Some(session_id);
                self.reload_sessions();
            }
            AgentEvent::Status(text) if text == "__idle__" => {
                self.running = false;
                self.cancel = None;
                if !self.streaming.is_empty() {
                    self.messages.push(UiMessage {
                        role: "assistant".into(),
                        text: std::mem::take(&mut self.streaming),
                    });
                }
                self.status = "Ready".into();
                self.reload_sessions();
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
            AgentEvent::Status(text) => self.status = text,
            AgentEvent::MessageChunk { text } => {
                if !text.is_empty() {
                    self.streaming.push_str(&text);
                }
            }
            AgentEvent::MessageCompleted { text } => {
                self.streaming.clear();
                if !text.is_empty() {
                    self.messages.push(UiMessage { role: "assistant".into(), text });
                }
            }
            AgentEvent::ToolRequested { name, .. } => self.activity.push(format!("request {name}")),
            AgentEvent::ApprovalRequired { id, title, detail, destructive } => {
                self.approval = Some(PendingApproval { id, title, detail, destructive });
                self.status = "Waiting for approval".into();
            }
            AgentEvent::ToolOutput { id, stream, .. } => {
                if id == "git" {
                    self.git_text = stream;
                    return;
                }
                if self.activity.len() > 80 {
                    self.activity.drain(0..40);
                }
                self.activity.push(stream);
            }
            AgentEvent::ToolFinished { name, ok, summary, .. } => {
                self.activity.push(format!("{} {} — {summary}", name, if ok { "ok" } else { "failed" }));
            }
            AgentEvent::FileChanged(diff) => {
                self.activity.push(format!("edited {}", diff.path));
                self.diffs.push(diff);
                self.diff_index = self.diffs.len().saturating_sub(1);
                self.right = RightTab::Diff;
            }
            AgentEvent::UsageUpdated(usage) => {
                self.status = usage.summary_line();
                self.usage = Some(usage);
            }
            AgentEvent::Completed { summary } => {
                if self.messages.last().map(|message| message.text.as_str()) != Some(summary.as_str()) && !summary.is_empty() {
                    self.messages.push(UiMessage { role: "assistant".into(), text: summary });
                }
            }
            AgentEvent::Failed { message } => {
                self.running = false;
                self.status = message.clone();
                self.messages.push(UiMessage { role: "error".into(), text: message });
            }
            AgentEvent::Cancelled => {
                self.running = false;
                self.status = "Cancelled".into();
            }
            AgentEvent::ReasoningChunk { .. } | AgentEvent::ToolStarted { .. } => {}
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
        self.sidebar(ui, &ctx);
        self.right_panel(ui);
        self.conversation(ui);
        self.settings_window(&ctx);
        self.account_window(&ctx);
    }
}

impl SerikaApp {
    fn sidebar(&mut self, root_ui: &mut egui::Ui, _ctx: &egui::Context) {
        egui::Panel::left("sidebar").exact_size(250.0).show(root_ui, |ui| {
            ui.add_space(8.0);
            ui.heading("Serika Code");
            ui.label(egui::RichText::new(&self.account).small().weak());
            ui.add_space(8.0);
            if ui.button("New session").clicked() {
                self.session_id = None;
                self.messages.clear();
                self.diffs.clear();
                self.activity.clear();
                self.streaming.clear();
            }
            if ui.button("Open project…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let _ = self.core.sessions.remember_project(&path);
                    self.project = path.display().to_string();
                    self.session_id = None;
                    self.messages.clear();
                    self.reload_sessions();
                }
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Project").small().weak());
            ui.add(egui::TextEdit::singleline(&mut self.project).desired_width(220.0));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Sessions").small().weak());
            let ids: Vec<String> = self.sessions.iter().map(|session| session.id.clone()).collect();
            let titles: Vec<String> = self.sessions.iter().map(|session| session.title.clone()).collect();
            for (id, title) in ids.into_iter().zip(titles) {
                let selected = self.session_id.as_deref() == Some(id.as_str());
                if ui.selectable_label(selected, title).clicked() {
                    self.open_session(&id);
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if ui.button("Account").clicked() {
                    self.show_account = true;
                }
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                ui.label(egui::RichText::new(&self.status).small());
            });
        });
    }

    fn conversation(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(root_ui, |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                if self.messages.is_empty() && self.streaming.is_empty() {
                    ui.add_space(24.0);
                    ui.label("Ask Serika Code to inspect this project, change it, and verify the result.");
                }
                for message in &self.messages {
                    message_block(ui, &message.role, &message.text);
                }
                if !self.streaming.is_empty() {
                    message_block(ui, "assistant", &self.streaming);
                }
            });
            ui.separator();
            if let Some(approval) = self.approval.clone() {
                ui.horizontal(|ui| {
                    ui.label(format!("Allow {}?", approval.title));
                    if approval.destructive {
                        ui.colored_label(egui::Color32::from_rgb(232, 168, 124), "destructive");
                    }
                });
                ui.label(approval.detail);
                ui.horizontal(|ui| {
                    if ui.button("Allow").clicked() {
                        self.core.approvals.resolve(&approval.id, true);
                        self.approval = None;
                        self.status = "Working".into();
                    }
                    if ui.button("Deny").clicked() {
                        self.core.approvals.resolve(&approval.id, false);
                        self.approval = None;
                    }
                });
            }
            let hint = if cfg!(target_os = "macos") {
                "Describe the task. ⌘+Enter sends."
            } else {
                "Describe the task. Ctrl+Enter sends."
            };
            let editor = egui::TextEdit::multiline(&mut self.input)
                .desired_rows(3)
                .hint_text(hint)
                .show(ui);
            let send = editor.response.has_focus()
                && ui.input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.command);
            if send && self.input.ends_with('\n') {
                self.input.pop();
                if self.input.ends_with('\r') {
                    self.input.pop();
                }
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.running, egui::Button::new("Send")).clicked() || send {
                    self.send();
                }
                if ui.add_enabled(self.running, egui::Button::new("Stop")).clicked() {
                    if let Some(cancel) = &self.cancel {
                        cancel.cancel();
                    }
                    self.core.approvals.deny_all();
                }
                ui.label(self.model_labels.get(self.model_index).cloned().unwrap_or_else(|| "model".into()));
            });
        });
    }

    fn right_panel(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::right("activity").exact_size(340.0).show(root_ui, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(self.right == RightTab::Activity, "Activity").clicked() {
                    self.right = RightTab::Activity;
                }
                if ui.selectable_label(self.right == RightTab::Diff, "Diff").clicked() {
                    self.right = RightTab::Diff;
                }
                if ui.selectable_label(self.right == RightTab::Git, "Git").clicked() {
                    self.right = RightTab::Git;
                    let project = self.project.clone();
                    let core_path = std::path::PathBuf::from(&project);
                    let tx = self.events_tx.clone();
                    self.rt.spawn(async move {
                        let status = serika_runtime::gitinfo::status(&core_path).await.unwrap_or_else(|err| err.to_string());
                        let diff = serika_runtime::gitinfo::diff(&core_path, None).await.unwrap_or_default();
                        let _ = tx.send(AgentEvent::ToolOutput {
                            id: "git".into(),
                            stream: format!("{status}\n\n{diff}"),
                        });
                    });
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| match self.right {
                RightTab::Activity => {
                    if let Some(usage) = &self.usage {
                        ui.label(usage.summary_line());
                        ui.separator();
                    }
                    for line in &self.activity {
                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                    }
                }
                RightTab::Diff => self.diff_view(ui),
                RightTab::Git => {
                    if self.git_text.is_empty() {
                        ui.label("Open this tab to load git status and the working diff.");
                    }
                    ui.label(egui::RichText::new(&self.git_text).monospace().size(12.0));
                }
            });
        });
    }

    fn diff_view(&mut self, ui: &mut egui::Ui) {
        if self.diffs.is_empty() {
            ui.label("No agent edits in this session yet.");
            return;
        }
        let names: Vec<String> = self.diffs.iter().map(|diff| diff.path.clone()).collect();
        egui::ComboBox::from_id_salt("diff-file")
            .selected_text(names.get(self.diff_index).cloned().unwrap_or_default())
            .show_ui(ui, |ui| {
                for (index, name) in names.iter().enumerate() {
                    ui.selectable_value(&mut self.diff_index, index, name);
                }
            });
        if let Some(diff) = self.diffs.get(self.diff_index) {
            for line in diff.diff.lines() {
                let color = if line.starts_with('+') && !line.starts_with("+++") {
                    egui::Color32::from_rgb(126, 186, 140)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    egui::Color32::from_rgb(214, 130, 130)
                } else {
                    egui::Color32::from_rgb(186, 188, 194)
                };
                ui.label(egui::RichText::new(line).monospace().size(12.0).color(color));
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("Settings").open(&mut open).show(ctx, |ui| {
            ui.label("Model");
            if !self.model_labels.is_empty() {
                egui::ComboBox::from_id_salt("model")
                    .selected_text(self.model_labels.get(self.model_index).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (index, label) in self.model_labels.iter().enumerate() {
                            ui.selectable_value(&mut self.model_index, index, label);
                        }
                    });
            }
            ui.label("Permission");
            for (mode, label) in [
                (PermissionMode::ReadOnly, "Read only"),
                (PermissionMode::Standard, "Standard"),
                (PermissionMode::Restricted, "Restricted"),
                (PermissionMode::Full, "Full access"),
            ] {
                ui.radio_value(&mut self.permission, mode, label);
            }
            ui.label("Command timeout (seconds)");
            ui.text_edit_singleline(&mut self.timeout);
            ui.label("Session retention (days)");
            ui.text_edit_singleline(&mut self.retention);
            ui.horizontal(|ui| {
                if ui.button("Dark").clicked() {
                    apply_theme(ctx, "dark");
                }
                if ui.button("Light").clicked() {
                    apply_theme(ctx, "light");
                }
            });
            if ui.button("Save").clicked() {
                self.persist_settings(ctx);
            }
        });
        self.show_settings = open;
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
        config.theme = if ctx.global_style().visuals.dark_mode { "dark" } else { "light" }.into();
        if let Err(err) = config.save(&self.core.paths.config_file) {
            self.status = err.to_string();
        } else {
            self.status = "Settings saved. Permission and model apply to the next task. Restart to apply timeout and retention.".into();
        }
    }

    fn account_window(&mut self, ctx: &egui::Context) {
        if !self.show_account {
            return;
        }
        let mut open = self.show_account;
        egui::Window::new("Account").open(&mut open).show(ctx, |ui| {
            ui.label(&self.account);
            if let Some(usage) = &self.usage {
                ui.label(usage.summary_line());
            }
            ui.separator();
            ui.label("Password sign-in");
            ui.text_edit_singleline(&mut self.login_email);
            ui.add(egui::TextEdit::singleline(&mut self.login_password).password(true));
            ui.text_edit_singleline(&mut self.login_code);
            ui.horizontal(|ui| {
                if ui.button("Sign in").clicked() {
                    self.do_login();
                }
                if ui.button("Email link").clicked() {
                    self.do_magic_request();
                }
                if ui.button("Sign out").clicked() {
                    self.do_logout();
                }
            });
            ui.label("Magic-link token");
            ui.text_edit_singleline(&mut self.login_magic);
            if ui.button("Use link token").clicked() {
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
                    let _ = tx.send(AgentEvent::Failed { message: err.to_string() });
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
                    let _ = tx.send(AgentEvent::Failed { message: err.to_string() });
                }
            }
        });
        self.account = "Not signed in".into();
    }
}

fn report_login(outcome: serika_runtime::Result<LoginOutcome>, tx: &UnboundedSender<AgentEvent>) {
    match outcome {
        Ok(LoginOutcome::SignedIn(session)) => {
            let _ = tx.send(AgentEvent::Status(format!("signed in as {}", session.username)));
        }
        Ok(LoginOutcome::NeedsSecondFactor { message }) => {
            let _ = tx.send(AgentEvent::Status(message));
        }
        Err(err) => {
            let _ = tx.send(AgentEvent::Failed { message: err.to_string() });
        }
    }
}

fn message_block(ui: &mut egui::Ui, role: &str, text: &str) {
    let (label, fill) = match role {
        "user" => ("You", egui::Color32::from_rgb(36, 42, 56)),
        "error" => ("Error", egui::Color32::from_rgb(58, 32, 32)),
        _ => ("Serika", egui::Color32::from_rgb(32, 33, 38)),
    };
    ui.add_space(6.0);
    ui.label(egui::RichText::new(label).small().weak());
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            render_rich(ui, text);
        });
}

fn render_rich(ui: &mut egui::Ui, text: &str) {
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        if start > 0 {
            ui.label(&rest[..start]);
        }
        let after = &rest[start + 3..];
        if let Some(end) = after.find("```") {
            let mut code = &after[..end];
            if let Some(newline) = code.find('\n') {
                code = &code[newline + 1..];
            }
            ui.label(egui::RichText::new(code.trim_end()).monospace());
            rest = &after[end + 3..];
        } else {
            ui.label(rest);
            return;
        }
    }
    if !rest.is_empty() {
        ui.label(rest);
    }
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
    let mut visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    if dark {
        visuals.window_fill = egui::Color32::from_rgb(20, 21, 24);
        visuals.panel_fill = egui::Color32::from_rgb(26, 27, 31);
        visuals.extreme_bg_color = egui::Color32::from_rgb(16, 17, 20);
        visuals.faint_bg_color = egui::Color32::from_rgb(34, 36, 42);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(26, 27, 31);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 42, 48);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(52, 56, 66);
        visuals.selection.bg_fill = egui::Color32::from_rgb(47, 79, 160);
        visuals.hyperlink_color = egui::Color32::from_rgb(142, 182, 255);
        visuals.override_text_color = Some(egui::Color32::from_rgb(230, 232, 236));
    }
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
}
