use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Key,
    Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit,
    TextStyle, Theme, Vec2,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use serika_runtime::client::LoginOutcome;
use serika_runtime::credentials::StoredSession;
use serika_runtime::gitinfo::{self, GitRepoSnapshot};
use serika_runtime::model::{AccountProfile, ChatModel, ModelCatalog, UsageSnapshot};
use serika_runtime::permissions::PermissionMode;
use serika_runtime::providers::{
    builtin_provider_models, detect_local_harnesses, LocalHarness, ProviderKind,
};
use serika_runtime::sessions::{SessionSummary, StoredMessage, ToolRun};
use serika_runtime::setup::{self, SetupOptions};
use serika_runtime::updater::{self, UpdateInfo};
use serika_runtime::{AgentEvent, Runtime, TaskOutcome, TaskRequest};

// =============================================================================
// Serika Brand Guidelines Palette (`serika-accounts/app/globals.css` + T3 Code)
// =============================================================================
const BG_DEEP: Color32 = Color32::from_rgb(12, 12, 14);
const BG_SIDEBAR: Color32 = Color32::from_rgb(12, 12, 14);
const BG_CARD: Color32 = Color32::from_rgb(20, 20, 22);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(28, 28, 32);
const BG_INPUT: Color32 = Color32::from_rgb(16, 16, 18);
const BG_ACTIVE_ROW: Color32 = Color32::from_rgb(32, 32, 36);

const BORDER_SUBTLE: Color32 = Color32::from_rgb(36, 36, 40);
const BORDER_STRONG: Color32 = Color32::from_rgb(54, 54, 60);

const TEXT_PRIMARY: Color32 = Color32::from_rgb(236, 236, 238);
const TEXT_MUTED: Color32 = Color32::from_rgb(154, 154, 162);
const TEXT_DIM: Color32 = Color32::from_rgb(108, 108, 116);

// Kept names so older call sites stay quiet. These are neutral, not a brand purple.
const SERIKA_VIOLET: Color32 = Color32::from_rgb(236, 236, 238);
const SERIKA_VIOLET_HOVER: Color32 = Color32::from_rgb(255, 255, 255);
const SERIKA_VIOLET_LIGHT: Color32 = Color32::from_rgb(196, 196, 204);
const SERIKA_VIOLET_SOFT: Color32 = Color32::from_rgb(32, 32, 36);

const EMERALD: Color32 = Color32::from_rgb(16, 185, 129); // #10B981 (--color-success)
const AMBER: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B (--color-warning)
const ROSE: Color32 = Color32::from_rgb(244, 63, 94); // #F43F5E (--color-danger)

// =============================================================================
// Custom Anti-Aliased Vector Icon Renderer (Zero Missing Glyph `▯` Boxes!)
// =============================================================================
#[derive(Debug, Clone, Copy)]
enum Icon {
    SerikaMark,
    Plus,
    Folder,
    GitBranch,
    Worktree,
    Search,
    Send,
    Stop,
    Sliders,
    Key,
    PanelRight,
    Sparkle,
    Check,
    Refresh,
    Terminal,
    Close,
}

fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.42;
    let stroke = Stroke::new(1.5, color);

    match icon {
        Icon::SerikaMark => {
            let top = Pos2::new(c.x, c.y - r * 1.15);
            let right = Pos2::new(c.x + r * 1.05, c.y);
            let bottom = Pos2::new(c.x, c.y + r * 1.15);
            let left = Pos2::new(c.x - r * 1.05, c.y);
            painter.add(egui::Shape::convex_polygon(
                vec![top, right, bottom, left],
                color,
                Stroke::NONE,
            ));
            let inner_r = r * 0.45;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x, c.y - inner_r),
                    Pos2::new(c.x + inner_r, c.y),
                    Pos2::new(c.x, c.y + inner_r),
                    Pos2::new(c.x - inner_r, c.y),
                ],
                BG_DEEP,
                Stroke::NONE,
            ));
        }
        Icon::Plus => {
            painter.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)], stroke);
        }
        Icon::Folder => {
            let box_rect =
                Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(r * 2.1, r * 1.5));
            let tab_rect = Rect::from_min_size(
                Pos2::new(box_rect.left(), box_rect.top() - r * 0.35),
                Vec2::new(r * 0.9, r * 0.45),
            );
            painter.rect_stroke(tab_rect, 1.5, stroke, StrokeKind::Inside);
            painter.rect_stroke(box_rect, 2.0, stroke, StrokeKind::Inside);
        }
        Icon::GitBranch => {
            let top_left = Pos2::new(c.x - r * 0.55, c.y - r * 0.75);
            let bot_left = Pos2::new(c.x - r * 0.55, c.y + r * 0.75);
            let top_right = Pos2::new(c.x + r * 0.6, c.y - r * 0.25);
            painter.line_segment([top_left, bot_left], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.55, c.y + r * 0.15), top_right], stroke);
            painter.circle_filled(top_left, 2.2, color);
            painter.circle_filled(bot_left, 2.2, color);
            painter.circle_filled(top_right, 2.2, color);
        }
        Icon::Worktree => {
            let root = Pos2::new(c.x, c.y + r * 0.8);
            let left = Pos2::new(c.x - r * 0.7, c.y - r * 0.6);
            let right = Pos2::new(c.x + r * 0.7, c.y - r * 0.6);
            painter.line_segment([root, left], stroke);
            painter.line_segment([root, right], stroke);
            painter.circle_filled(root, 2.2, color);
            painter.circle_filled(left, 2.2, color);
            painter.circle_filled(right, 2.2, color);
        }
        Icon::Search => {
            let circle_c = Pos2::new(c.x - r * 0.18, c.y - r * 0.18);
            painter.circle_stroke(circle_c, r * 0.68, stroke);
            painter.line_segment(
                [
                    Pos2::new(circle_c.x + r * 0.48, circle_c.y + r * 0.48),
                    Pos2::new(c.x + r * 0.85, c.y + r * 0.85),
                ],
                stroke,
            );
        }
        Icon::Send => {
            let top = Pos2::new(c.x, c.y - r * 0.85);
            let bot = Pos2::new(c.x, c.y + r * 0.85);
            let wing_l = Pos2::new(c.x - r * 0.65, c.y - r * 0.15);
            let wing_r = Pos2::new(c.x + r * 0.65, c.y - r * 0.15);
            let s = Stroke::new(1.8, color);
            painter.line_segment([bot, top], s);
            painter.line_segment([top, wing_l], s);
            painter.line_segment([top, wing_r], s);
        }
        Icon::Stop => {
            let sq = Rect::from_center_size(c, Vec2::splat(r * 1.35));
            painter.rect_filled(sq, 2.0, color);
        }
        Icon::Sliders => {
            let y1 = c.y - r * 0.45;
            let y2 = c.y + r * 0.45;
            painter.line_segment([Pos2::new(c.x - r, y1), Pos2::new(c.x + r, y1)], stroke);
            painter.line_segment([Pos2::new(c.x - r, y2), Pos2::new(c.x + r, y2)], stroke);
            painter.circle_filled(Pos2::new(c.x - r * 0.3, y1), 2.5, color);
            painter.circle_filled(Pos2::new(c.x + r * 0.35, y2), 2.5, color);
        }
        Icon::Key => {
            let head = Pos2::new(c.x - r * 0.35, c.y);
            painter.circle_stroke(head, r * 0.5, stroke);
            painter.line_segment(
                [Pos2::new(head.x + r * 0.5, c.y), Pos2::new(c.x + r * 0.9, c.y)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.55, c.y),
                    Pos2::new(c.x + r * 0.55, c.y + r * 0.4),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.85, c.y),
                    Pos2::new(c.x + r * 0.85, c.y + r * 0.4),
                ],
                stroke,
            );
        }
        Icon::PanelRight => {
            let box_rect = Rect::from_center_size(c, Vec2::new(r * 2.1, r * 1.65));
            painter.rect_stroke(box_rect, 2.0, stroke, StrokeKind::Inside);
            let split_x = box_rect.right() - box_rect.width() * 0.35;
            painter.line_segment(
                [
                    Pos2::new(split_x, box_rect.top()),
                    Pos2::new(split_x, box_rect.bottom()),
                ],
                stroke,
            );
        }
        Icon::Sparkle => {
            painter.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)], stroke);
            let d = r * 0.55;
            painter.line_segment(
                [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                Stroke::new(1.1, color),
            );
            painter.line_segment(
                [Pos2::new(c.x - d, c.y + d), Pos2::new(c.x + d, c.y - d)],
                Stroke::new(1.1, color),
            );
        }
        Icon::Check => {
            let p1 = Pos2::new(c.x - r * 0.75, c.y + r * 0.05);
            let p2 = Pos2::new(c.x - r * 0.15, c.y + r * 0.65);
            let p3 = Pos2::new(c.x + r * 0.85, c.y - r * 0.55);
            let s = Stroke::new(1.8, color);
            painter.line_segment([p1, p2], s);
            painter.line_segment([p2, p3], s);
        }
        Icon::Refresh => {
            painter.circle_stroke(c, r * 0.75, stroke);
            painter.circle_filled(Pos2::new(c.x + r * 0.65, c.y - r * 0.35), 2.2, color);
        }
        Icon::Terminal => {
            let p1 = Pos2::new(c.x - r * 0.75, c.y - r * 0.45);
            let p2 = Pos2::new(c.x - r * 0.15, c.y);
            let p3 = Pos2::new(c.x - r * 0.75, c.y + r * 0.45);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p2, p3], stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.05, c.y + r * 0.45),
                    Pos2::new(c.x + r * 0.75, c.y + r * 0.45),
                ],
                stroke,
            );
        }
        Icon::Close => {
            let d = r * 0.65;
            painter.line_segment([Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)], stroke);
            painter.line_segment([Pos2::new(c.x - d, c.y + d), Pos2::new(c.x + d, c.y - d)], stroke);
        }
    }
}

pub fn launch(smoke: bool) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Serika Code")
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([960.0, 620.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Serika Code",
        options,
        Box::new(move |cc| Ok(Box::new(SerikaApp::new(cc, smoke)))),
    );
    if let Err(err) = result {
        eprintln!("serika-desktop failed to start: {err}");
        std::process::exit(1);
    }
}

enum UiNotice {
    Catalog(Result<ModelCatalog, String>),
    Account(Result<(AccountProfile, UsageSnapshot), String>),
    LoggedIn(Result<LoginOutcome, String>),
    GitRefreshed(GitRepoSnapshot),
    GitActionDone(Result<String, String>),
    DiffLoaded((String, String)),
    WorktreeCreated(Result<PathBuf, String>),
    UpdateChecked(Result<UpdateInfo, String>),
    SetupDone(Result<String, String>),
    TaskFinished(Result<TaskOutcome, String>),
}

#[derive(Debug, Clone)]
struct PendingApproval {
    id: String,
    title: String,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Git,
    Diffs,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerMode {
    Build,
    Plan,
}

struct SerikaApp {
    smoke: bool,
    tokio: tokio::runtime::Runtime,
    core: Option<Arc<Runtime>>,
    startup_error: Option<String>,
    tx: UnboundedSender<UiNotice>,
    rx: UnboundedReceiver<UiNotice>,
    events_rx: Option<UnboundedReceiver<AgentEvent>>,
    pending_approval: Option<PendingApproval>,
    cancel: Option<CancellationToken>,

    // Workspace & Threads
    project: PathBuf,
    recent_projects: Vec<PathBuf>,
    sessions: Vec<SessionSummary>,
    session_filter: String,
    active_session: Option<String>,
    messages: Vec<StoredMessage>,
    tools: Vec<ToolRun>,
    diffs: Vec<(String, String)>,
    selected_diff: usize,
    streaming_reply: String,
    streaming_reasoning: String,

    // Composer state
    prompt: String,
    composer_mode: ComposerMode,
    selected_model: String,
    permission: PermissionMode,
    auto_approve_session: bool,
    status: String,
    running: bool,

    // Providers, Harnesses & Optional APIs
    local_harnesses: Vec<LocalHarness>,
    provider_models: Vec<ChatModel>,
    catalog: ModelCatalog,
    openai_key_input: String,
    anthropic_key_input: String,
    gemini_key_input: String,
    api_base_input: String,
    accounts_base_input: String,

    // Optional Serika Cloud Account
    session: Option<StoredSession>,
    profile: Option<AccountProfile>,
    usage: Option<UsageSnapshot>,
    email_input: String,
    password_input: String,
    two_factor_input: String,
    magic_token_input: String,
    auth_notice: String,

    // Panels & Modals
    show_inspector: bool,
    inspector_tab: InspectorTab,
    show_providers: bool,
    show_settings: bool,
    show_palette: bool,
    palette_query: String,

    // Git & Worktree state
    git: GitRepoSnapshot,
    commit_message: String,
    worktree_branch_input: String,
    last_git_poll: Instant,

    // Updater
    update_info: Option<UpdateInfo>,
    setup_status: String,
}

impl SerikaApp {
    fn new(cc: &eframe::CreationContext<'_>, smoke: bool) -> Self {
        install_system_fonts(&cc.egui_ctx);
        apply_serika_theme(&cc.egui_ctx);

        let tokio = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (tx, rx) = unbounded_channel();
        let (core, startup_error) = match Runtime::open() {
            Ok(runtime) => (Some(Arc::new(runtime)), None),
            Err(err) => (None, Some(err.to_string())),
        };

        let local_harnesses = detect_local_harnesses();
        let provider_models = builtin_provider_models(&local_harnesses);

        let permission = core
            .as_ref()
            .map(|c| c.config.permission)
            .unwrap_or(PermissionMode::Standard);
        let selected_model = core
            .as_ref()
            .and_then(|c| c.config.model.clone())
            .unwrap_or_else(|| {
                if let Some(c) = &core {
                    if c.config.resolve_anthropic_key().is_some() {
                        return "claude/claude-sonnet-4-20250514".to_string();
                    }
                    if c.config.resolve_openai_key().is_some() {
                        return "openai/gpt-4.1".to_string();
                    }
                    if c.config.resolve_gemini_key().is_some() {
                        return "gemini/gemini-2.5-pro".to_string();
                    }
                }
                "local/workspace".to_string()
            });

        let session = core.as_ref().and_then(|c| c.secrets.load().ok().flatten());
        let recent_projects = core
            .as_ref()
            .and_then(|c| c.sessions.recent_projects().ok())
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect::<Vec<_>>();

        let project = choose_initial_workspace(&core, &recent_projects);
        let sessions = core
            .as_ref()
            .and_then(|c| c.sessions.list(Some(&project)).ok())
            .unwrap_or_default();

        let openai_key_input = core
            .as_ref()
            .and_then(|c| c.config.openai_api_key.clone())
            .unwrap_or_default();
        let anthropic_key_input = core
            .as_ref()
            .and_then(|c| c.config.anthropic_api_key.clone())
            .unwrap_or_default();
        let gemini_key_input = core
            .as_ref()
            .and_then(|c| c.config.gemini_api_key.clone())
            .unwrap_or_default();
        let api_base_input = core
            .as_ref()
            .map(|c| c.config.api_base_url.clone())
            .unwrap_or_default();
        let accounts_base_input = core
            .as_ref()
            .map(|c| c.config.accounts_base_url.clone())
            .unwrap_or_default();

        let git = gitinfo::snapshot_sync(&project).unwrap_or_default();
        // Only auto-open the right inspector if there are uncommitted changes in a Git repo
        let show_inspector = git.is_repo && !git.entries.is_empty();

        let mut app = Self {
            smoke,
            tokio,
            core,
            startup_error,
            tx,
            rx,
            events_rx: None,
            pending_approval: None,
            cancel: None,
            project,
            recent_projects,
            sessions,
            session_filter: String::new(),
            active_session: None,
            messages: Vec::new(),
            tools: Vec::new(),
            diffs: Vec::new(),
            selected_diff: 0,
            streaming_reply: String::new(),
            streaming_reasoning: String::new(),
            prompt: String::new(),
            composer_mode: ComposerMode::Build,
            selected_model,
            permission,
            auto_approve_session: false,
            status: "Ready".into(),
            running: false,
            local_harnesses,
            provider_models,
            catalog: ModelCatalog::default(),
            openai_key_input,
            anthropic_key_input,
            gemini_key_input,
            api_base_input,
            accounts_base_input,
            session,
            profile: None,
            usage: None,
            email_input: String::new(),
            password_input: String::new(),
            two_factor_input: String::new(),
            magic_token_input: String::new(),
            auth_notice: String::new(),
            show_inspector,
            inspector_tab: InspectorTab::Git,
            show_providers: false,
            show_settings: false,
            show_palette: false,
            palette_query: String::new(),
            git,
            commit_message: String::new(),
            worktree_branch_input: String::new(),
            last_git_poll: Instant::now(),
            update_info: None,
            setup_status: String::new(),
        };
        app.refresh_remote();
        app
    }

    fn refresh_remote(&mut self) {
        let Some(core) = self.core.clone() else { return };
        if self.session.is_some() {
            let tx = self.tx.clone();
            let core_models = core.clone();
            self.tokio.spawn(async move {
                let _ = tx.send(UiNotice::Catalog(
                    core_models.client.models().await.map_err(|e| e.to_string()),
                ));
            });
        }
        if let Some(session) = self.session.clone() {
            let tx = self.tx.clone();
            let core_acct = core;
            self.tokio.spawn(async move {
                let acct = core_acct.client.account(&session.token).await;
                let usage = core_acct.client.usage(&session.token).await;
                let result = match (acct, usage) {
                    (Ok(a), Ok(u)) => Ok((a, u)),
                    (Ok(a), Err(_)) => Ok((a, UsageSnapshot::default())),
                    (Err(err), _) => Err(err.to_string()),
                };
                let _ = tx.send(UiNotice::Account(result));
            });
        }
    }

    fn refresh_git_async(&mut self) {
        let project = self.project.clone();
        let tx = self.tx.clone();
        self.last_git_poll = Instant::now();
        self.tokio.spawn(async move {
            let snap = gitinfo::snapshot(&project).await.unwrap_or_default();
            let _ = tx.send(UiNotice::GitRefreshed(snap));
        });
    }

    fn check_updates_async(&mut self) {
        let tx = self.tx.clone();
        self.status = "Checking GitHub releases…".into();
        self.tokio.spawn(async move {
            let res = updater::check_for_update().await.map_err(|e| e.to_string());
            let _ = tx.send(UiNotice::UpdateChecked(res));
        });
    }

    fn reload_sessions(&mut self) {
        if let Some(core) = &self.core {
            self.sessions = core.sessions.list(Some(&self.project)).unwrap_or_default();
            if let Ok(recents) = core.sessions.recent_projects() {
                self.recent_projects = recents
                    .into_iter()
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir())
                    .collect();
            }
        }
    }

    fn switch_project(&mut self, next: PathBuf) {
        self.project = next;
        self.active_session = None;
        self.messages.clear();
        self.tools.clear();
        self.diffs.clear();
        self.reload_sessions();
        self.refresh_git_async();
    }

    fn select_session(&mut self, id: &str) {
        let Some(core) = &self.core else { return };
        self.active_session = Some(id.to_string());
        self.messages = core.sessions.messages(id).unwrap_or_default();
        self.tools = core.sessions.tool_runs(id).unwrap_or_default();
        self.diffs = core.sessions.changes(id).unwrap_or_default();
        self.selected_diff = 0;
    }

    fn new_thread(&mut self) {
        self.active_session = None;
        self.messages.clear();
        self.tools.clear();
        self.diffs.clear();
        self.streaming_reply.clear();
        self.streaming_reasoning.clear();
    }

    fn send_prompt(&mut self) {
        if self.running || self.prompt.trim().is_empty() {
            return;
        }
        let Some(core) = self.core.clone() else { return };
        let raw_prompt = self.prompt.trim().to_string();
        let prompt = match self.composer_mode {
            ComposerMode::Build => raw_prompt,
            ComposerMode::Plan => format!(
                "[PLAN MODE] Inspect the relevant files and produce a clear, step-by-step engineering plan before making edits:\n\n{raw_prompt}"
            ),
        };
        self.prompt.clear();
        self.running = true;
        self.streaming_reply.clear();
        self.streaming_reasoning.clear();
        self.status = "Running…".into();

        let (events_tx, events_rx) = unbounded_channel();
        self.events_rx = Some(events_rx);
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());

        let request = TaskRequest {
            project: self.project.clone(),
            prompt,
            session_id: self.active_session.clone(),
            model: if self.selected_model.is_empty() {
                None
            } else {
                Some(self.selected_model.clone())
            },
            permission: Some(if self.composer_mode == ComposerMode::Plan {
                PermissionMode::ReadOnly
            } else {
                self.permission
            }),
            yes: self.auto_approve_session,
            events: events_tx,
            cancel,
        };
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = core.run_task(request).await.map_err(|err| err.to_string());
            let _ = tx.send(UiNotice::TaskFinished(result));
        });
    }

    fn drain_channels(&mut self) {
        while let Ok(notice) = self.rx.try_recv() {
            match notice {
                UiNotice::Catalog(Ok(catalog)) => {
                    self.catalog = catalog;
                }
                UiNotice::Catalog(Err(_)) => {}
                UiNotice::Account(Ok((profile, usage))) => {
                    self.profile = Some(profile);
                    self.usage = Some(usage);
                }
                UiNotice::Account(Err(_)) => {}
                UiNotice::LoggedIn(Ok(LoginOutcome::SignedIn(session))) => {
                    if let Some(core) = &self.core {
                        let _ = core.secrets.save(&session);
                    }
                    self.session = Some(session);
                    self.password_input.clear();
                    self.two_factor_input.clear();
                    self.magic_token_input.clear();
                    self.auth_notice = "Signed in to Serika Cloud.".into();
                    self.refresh_remote();
                }
                UiNotice::LoggedIn(Ok(LoginOutcome::NeedsSecondFactor { message })) => {
                    self.auth_notice = message;
                }
                UiNotice::LoggedIn(Err(err)) => {
                    self.auth_notice = err;
                }
                UiNotice::GitRefreshed(snap) => {
                    self.git = snap;
                }
                UiNotice::GitActionDone(res) => {
                    match res {
                        Ok(msg) => self.status = msg,
                        Err(err) => self.status = format!("Git: {err}"),
                    }
                    self.refresh_git_async();
                }
                UiNotice::DiffLoaded((path, d)) => {
                    self.diffs.retain(|(p, _)| p != &path);
                    self.diffs.push((path, d));
                    self.selected_diff = self.diffs.len().saturating_sub(1);
                    self.inspector_tab = InspectorTab::Diffs;
                }
                UiNotice::WorktreeCreated(Ok(wt)) => {
                    self.status = format!("Switched to worktree {}", wt.display());
                    self.switch_project(wt);
                }
                UiNotice::WorktreeCreated(Err(e)) => {
                    self.status = format!("Worktree: {e}");
                }
                UiNotice::UpdateChecked(Ok(info)) => {
                    self.status = if info.update_available {
                        format!("Update v{} available", info.latest_version)
                    } else {
                        format!("Up to date (v{})", info.current_version)
                    };
                    self.update_info = Some(info);
                }
                UiNotice::UpdateChecked(Err(e)) => {
                    self.status = format!("Update check: {e}");
                }
                UiNotice::SetupDone(Ok(msg)) => {
                    self.setup_status = msg;
                    self.status = "Setup complete".into();
                }
                UiNotice::SetupDone(Err(e)) => {
                    self.setup_status = format!("Setup failed: {e}");
                }
                UiNotice::TaskFinished(result) => {
                    self.running = false;
                    self.cancel = None;
                    self.streaming_reply.clear();
                    self.streaming_reasoning.clear();
                    match result {
                        Ok(outcome) => {
                            self.status = if outcome.finished {
                                "Ready".into()
                            } else {
                                "Stopped at tool round limit".into()
                            };
                            self.reload_sessions();
                            self.select_session(&outcome.session_id);
                            self.refresh_git_async();
                        }
                        Err(err) => {
                            self.status = err;
                            if let Some(id) = self.active_session.clone() {
                                self.select_session(&id);
                            }
                        }
                    }
                }
            }
        }

        let mut incoming = Vec::new();
        if let Some(rx) = &mut self.events_rx {
            while let Ok(event) = rx.try_recv() {
                incoming.push(event);
            }
        }
        for event in incoming {
            match event {
                AgentEvent::Started { session_id } => {
                    self.active_session = Some(session_id.clone());
                    self.reload_sessions();
                    self.select_session(&session_id);
                }
                AgentEvent::Status(text) => self.status = text,
                AgentEvent::ReasoningChunk { text } => self.streaming_reasoning.push_str(&text),
                AgentEvent::MessageChunk { text } => self.streaming_reply.push_str(&text),
                AgentEvent::MessageCompleted { .. } => {
                    self.streaming_reply.clear();
                    self.streaming_reasoning.clear();
                    if let Some(id) = self.active_session.clone() {
                        self.select_session(&id);
                    }
                }
                AgentEvent::ToolRequested { name, .. } => {
                    self.status = format!("Preparing {name}…");
                }
                AgentEvent::ApprovalRequired {
                    id,
                    title,
                    detail,
                    ..
                } => {
                    self.pending_approval = Some(PendingApproval { id, title, detail });
                }
                AgentEvent::ToolStarted { name, .. } => {
                    self.status = format!("Running {name}…");
                }
                AgentEvent::ToolOutput { .. } => {}
                AgentEvent::ToolFinished { .. } => {
                    if let Some(id) = self.active_session.clone() {
                        self.select_session(&id);
                    }
                }
                AgentEvent::FileChanged(diff) => {
                    self.diffs.retain(|(p, _)| p != &diff.path);
                    self.diffs.push((diff.path, diff.diff));
                    self.selected_diff = self.diffs.len().saturating_sub(1);
                }
                AgentEvent::UsageUpdated(u) => self.usage = Some(u),
                AgentEvent::Completed { .. } => self.status = "Ready".into(),
                AgentEvent::Failed { message } => self.status = message,
                AgentEvent::Cancelled => self.status = "Cancelled".into(),
            }
        }
    }

    fn save_providers_and_settings(&mut self) {
        if let Some(core) = &self.core {
            let mut config = core.config.clone();
            config.permission = self.permission;
            config.model = if self.selected_model.is_empty() {
                None
            } else {
                Some(self.selected_model.clone())
            };
            config.openai_api_key = non_empty(&self.openai_key_input);
            config.anthropic_api_key = non_empty(&self.anthropic_key_input);
            config.gemini_api_key = non_empty(&self.gemini_key_input);
            if !self.api_base_input.trim().is_empty() {
                config.api_base_url = self.api_base_input.trim().to_string();
            }
            if !self.accounts_base_input.trim().is_empty() {
                config.accounts_base_url = self.accounts_base_input.trim().to_string();
            }
            match config.save(&core.paths.config_file) {
                Ok(()) => {
                    if let Ok(reopened) = Runtime::open() {
                        self.core = Some(Arc::new(reopened));
                    }
                    self.local_harnesses = detect_local_harnesses();
                    self.provider_models = builtin_provider_models(&self.local_harnesses);
                    self.status = "Saved providers & configuration".into();
                }
                Err(err) => self.status = format!("Save error: {err}"),
            }
        }
    }
}

impl eframe::App for SerikaApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        if self.smoke {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.drain_channels();
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.last_git_poll.elapsed() > Duration::from_secs(8) {
            self.refresh_git_async();
        }

        // Global shortcuts
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, Key::K) {
                self.show_palette = !self.show_palette;
            }
            if i.consume_key(egui::Modifiers::COMMAND, Key::N) {
                self.new_thread();
            }
            if i.consume_key(egui::Modifiers::COMMAND, Key::B) {
                self.show_inspector = !self.show_inspector;
            }
        });

        self.draw_sidebar(root_ui);
        if self.show_inspector {
            self.draw_inspector(root_ui);
        }
        self.draw_main_canvas(root_ui);

        self.draw_approval_modal(&ctx);
        self.draw_providers_modal(&ctx);
        self.draw_settings_modal(&ctx);
        self.draw_palette_modal(&ctx);
    }
}

// =============================================================================
// UI Layout Sections (T3 Code + Serika Brand Guidelines)
// =============================================================================
impl SerikaApp {
    fn draw_sidebar(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::left("t3_sidebar")
            .resizable(true)
            .default_size(248.0)
            .size_range(200.0..=320.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::symmetric(10, 12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(root_ui, |ui| {
                let project_label = workspace_display_name(&self.project, &self.core);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&project_label)
                                    .size(13.0)
                                    .color(TEXT_PRIMARY),
                            )
                            .frame(false),
                        )
                        .on_hover_text(self.project.display().to_string())
                        .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.switch_project(folder);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("+").size(16.0).color(TEXT_MUTED))
                                    .frame(false),
                            )
                            .on_hover_text("New thread")
                            .clicked()
                        {
                            self.new_thread();
                        }
                    });
                });
                if self.git.is_repo && !self.git.branch.is_empty() {
                    ui.label(
                        RichText::new(&self.git.branch)
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                }
                ui.add_space(10.0);
                ui.add(
                    TextEdit::singleline(&mut self.session_filter)
                        .hint_text("Search")
                        .desired_width(f32::INFINITY)
                        .font(FontId::proportional(12.5)),
                );
                ui.add_space(8.0);

                // 4. Thread List
                let q = self.session_filter.to_ascii_lowercase();
                let filtered: Vec<SessionSummary> = self
                    .sessions
                    .iter()
                    .filter(|s| q.is_empty() || s.title.to_ascii_lowercase().contains(&q))
                    .cloned()
                    .collect();

                let footer_reserve = 88.0;
                let list_h = (ui.available_height() - footer_reserve).max(100.0);
                ScrollArea::vertical()
                    .id_salt("t3_thread_scroll")
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new(
                                    "No threads yet. Start typing in the composer to begin.",
                                )
                                .size(11.5)
                                .color(TEXT_DIM),
                            );
                        }
                        let mut to_select = None;
                        let mut to_delete = None;
                        for item in &filtered {
                            let is_active =
                                self.active_session.as_deref() == Some(item.id.as_str());
                            let row_resp = draw_thread_row(ui, item, is_active);
                            if row_resp.0 {
                                to_select = Some(item.id.clone());
                            }
                            if row_resp.1 {
                                to_delete = Some(item.id.clone());
                            }
                        }
                        if let Some(id) = to_select {
                            self.select_session(&id);
                        }
                        if let Some(id) = to_delete {
                            if let Some(core) = &self.core {
                                let _ = core.sessions.delete(&id);
                                if self.active_session.as_deref() == Some(id.as_str()) {
                                    self.new_thread();
                                }
                                self.reload_sessions();
                            }
                        }

                        if self.recent_projects.len() > 1 {
                            ui.add_space(16.0);
                            ui.label(RichText::new("Recent").size(11.0).color(TEXT_DIM));
                            ui.add_space(2.0);
                            let mut switch_to = None;
                            for rp in &self.recent_projects {
                                if rp == &self.project {
                                    continue;
                                }
                                let name = rp
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("project");
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(name).size(11.5).color(TEXT_MUTED),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .frame(false),
                                    )
                                    .on_hover_text(rp.display().to_string())
                                    .clicked()
                                {
                                    switch_to = Some(rp.clone());
                                }
                            }
                            if let Some(p) = switch_to {
                                self.switch_project(p);
                            }
                        }
                    });

                // 5. Sidebar Footer (Provider Pill + Quick Actions)
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        if ui
                            .add(egui::Button::new(RichText::new("Account").size(12.0).color(TEXT_MUTED)).frame(false))
                            .clicked()
                        {
                            self.show_providers = !self.show_providers;
                        }
                        if ui
                            .add(egui::Button::new(RichText::new("Settings").size(12.0).color(TEXT_MUTED)).frame(false))
                            .clicked()
                        {
                            self.show_settings = !self.show_settings;
                        }
                    });
                    ui.add_space(6.0);
                    let active_provider = ProviderKind::from_model_id(&self.selected_model);
                    let provider_ready = match active_provider {
                        ProviderKind::Local => true,
                        ProviderKind::OpenAi => self
                            .core
                            .as_ref()
                            .and_then(|c| c.config.resolve_openai_key())
                            .is_some(),
                        ProviderKind::Anthropic => self
                            .core
                            .as_ref()
                            .and_then(|c| c.config.resolve_anthropic_key())
                            .is_some(),
                        ProviderKind::Gemini => self
                            .core
                            .as_ref()
                            .and_then(|c| c.config.resolve_gemini_key())
                            .is_some(),
                        ProviderKind::Serika => self.session.is_some(),
                    };
                    let who = if provider_ready {
                        active_provider.label().to_string()
                    } else {
                        format!("{} · sign in", active_provider.label())
                    };
                    ui.label(RichText::new(who).size(11.5).color(TEXT_DIM));
                });
            });
    }

    fn draw_main_canvas(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG_DEEP))
            .show(root_ui, |ui| {
                let is_draft = self.messages.is_empty() && !self.running;
                if !is_draft {
                    self.draw_top_header(ui);
                }

                if let Some(err) = &self.startup_error {
                    ui.add_space(12.0);
                    ui.colored_label(ROSE, format!("Runtime error: {err}"));
                    return;
                }

                if is_draft {
                    self.draw_t3_draft_hero(ui);
                } else {
                    self.draw_active_thread_canvas(ui);
                }
            });
    }

    fn draw_top_header(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .fill(BG_DEEP)
            .inner_margin(Margin::symmetric(28, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let proj_name = workspace_display_name(&self.project, &self.core);
                    ui.label(RichText::new(proj_name).size(12.0).color(TEXT_DIM));
                    ui.label(RichText::new("/").size(12.0).color(TEXT_DIM));
                    let thread_title = self
                        .active_session
                        .as_ref()
                        .and_then(|id| self.sessions.iter().find(|s| &s.id == id))
                        .map(|s| s.title.as_str())
                        .unwrap_or("New thread");
                    ui.label(RichText::new(thread_title).size(12.0).color(TEXT_PRIMARY));
                    if self.git.is_repo && !self.git.entries.is_empty() {
                        ui.label(
                            RichText::new(format!("{} changed", self.git.entries.len()))
                                .size(12.0)
                                .color(TEXT_DIM),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(if self.show_inspector { "Hide" } else { "Changes" })
                                        .size(12.0)
                                        .color(TEXT_MUTED),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.show_inspector = !self.show_inspector;
                        }
                    });
                });
            });
    }

    fn draw_t3_draft_hero(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let top_pad = ((avail.y - 180.0) * 0.42).max(24.0);
        ui.add_space(top_pad);
        ui.vertical_centered(|ui| {
            let lane_w = (ui.available_width() - 48.0).clamp(420.0, 720.0);
            ui.allocate_ui_with_layout(
                Vec2::new(lane_w, 10.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    let name = workspace_display_name(&self.project, &self.core);
                    ui.label(RichText::new(name).size(13.0).color(TEXT_DIM));
                    ui.add_space(10.0);
                    self.draw_composer_card(ui, true);
                },
            );
        });
    }

    #[allow(dead_code)]
    fn draw_t3_context_strip(&mut self, ui: &mut egui::Ui) {
        let proj_name = workspace_display_name(&self.project, &self.core);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (fr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                paint_icon(ui.painter(), fr, Icon::Folder, TEXT_DIM);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("Project: {proj_name}"))
                                .size(11.0)
                                .color(TEXT_MUTED),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Click to select a project directory")
                    .clicked()
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.switch_project(folder);
                    }
                }
            });

            ui.label(RichText::new("·").size(11.0).color(TEXT_DIM));

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (gr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                paint_icon(ui.painter(), gr, Icon::GitBranch, TEXT_DIM);
                let git_txt = if self.git.is_repo {
                    format!("Branch: {}", self.git.branch)
                } else {
                    "No Git repo (Local folder)".to_string()
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(git_txt).size(11.0).color(TEXT_MUTED))
                            .frame(false),
                    )
                    .clicked()
                {
                    self.show_inspector = true;
                    self.inspector_tab = InspectorTab::Git;
                }
            });

            if self.git.is_repo {
                ui.label(RichText::new("·").size(11.0).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (wr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    paint_icon(ui.painter(), wr, Icon::Worktree, TEXT_DIM);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Create isolated worktree")
                                    .size(11.0)
                                    .color(SERIKA_VIOLET_LIGHT),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Create an isolated git worktree for parallel agent work")
                        .clicked()
                    {
                        self.show_inspector = true;
                        self.inspector_tab = InspectorTab::Git;
                    }
                });
            }
        });
    }

    fn draw_active_thread_canvas(&mut self, ui: &mut egui::Ui) {
        let avail_h = ui.available_height();
        let composer_reserve = 148.0;
        let scroll_h = (avail_h - composer_reserve).max(120.0);

        ScrollArea::vertical()
            .id_salt("t3_active_thread_scroll")
            .max_height(scroll_h)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    let lane_w = ui.available_width().min(720.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(lane_w, 10.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            for msg in &self.messages {
                                if msg.content.starts_with("Tool results:\n") {
                                    continue;
                                }
                                draw_message_card(ui, msg);
                                ui.add_space(10.0);
                            }

                            if !self.tools.is_empty() {
                                ui.add_space(2.0);
                                let mut open_diff_idx = None;
                                for tool in self.tools.iter().rev().take(6).rev() {
                                    let diff_idx =
                                        self.diffs.iter().position(|(_, d)| !d.is_empty());
                                    if draw_tool_pill(ui, tool, diff_idx.is_some()) {
                                        open_diff_idx = diff_idx;
                                    }
                                    ui.add_space(4.0);
                                }
                                if let Some(idx) = open_diff_idx {
                                    self.selected_diff = idx;
                                    self.show_inspector = true;
                                    self.inspector_tab = InspectorTab::Diffs;
                                }
                                ui.add_space(6.0);
                            }

                            if !self.streaming_reasoning.is_empty() {
                                ui.label(RichText::new("Thinking").size(12.0).color(TEXT_DIM));
                                ui.label(
                                    RichText::new(&self.streaming_reasoning)
                                        .size(13.0)
                                        .color(TEXT_MUTED),
                                );
                                ui.add_space(12.0);
                            }

                            if !self.streaming_reply.is_empty() {
                                ui.label(
                                    RichText::new(&self.streaming_reply)
                                        .size(14.5)
                                        .color(TEXT_PRIMARY),
                                );
                            }
                        },
                    );
                });
                ui.add_space(12.0);
            });

        // Bottom Docked Composer Lane
        ui.vertical_centered(|ui| {
            let lane_w = ui.available_width().min(720.0);
            ui.allocate_ui_with_layout(
                Vec2::new(lane_w, 10.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    self.draw_composer_card(ui, false);
                },
            );
        });
    }

    fn draw_composer_card(&mut self, ui: &mut egui::Ui, is_hero: bool) {
        let border_col = if self.running {
            SERIKA_VIOLET
        } else if is_hero {
            BORDER_STRONG
        } else {
            BORDER_SUBTLE
        };

        egui::Frame::NONE
            .fill(BG_CARD)
            .corner_radius(CornerRadius::same(12))
            .stroke(Stroke::new(1.0, border_col))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                let rows = if is_hero { 3 } else { 2 };
                let resp = ui.add(
                    TextEdit::multiline(&mut self.prompt)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .font(FontId::proportional(15.0))
                        .hint_text("Ask Serika to change this project"),
                );
                if resp.has_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.command)
                {
                    self.send_prompt();
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // T3 Composer Footer Controls
                ui.horizontal(|ui| {
                    // 1. Provider & Model Selector
                    let current_kind = ProviderKind::from_model_id(&self.selected_model);
                    let display_model = self
                        .provider_models
                        .iter()
                        .chain(self.catalog.all())
                        .find(|m| m.id == self.selected_model)
                        .map(|m| format!("{} · {}", current_kind.badge(), m.name))
                        .unwrap_or_else(|| {
                            format!("{} · {}", current_kind.badge(), self.selected_model)
                        });

                    egui::ComboBox::from_id_salt(if is_hero {
                        "hero_model_combo"
                    } else {
                        "dock_model_combo"
                    })
                    .width(220.0)
                    .selected_text(RichText::new(display_model).size(11.5).color(TEXT_PRIMARY))
                    .show_ui(ui, |ui| {
                        ui.label(RichText::new("On this machine").size(11.0).color(TEXT_DIM));
                        for m in self
                            .provider_models
                            .iter()
                            .filter(|m| m.provider.as_deref() == Some("local"))
                        {
                            ui.selectable_value(
                                &mut self.selected_model,
                                m.id.clone(),
                                format!("{} (No API)", m.name),
                            );
                        }
                        ui.separator();
                        ui.label(RichText::new("Your API key").size(11.0).color(TEXT_DIM));
                        for m in self
                            .provider_models
                            .iter()
                            .filter(|m| m.provider.as_deref() != Some("local"))
                        {
                            let p = m.provider.as_deref().unwrap_or("api").to_uppercase();
                            ui.selectable_value(
                                &mut self.selected_model,
                                m.id.clone(),
                                format!("{p} · {}", m.name),
                            );
                        }
                        if self.catalog.all().next().is_some() {
                            ui.separator();
                            ui.label(RichText::new("Serika").size(11.0).color(TEXT_DIM));
                            for m in self.catalog.all() {
                                ui.selectable_value(
                                    &mut self.selected_model,
                                    m.id.clone(),
                                    format!("SERIKA · {}", m.name),
                                );
                            }
                        }
                    });

                    // 2. Build / Plan Mode Toggle Pill (like T3 Code)
                    let mode_label = match self.composer_mode {
                        ComposerMode::Build => "Build",
                        ComposerMode::Plan => "Plan",
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(mode_label).size(12.0).color(TEXT_MUTED))
                                .frame(false),
                        )
                        .on_hover_text("Toggle between Build (execute edits) and Plan (read-only architecture plan)")
                        .clicked()
                    {
                        self.composer_mode = match self.composer_mode {
                            ComposerMode::Build => ComposerMode::Plan,
                            ComposerMode::Plan => ComposerMode::Build,
                        };
                    }

                    // 3. Permission Selector Pill
                    egui::ComboBox::from_id_salt(if is_hero {
                        "hero_perm_combo"
                    } else {
                        "dock_perm_combo"
                    })
                    .width(102.0)
                    .selected_text(
                        RichText::new(self.permission.as_str())
                            .size(11.5)
                            .color(TEXT_MUTED),
                    )
                    .show_ui(ui, |ui| {
                        for mode in [
                            PermissionMode::ReadOnly,
                            PermissionMode::Standard,
                            PermissionMode::Restricted,
                            PermissionMode::Full,
                        ] {
                            ui.selectable_value(&mut self.permission, mode, mode.as_str());
                        }
                    });

                    // 4. Right-aligned Send / Stop Button
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.running {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Stop").size(12.5).color(TEXT_PRIMARY))
                                        .fill(BG_INPUT)
                                        .corner_radius(CornerRadius::same(8)),
                                )
                                .clicked()
                            {
                                if let Some(cancel) = &self.cancel {
                                    cancel.cancel();
                                }
                            }
                        } else {
                            let can_send = !self.prompt.trim().is_empty();
                            let fill = if can_send { TEXT_PRIMARY } else { BG_INPUT };
                            let ink = if can_send { BG_DEEP } else { TEXT_DIM };
                            if ui
                                .add_sized(
                                    [28.0, 28.0],
                                    egui::Button::new(RichText::new("↑").size(16.0).color(ink))
                                        .fill(fill)
                                        .corner_radius(CornerRadius::same(8)),
                                )
                                .clicked()
                                && can_send
                            {
                                self.send_prompt();
                            }
                        }
                    });
                });
            });
    }

    fn draw_inspector(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::right("t3_inspector")
            .resizable(true)
            .default_size(340.0)
            .size_range(280.0..=520.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (InspectorTab::Git, "Source Control"),
                        (InspectorTab::Diffs, "Diffs"),
                        (InspectorTab::Activity, "Activity"),
                    ] {
                        let active = self.inspector_tab == tab;
                        let col = if active { TEXT_PRIMARY } else { TEXT_DIM };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(label).size(12.5).color(col))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.inspector_tab = tab;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Close Inspector (Ctrl+B)", false).clicked()
                        {
                            self.show_inspector = false;
                        }
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                match self.inspector_tab {
                    InspectorTab::Git => self.draw_git_tab(ui),
                    InspectorTab::Diffs => self.draw_diffs_tab(ui),
                    InspectorTab::Activity => self.draw_activity_tab(ui),
                }
            });
    }

    fn draw_git_tab(&mut self, ui: &mut egui::Ui) {
        if !self.git.is_repo {
            ui.label(
                RichText::new("Current folder is not a Git repository.")
                    .size(12.5)
                    .color(TEXT_MUTED),
            );
            ui.add_space(6.0);
            if pill_icon_button(ui, Icon::Refresh, "Refresh", false).clicked() {
                self.refresh_git_async();
            }
            return;
        }

        ui.horizontal(|ui| {
            let (gr, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
            paint_icon(ui.painter(), gr, Icon::GitBranch, SERIKA_VIOLET_LIGHT);
            egui::ComboBox::from_id_salt("git_branch_combo")
                .width(150.0)
                .selected_text(
                    RichText::new(&self.git.branch)
                        .size(12.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                )
                .show_ui(ui, |ui| {
                    let mut switch = None;
                    for b in &self.git.branches {
                        if ui.selectable_label(b == &self.git.branch, b).clicked() {
                            switch = Some(b.clone());
                        }
                    }
                    if let Some(target) = switch {
                        let proj = self.project.clone();
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let res = gitinfo::checkout_branch(&proj, &target, false)
                                .await
                                .map(|_| format!("Switched to {target}"))
                                .map_err(|e| e.to_string());
                            let _ = tx.send(UiNotice::GitActionDone(res));
                        });
                    }
                });

            if icon_button(ui, Icon::Refresh, "Refresh Git", false).clicked() {
                self.refresh_git_async();
            }
        });

        ui.add_space(8.0);

        // Commit Box
        egui::Frame::NONE
            .fill(BG_CARD)
            .corner_radius(CornerRadius::same(8))
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.add(
                    TextEdit::singleline(&mut self.commit_message)
                        .hint_text("Commit message…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Stage All").size(11.0).color(TEXT_PRIMARY),
                            )
                            .fill(BG_INPUT)
                            .corner_radius(CornerRadius::same(6)),
                        )
                        .clicked()
                    {
                        let proj = self.project.clone();
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let res = gitinfo::stage_all(&proj)
                                .await
                                .map(|_| "Staged all changes".into())
                                .map_err(|e| e.to_string());
                            let _ = tx.send(UiNotice::GitActionDone(res));
                        });
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Commit")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(SERIKA_VIOLET)
                            .corner_radius(CornerRadius::same(6)),
                        )
                        .clicked()
                        && !self.commit_message.trim().is_empty()
                    {
                        let proj = self.project.clone();
                        let msg = self.commit_message.trim().to_string();
                        self.commit_message.clear();
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let res = gitinfo::commit_all(&proj, &msg)
                                .await
                                .map_err(|e| e.to_string());
                            let _ = tx.send(UiNotice::GitActionDone(res));
                        });
                    }
                });
            });

        ui.add_space(10.0);
        ui.label(
            RichText::new(format!("WORKING TREE ({})", self.git.entries.len()))
                .size(10.5)
                .strong()
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);

        ScrollArea::vertical()
            .id_salt("git_entries_scroll")
            .max_height(240.0)
            .show(ui, |ui| {
                if self.git.entries.is_empty() {
                    ui.label(RichText::new("Working tree is clean.").size(11.5).color(EMERALD));
                }
                let entries = self.git.entries.clone();
                for entry in entries {
                    ui.horizontal(|ui| {
                        let col = if entry.staged { EMERALD } else { AMBER };
                        ui.label(
                            RichText::new(entry.badge())
                                .monospace()
                                .size(11.0)
                                .strong()
                                .color(col),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(&entry.path).size(11.5).color(TEXT_PRIMARY),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            let proj = self.project.clone();
                            let path = entry.path.clone();
                            let staged = entry.staged;
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                if let Ok(d) = gitinfo::file_diff(&proj, &path, staged).await {
                                    let _ = tx.send(UiNotice::DiffLoaded((path, d)));
                                }
                            });
                        }
                    });
                }
            });

        ui.add_space(10.0);
        ui.collapsing(
            RichText::new(format!("Worktrees ({})", self.git.worktrees.len()))
                .size(11.5)
                .color(TEXT_MUTED),
            |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.worktree_branch_input)
                            .hint_text("feat/agent-branch")
                            .desired_width(150.0),
                    );
                    if ui.button("Isolate").clicked()
                        && !self.worktree_branch_input.trim().is_empty()
                    {
                        if let Some(core) = &self.core {
                            let base = core.paths.data_dir.join("worktrees");
                            let branch = self.worktree_branch_input.trim().to_string();
                            self.worktree_branch_input.clear();
                            let proj = self.project.clone();
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                let res = gitinfo::create_worktree(&proj, &base, &branch)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(UiNotice::WorktreeCreated(res));
                            });
                        }
                    }
                });
            },
        );
    }

    fn draw_diffs_tab(&mut self, ui: &mut egui::Ui) {
        if self.diffs.is_empty() {
            ui.label(
                RichText::new("No diffs recorded in this thread yet.")
                    .size(12.0)
                    .color(TEXT_DIM),
            );
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (idx, (path, _)) in self.diffs.iter().enumerate() {
                let active = idx == self.selected_diff;
                let fill = if active { SERIKA_VIOLET } else { BG_CARD };
                if ui
                    .add(
                        egui::Button::new(RichText::new(path).size(11.0).color(TEXT_PRIMARY))
                            .fill(fill)
                            .corner_radius(CornerRadius::same(6)),
                    )
                    .clicked()
                {
                    self.selected_diff = idx;
                }
            }
        });
        ui.separator();
        if let Some((_, diff)) = self.diffs.get(self.selected_diff) {
            ScrollArea::vertical()
                .id_salt("inspector_diff_scroll")
                .show(ui, |ui| {
                    for line in diff.lines() {
                        let color = if line.starts_with('+') {
                            EMERALD
                        } else if line.starts_with('-') {
                            ROSE
                        } else if line.starts_with("@@") {
                            SERIKA_VIOLET_LIGHT
                        } else {
                            TEXT_MUTED
                        };
                        ui.label(RichText::new(line).monospace().size(11.5).color(color));
                    }
                });
        }
    }

    fn draw_activity_tab(&mut self, ui: &mut egui::Ui) {
        if self.tools.is_empty() {
            ui.label(
                RichText::new("No tool executions in this thread yet.")
                    .size(12.0)
                    .color(TEXT_DIM),
            );
            return;
        }
        ScrollArea::vertical()
            .id_salt("inspector_activity_scroll")
            .show(ui, |ui| {
                for tool in self.tools.iter().rev() {
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .corner_radius(CornerRadius::same(8))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(Margin::same(8))
                        .show(ui, |ui| {
                            let c = if tool.ok { EMERALD } else { ROSE };
                            ui.label(
                                RichText::new(&tool.name)
                                    .monospace()
                                    .size(11.5)
                                    .strong()
                                    .color(c),
                            );
                            ui.label(
                                RichText::new(&tool.output)
                                    .monospace()
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                            );
                        });
                    ui.add_space(6.0);
                }
            });
    }

    // =========================================================================
    // Modals: Providers & API Keys, Settings, Command Palette, Approvals
    // =========================================================================
    fn draw_providers_modal(&mut self, ctx: &egui::Context) {
        if !self.show_providers {
            return;
        }
        let mut open = self.show_providers;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new("Providers & Optional API Keys")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("1. Local Harnesses (Default · Zero API Key Required)")
                        .strong()
                        .size(13.0)
                        .color(EMERALD),
                );
                ui.label(
                    RichText::new(
                        "Just like T3 Code, Serika Code works out-of-the-box without requiring any API key.",
                    )
                    .size(11.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(6.0);
                for h in &self.local_harnesses {
                    ui.horizontal(|ui| {
                        let col = if h.available { EMERALD } else { TEXT_DIM };
                        let status = if h.available { "READY" } else { "NOT FOUND" };
                        ui.label(RichText::new(status).monospace().size(10.5).strong().color(col));
                        if ui
                            .selectable_label(self.selected_model == h.id, &h.name)
                            .clicked()
                            && h.available
                        {
                            self.selected_model = h.id.clone();
                        }
                        ui.label(RichText::new(&h.description).size(11.0).color(TEXT_DIM));
                    });
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("2. Optional BYOK APIs (Claude, OpenAI & Gemini)")
                        .strong()
                        .size(13.0)
                        .color(SERIKA_VIOLET_LIGHT),
                );
                ui.label(
                    RichText::new(
                        "Optionally add your own API keys to use Claude, OpenAI, or Gemini directly.",
                    )
                    .size(11.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Anthropic Claude API Key (optional)")
                        .size(11.5)
                        .color(TEXT_MUTED),
                );
                ui.add(
                    TextEdit::singleline(&mut self.anthropic_key_input)
                        .password(true)
                        .hint_text("sk-ant-… (or set ANTHROPIC_API_KEY)")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new("OpenAI API Key (optional)")
                        .size(11.5)
                        .color(TEXT_MUTED),
                );
                ui.add(
                    TextEdit::singleline(&mut self.openai_key_input)
                        .password(true)
                        .hint_text("sk-… (or set OPENAI_API_KEY)")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new("Google Gemini API Key (optional)")
                        .size(11.5)
                        .color(TEXT_MUTED),
                );
                ui.add(
                    TextEdit::singleline(&mut self.gemini_key_input)
                        .password(true)
                        .hint_text("AIza… (or set GEMINI_API_KEY)")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Save Providers & Keys")
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(SERIKA_VIOLET)
                            .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        self.save_providers_and_settings();
                    }
                });

                ui.add_space(10.0);
                ui.collapsing(
                    RichText::new("Optional: Serika Cloud Account")
                        .size(12.0)
                        .color(TEXT_MUTED),
                    |ui| {
                        if let Some(session) = self.session.clone() {
                            ui.label(format!(
                                "Signed in as {} ({})",
                                session.username, session.email
                            ));
                            if ui.button("Sign out").clicked() {
                                if let Some(core) = &self.core {
                                    let _ = core.secrets.clear();
                                }
                                self.session = None;
                            }
                        } else {
                            ui.add(
                                TextEdit::singleline(&mut self.email_input)
                                    .hint_text("Email")
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add(
                                TextEdit::singleline(&mut self.password_input)
                                    .password(true)
                                    .hint_text("Password")
                                    .desired_width(f32::INFINITY),
                            );
                            if ui.button("Sign in to Serika Cloud").clicked() {
                                if let Some(core) = self.core.clone() {
                                    let email = self.email_input.trim().to_string();
                                    let password = self.password_input.clone();
                                    let tx = self.tx.clone();
                                    self.tokio.spawn(async move {
                                        let res = core
                                            .client
                                            .login_password(&email, &password, None)
                                            .await
                                            .map_err(|e| e.to_string());
                                        let _ = tx.send(UiNotice::LoggedIn(res));
                                    });
                                }
                            }
                            if !self.auth_notice.is_empty() {
                                ui.label(
                                    RichText::new(&self.auth_notice)
                                        .size(11.0)
                                        .color(SERIKA_VIOLET_LIGHT),
                                );
                            }
                        }
                    },
                );
            });
        self.show_providers = open;
    }

    fn draw_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new("Settings & System Setup")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.checkbox(
                    &mut self.auto_approve_session,
                    "Auto-approve file edits and shell commands in this session",
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Re-run System Setup & Shortcuts")
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(SERIKA_VIOLET)
                            .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        if let Some(core) = self.core.clone() {
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                let res = tokio::task::spawn_blocking(move || {
                                    setup::install_system(&core.paths, &SetupOptions::default())
                                        .map(|r| r.render())
                                        .map_err(|e| e.to_string())
                                })
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()));
                                let _ = tx.send(UiNotice::SetupDone(res));
                            });
                        }
                    }
                    if ui.button("Check for Updates").clicked() {
                        self.check_updates_async();
                    }
                });
                if !self.setup_status.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(&self.setup_status)
                            .monospace()
                            .size(11.0)
                            .color(TEXT_MUTED),
                    );
                }
            });
        self.show_settings = open;
    }

    fn draw_palette_modal(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        let mut open = self.show_palette;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new("Command Palette")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                let resp = ui.add(
                    TextEdit::singleline(&mut self.palette_query)
                        .hint_text("Type a command…")
                        .desired_width(f32::INFINITY),
                );
                resp.request_focus();
                ui.add_space(6.0);
                let q = self.palette_query.to_ascii_lowercase();
                for (id, label) in [
                    ("new", "New Thread (Ctrl+N)"),
                    ("folder", "Open Project Folder…"),
                    ("scratch", "Switch to Scratch Workspace"),
                    (
                        "providers",
                        "Configure Providers & Optional API Keys (OpenAI / Gemini / Claude)",
                    ),
                    ("inspector", "Toggle Git & Diffs Inspector (Ctrl+B)"),
                    ("update", "Check for Updates"),
                ] {
                    if !q.is_empty() && !label.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new(RichText::new(label).size(12.0).color(TEXT_PRIMARY))
                                .fill(BG_INPUT)
                                .corner_radius(CornerRadius::same(6)),
                        )
                        .clicked()
                    {
                        self.show_palette = false;
                        self.palette_query.clear();
                        match id {
                            "new" => self.new_thread(),
                            "folder" => {
                                if let Some(f) = rfd::FileDialog::new().pick_folder() {
                                    self.switch_project(f);
                                }
                            }
                            "scratch" => {
                                if let Some(core) = &self.core {
                                    let s = core.paths.scratch_dir();
                                    let _ = std::fs::create_dir_all(&s);
                                    self.switch_project(s);
                                }
                            }
                            "providers" => self.show_providers = true,
                            "inspector" => self.show_inspector = !self.show_inspector,
                            "update" => self.check_updates_async(),
                            _ => {}
                        }
                    }
                }
            });
        if !open {
            self.show_palette = false;
        }
    }

    fn draw_approval_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_approval.clone() else { return };
        let mut choice = None;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new("Approve Tool Execution")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.5, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(&pending.title)
                        .strong()
                        .size(14.0)
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&pending.detail)
                        .monospace()
                        .size(12.0)
                        .color(TEXT_MUTED),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Approve")
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(EMERALD)
                            .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        choice = Some(true);
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Deny").size(12.0).strong().color(Color32::WHITE),
                            )
                            .fill(ROSE)
                            .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        choice = Some(false);
                    }
                });
            });
        if let Some(approved) = choice {
            if let Some(req) = self.pending_approval.take() {
                if let Some(core) = &self.core {
                    core.approvals.resolve(&req.id, approved);
                }
            }
        }
    }
}

// =============================================================================
// Helper Components & Theme Setup
// =============================================================================

/// Prevent starting in `C:\Users\...\AppData\Local\Programs\Serika Code\bin` when launched from a shortcut.
fn choose_initial_workspace(core: &Option<Arc<Runtime>>, recents: &[PathBuf]) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd_str = cwd.to_string_lossy().to_ascii_lowercase();
    let is_install_or_system_dir = cwd_str.contains("programs\\serika code")
        || cwd_str.contains("appdata\\local")
        || cwd_str.ends_with("\\bin")
        || cwd_str.contains("windows\\system32");

    if !is_install_or_system_dir && cwd.join(".git").exists() {
        return cwd;
    }
    if let Some(first_recent) = recents.first() {
        return first_recent.clone();
    }
    if !is_install_or_system_dir {
        return cwd;
    }
    if let Some(c) = core {
        let scratch = c.paths.scratch_dir();
        let _ = std::fs::create_dir_all(&scratch);
        return scratch;
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or(cwd)
}

fn workspace_display_name(project: &Path, core: &Option<Arc<Runtime>>) -> String {
    if let Some(c) = core {
        if project == c.paths.scratch_dir() {
            return "Scratch".to_string();
        }
    }
    project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Workspace")
        .to_string()
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, primary: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    let fill = if primary {
        if resp.hovered() {
            SERIKA_VIOLET_HOVER
        } else {
            SERIKA_VIOLET
        }
    } else if resp.hovered() {
        BG_CARD_HOVER
    } else {
        BG_CARD
    };
    let fg = if primary {
        Color32::WHITE
    } else if resp.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_MUTED
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if !primary {
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.0, BORDER_SUBTLE), StrokeKind::Inside);
    }
    paint_icon(ui.painter(), rect.shrink(5.0), icon, fg);
    resp.on_hover_text(tooltip)
}

fn pill_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    active: bool,
) -> egui::Response {
    let fill = if active { SERIKA_VIOLET_SOFT } else { BG_CARD };
    let stroke_col = if active { SERIKA_VIOLET } else { BORDER_SUBTLE };
    let fg = if active {
        SERIKA_VIOLET_LIGHT
    } else {
        TEXT_MUTED
    };
    let btn = ui.add(
        egui::Button::new(RichText::new(format!("    {label}")).size(11.0).color(fg))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke_col))
            .corner_radius(CornerRadius::same(6)),
    );
    let ir = Rect::from_center_size(
        Pos2::new(btn.rect.left() + 11.0, btn.rect.center().y),
        Vec2::splat(11.0),
    );
    paint_icon(ui.painter(), ir, icon, fg);
    btn
}

fn starter_chip(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    let btn = ui.add(
        egui::Button::new(
            RichText::new(format!("     {label}"))
                .size(11.5)
                .color(TEXT_MUTED),
        )
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(20)),
    );
    let ir = Rect::from_center_size(
        Pos2::new(btn.rect.left() + 13.0, btn.rect.center().y),
        Vec2::splat(11.0),
    );
    paint_icon(ui.painter(), ir, icon, SERIKA_VIOLET_LIGHT);
    btn
}

fn draw_thread_row(ui: &mut egui::Ui, item: &SessionSummary, is_active: bool) -> (bool, bool) {
    let mut clicked = false;
    let mut deleted = false;
    let desired = Vec2::new(ui.available_width(), 34.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    let fill = if is_active {
        BG_ACTIVE_ROW
    } else if resp.hovered() {
        BG_CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);

    let title_col = if is_active || resp.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_MUTED
    };
    let text_pos = Pos2::new(rect.left() + 8.0, rect.center().y - 7.0);
    let clipped_title: String = item.title.chars().take(26).collect();
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_TOP,
        clipped_title,
        FontId::proportional(13.0),
        title_col,
    );
    if !resp.hovered() {
        ui.painter().text(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            short_age(item.updated_at),
            FontId::proportional(11.0),
            TEXT_DIM,
        );
    }

    if resp.hovered() {
        let del_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 14.0, rect.center().y),
            Vec2::splat(16.0),
        );
        let del_resp = ui.interact(del_rect, ui.scope_id().with(&item.id), Sense::click());
        paint_icon(
            ui.painter(),
            del_rect.shrink(3.5),
            Icon::Close,
            if del_resp.hovered() { ROSE } else { TEXT_DIM },
        );
        if del_resp.clicked() {
            deleted = true;
        }
    }

    if resp.clicked() && !deleted {
        clicked = true;
    }
    (clicked, deleted)
}

fn draw_message_card(ui: &mut egui::Ui, msg: &StoredMessage) {
    let is_user = msg.role == "user";
    ui.add_space(4.0);
    if is_user {
        ui.label(RichText::new("You").size(12.0).color(TEXT_DIM));
        ui.add_space(2.0);
        ui.label(RichText::new(&msg.content).size(15.0).color(TEXT_PRIMARY));
    } else {
        ui.label(RichText::new(&msg.content).size(14.5).color(TEXT_PRIMARY));
    }
    ui.add_space(10.0);
}

fn draw_tool_pill(ui: &mut egui::Ui, tool: &ToolRun, has_diff: bool) -> bool {
    let mut view_diff = false;
    ui.horizontal(|ui| {
        let mark = if tool.ok { "ok" } else { "failed" };
        let col = if tool.ok { TEXT_DIM } else { ROSE };
        ui.label(
            RichText::new(format!("{}  {mark}", tool.name))
                .monospace()
                .size(12.0)
                .color(col),
        );
        if has_diff && (tool.name.contains("file") || tool.name.contains("patch")) && ui
            .add(egui::Button::new(RichText::new("diff").size(12.0).color(TEXT_MUTED)).frame(false))
            .clicked()
        {
            view_diff = true;
        }
    });
    view_diff
}

fn short_age(updated_at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(updated_at);
    let delta = (now - updated_at).max(0);
    if delta < 60 {
        "now".into()
    } else if delta < 3600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3600)
    } else {
        format!("{}d", delta / 86_400)
    }
}

fn install_system_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let ui_candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in ui_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("system_ui".into(), FontData::from_owned(bytes).into());
            if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
                fam.insert(0, "system_ui".into());
            }
            break;
        }
    }

    let mono_candidates = [
        r"C:\Windows\Fonts\CascadiaCode.ttf",
        r"C:\Windows\Fonts\consola.ttf",
        "/System/Library/Fonts/SFNSMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];
    for path in mono_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("system_mono".into(), FontData::from_owned(bytes).into());
            if let Some(fam) = fonts.families.get_mut(&FontFamily::Monospace) {
                fam.insert(0, "system_mono".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn apply_serika_theme(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    let mut style = (*ctx.style_of(Theme::Dark)).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_DEEP;
    visuals.window_fill = BG_CARD;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.faint_bg_color = BG_SIDEBAR;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = BG_ACTIVE_ROW;
    visuals.selection.stroke = Stroke::new(1.0, BORDER_STRONG);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);

    visuals.widgets.inactive.bg_fill = BG_INPUT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);

    visuals.widgets.active.bg_fill = BG_CARD_HOVER;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    style.visuals = visuals;
    ctx.set_style_of(Theme::Dark, style);
}
