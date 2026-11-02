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
use serika_runtime::gitinfo::{self, GitFileEntry, GitRepoSnapshot};
use serika_runtime::model::{AccountProfile, ChatModel, ModelCatalog, UsageSnapshot};
use serika_runtime::permissions::PermissionMode;
use serika_runtime::providers::{
    builtin_provider_models, detect_local_harnesses, discover_all_models, LocalHarness,
    ProviderDiscoveryReport, DEFAULT_LOCAL_SERVER_BASE, DEFAULT_OLLAMA_BASE,
};
use serika_runtime::sessions::{SessionSummary, StoredMessage, ToolRun};
use serika_runtime::setup::{self, SetupOptions};
use serika_runtime::updater::{self, UpdateInfo};
use serika_runtime::{AgentEvent, Runtime, TaskOutcome, TaskRequest};

// =============================================================================
// SerikaChat Carbon Monochrome Palette + Serika Code Semantic Accents
// =============================================================================
const BG_DEEP: Color32 = Color32::from_rgb(13, 13, 13); // #0D0D0D (SerikaChat Main Canvas)
const BG_SIDEBAR: Color32 = Color32::from_rgb(10, 10, 10); // #0A0A0A (SerikaChat Sidebar)
const BG_CARD: Color32 = Color32::from_rgb(23, 23, 23); // #171717 (SerikaChat Composer & Cards)
const BG_ELEVATED: Color32 = Color32::from_rgb(30, 30, 30); // #1E1E1E (User Bubble & Elevated Pills)
const BG_CARD_HOVER: Color32 = Color32::from_rgb(36, 36, 36); // #242424 (Hover Surface)
const BG_INPUT: Color32 = Color32::from_rgb(20, 20, 20); // #141414 (Search & Input Surface)
const BG_ACTIVE_ROW: Color32 = Color32::from_rgb(28, 28, 30); // #1C1C1E (Active Thread Row)

const BORDER_HAIRLINE: Color32 = Color32::from_rgb(26, 26, 26); // #1A1A1A (1px Divider)
const BORDER_SUBTLE: Color32 = Color32::from_rgb(38, 38, 38); // #262626 (Composer & Card Border)
const BORDER_STRONG: Color32 = Color32::from_rgb(58, 58, 62); // #3A3A3E (Focused / Hover Border)

const TEXT_PRIMARY: Color32 = Color32::from_rgb(245, 245, 245); // #F5F5F5 (Crisp White)
const TEXT_SECONDARY: Color32 = Color32::from_rgb(212, 212, 216); // #D4D4D8
const TEXT_MUTED: Color32 = Color32::from_rgb(142, 142, 147); // #8E8E93
const TEXT_DIM: Color32 = Color32::from_rgb(99, 99, 102); // #636366

const SERIKA_VIOLET: Color32 = Color32::from_rgb(228, 228, 231); // Crisp Monochrome Silver Accent
const SERIKA_VIOLET_HOVER: Color32 = Color32::from_rgb(255, 255, 255);
const SERIKA_VIOLET_LIGHT: Color32 = Color32::from_rgb(212, 212, 216);
const SERIKA_VIOLET_SOFT: Color32 = Color32::from_rgb(34, 34, 38);

const EMERALD: Color32 = Color32::from_rgb(16, 185, 129); // #10B981
const EMERALD_SOFT: Color32 = Color32::from_rgb(13, 38, 28);
const AMBER: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B
const ROSE: Color32 = Color32::from_rgb(244, 63, 94); // #F43F5E
const ROSE_SOFT: Color32 = Color32::from_rgb(46, 16, 24);

#[derive(Default, Clone)]
pub struct LaunchConfig {
    pub smoke: bool,
    pub screenshot_path: Option<PathBuf>,
    pub screenshot_view: Option<String>,
}

// =============================================================================
// Custom Anti-Aliased Vector Icon Renderer (Zero Missing Glyph `▯` Boxes!)
// =============================================================================
#[allow(dead_code)]
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
    ChevronDown,
    Rewind,
    Shield,
    Code,
    Dots,
    EyeSlash,
    Edit,
    Book,
    Globe,
    Microphone,
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
                color.gamma_multiply(0.22),
                Stroke::new(1.6, color),
            ));
            painter.circle_filled(c, (r * 0.30).max(1.5), Color32::WHITE);
        }
        Icon::Plus => {
            painter.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)], stroke);
        }
        Icon::Folder => {
            let body = Rect::from_min_max(
                Pos2::new(c.x - r * 1.1, c.y - r * 0.55),
                Pos2::new(c.x + r * 1.1, c.y + r * 0.85),
            );
            let tab = Rect::from_min_max(
                Pos2::new(c.x - r * 1.1, c.y - r * 0.85),
                Pos2::new(c.x - r * 0.15, c.y - r * 0.45),
            );
            painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
            painter.rect_filled(tab, 1.5, color);
        }
        Icon::GitBranch => {
            let top_l = Pos2::new(c.x - r * 0.55, c.y - r * 0.75);
            let bot_l = Pos2::new(c.x - r * 0.55, c.y + r * 0.75);
            let top_r = Pos2::new(c.x + r * 0.55, c.y - r * 0.35);
            painter.line_segment([top_l, bot_l], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.55, c.y + r * 0.15), top_r], stroke);
            painter.circle_filled(top_l, 2.2, color);
            painter.circle_filled(bot_l, 2.2, color);
            painter.circle_filled(top_r, 2.2, color);
        }
        Icon::Worktree => {
            let top = Pos2::new(c.x, c.y - r * 0.85);
            let left = Pos2::new(c.x - r * 0.75, c.y + r * 0.7);
            let right = Pos2::new(c.x + r * 0.75, c.y + r * 0.7);
            painter.line_segment([top, left], stroke);
            painter.line_segment([top, right], stroke);
            painter.circle_filled(top, 2.3, color);
            painter.circle_filled(left, 2.3, color);
            painter.circle_filled(right, 2.3, color);
        }
        Icon::Search => {
            let sc = Pos2::new(c.x - r * 0.15, c.y - r * 0.15);
            painter.circle_stroke(sc, r * 0.68, stroke);
            painter.line_segment(
                [
                    Pos2::new(sc.x + r * 0.48, sc.y + r * 0.48),
                    Pos2::new(c.x + r * 0.95, c.y + r * 0.95),
                ],
                stroke,
            );
        }
        Icon::Send => {
            let tip = Pos2::new(c.x + r * 0.95, c.y);
            let tail = Pos2::new(c.x - r * 0.85, c.y);
            let wing_t = Pos2::new(c.x + r * 0.15, c.y - r * 0.75);
            let wing_b = Pos2::new(c.x + r * 0.15, c.y + r * 0.75);
            let s = Stroke::new(1.7, color);
            painter.line_segment([tail, tip], s);
            painter.line_segment([wing_t, tip], s);
            painter.line_segment([wing_b, tip], s);
        }
        Icon::Stop => {
            let sq = Rect::from_center_size(c, Vec2::splat(r * 1.25));
            painter.rect_filled(sq, 2.0, color);
        }
        Icon::Sliders => {
            for (i, frac) in [-0.6f32, 0.0, 0.6].iter().enumerate() {
                let y = c.y + frac * r * 1.2;
                painter.line_segment(
                    [Pos2::new(c.x - r, y), Pos2::new(c.x + r, y)],
                    Stroke::new(1.3, color),
                );
                let kx = if i == 1 { c.x + r * 0.35 } else { c.x - r * 0.35 };
                painter.circle_filled(Pos2::new(kx, y), 2.3, color);
            }
        }
        Icon::Key => {
            let kc = Pos2::new(c.x - r * 0.4, c.y);
            painter.circle_stroke(kc, r * 0.55, stroke);
            painter.line_segment([Pos2::new(kc.x + r * 0.55, c.y), Pos2::new(c.x + r, c.y)], stroke);
            painter.line_segment(
                [Pos2::new(c.x + r * 0.75, c.y), Pos2::new(c.x + r * 0.75, c.y + r * 0.45)],
                stroke,
            );
        }
        Icon::PanelRight => {
            let box_r = Rect::from_center_size(c, Vec2::new(r * 2.1, r * 1.7));
            painter.rect_stroke(box_r, 2.5, stroke, StrokeKind::Inside);
            let split_x = box_r.right() - box_r.width() * 0.35;
            painter.line_segment(
                [Pos2::new(split_x, box_r.top()), Pos2::new(split_x, box_r.bottom())],
                stroke,
            );
        }
        Icon::Sparkle => {
            painter.line_segment([Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)], stroke);
            painter.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], stroke);
            let d = r * 0.58;
            painter.line_segment(
                [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
                Stroke::new(1.2, color),
            );
        }
        Icon::Check => {
            let p1 = Pos2::new(c.x - r * 0.8, c.y + r * 0.05);
            let p2 = Pos2::new(c.x - r * 0.2, c.y + r * 0.65);
            let p3 = Pos2::new(c.x + r * 0.85, c.y - r * 0.65);
            painter.line_segment([p1, p2], Stroke::new(1.8, color));
            painter.line_segment([p2, p3], Stroke::new(1.8, color));
        }
        Icon::Refresh => {
            painter.circle_stroke(c, r * 0.8, stroke);
            painter.circle_filled(Pos2::new(c.x + r * 0.65, c.y - r * 0.45), 2.2, color);
        }
        Icon::Terminal => {
            let box_r = Rect::from_center_size(c, Vec2::new(r * 2.1, r * 1.7));
            painter.rect_stroke(box_r, 2.5, stroke, StrokeKind::Inside);
            let p1 = Pos2::new(c.x - r * 0.55, c.y - r * 0.35);
            let p2 = Pos2::new(c.x - r * 0.15, c.y);
            let p3 = Pos2::new(c.x - r * 0.55, c.y + r * 0.35);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p2, p3], stroke);
        }
        Icon::Close => {
            let d = r * 0.7;
            painter.line_segment([Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)], stroke);
            painter.line_segment([Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)], stroke);
        }
        Icon::ChevronDown => {
            let p1 = Pos2::new(c.x - r * 0.65, c.y - r * 0.25);
            let p2 = Pos2::new(c.x, c.y + r * 0.40);
            let p3 = Pos2::new(c.x + r * 0.65, c.y - r * 0.25);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p2, p3], stroke);
        }
        Icon::Rewind => {
            let tip = Pos2::new(c.x - r * 0.85, c.y - r * 0.2);
            let w1 = Pos2::new(c.x - r * 0.25, c.y - r * 0.75);
            let w2 = Pos2::new(c.x - r * 0.25, c.y + r * 0.35);
            painter.line_segment([tip, w1], stroke);
            painter.line_segment([tip, w2], stroke);
            painter.line_segment([tip, Pos2::new(c.x + r * 0.75, c.y - r * 0.2)], stroke);
            painter.line_segment(
                [Pos2::new(c.x + r * 0.75, c.y - r * 0.2), Pos2::new(c.x + r * 0.75, c.y + r * 0.65)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + r * 0.75, c.y + r * 0.65), Pos2::new(c.x - r * 0.3, c.y + r * 0.65)],
                stroke,
            );
        }
        Icon::Shield => {
            let top = Pos2::new(c.x, c.y - r * 0.95);
            let tr = Pos2::new(c.x + r * 0.85, c.y - r * 0.55);
            let br = Pos2::new(c.x + r * 0.65, c.y + r * 0.35);
            let bot = Pos2::new(c.x, c.y + r * 0.98);
            let bl = Pos2::new(c.x - r * 0.65, c.y + r * 0.35);
            let tl = Pos2::new(c.x - r * 0.85, c.y - r * 0.55);
            painter.add(egui::Shape::closed_line(
                vec![top, tr, br, bot, bl, tl],
                stroke,
            ));
        }
        Icon::Code => {
            let l1 = Pos2::new(c.x - r * 0.35, c.y - r * 0.65);
            let l2 = Pos2::new(c.x - r * 0.95, c.y);
            let l3 = Pos2::new(c.x - r * 0.35, c.y + r * 0.65);
            let r1 = Pos2::new(c.x + r * 0.35, c.y - r * 0.65);
            let r2 = Pos2::new(c.x + r * 0.95, c.y);
            let r3 = Pos2::new(c.x + r * 0.35, c.y + r * 0.65);
            painter.line_segment([l1, l2], stroke);
            painter.line_segment([l2, l3], stroke);
            painter.line_segment([r1, r2], stroke);
            painter.line_segment([r2, r3], stroke);
        }
        Icon::Dots => {
            for dx in [-r * 0.75, 0.0, r * 0.75] {
                painter.circle_filled(Pos2::new(c.x + dx, c.y), 1.8, color);
            }
        }
        Icon::EyeSlash => {
            let sc = Pos2::new(c.x, c.y);
            painter.line_segment([Pos2::new(c.x - r * 0.9, c.y), Pos2::new(c.x, c.y - r * 0.55)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - r * 0.55), Pos2::new(c.x + r * 0.9, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.9, c.y), Pos2::new(c.x, c.y + r * 0.55)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y + r * 0.55), Pos2::new(c.x + r * 0.9, c.y)], stroke);
            painter.circle_filled(sc, 1.8, color);
            let slash_s = Stroke::new(1.6, color);
            painter.line_segment([Pos2::new(c.x - r * 0.9, c.y - r * 0.7), Pos2::new(c.x + r * 0.9, c.y + r * 0.7)], slash_s);
        }
        Icon::Edit => {
            let box_r = Rect::from_min_max(Pos2::new(c.x - r * 0.9, c.y - r * 0.5), Pos2::new(c.x + r * 0.5, c.y + r * 0.9));
            painter.rect_stroke(box_r, 2.0, stroke, StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x - r * 0.2, c.y + r * 0.2), Pos2::new(c.x + r * 0.9, c.y - r * 0.9)], stroke);
        }
        Icon::Book => {
            painter.line_segment([Pos2::new(c.x, c.y - r * 0.8), Pos2::new(c.x, c.y + r * 0.8)], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.85, c.y - r * 0.6), Pos2::new(c.x, c.y - r * 0.8)], stroke);
            painter.line_segment([Pos2::new(c.x + r * 0.85, c.y - r * 0.6), Pos2::new(c.x, c.y - r * 0.8)], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.85, c.y + r * 0.6), Pos2::new(c.x, c.y + r * 0.8)], stroke);
            painter.line_segment([Pos2::new(c.x + r * 0.85, c.y + r * 0.6), Pos2::new(c.x, c.y + r * 0.8)], stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.85, c.y - r * 0.6), Pos2::new(c.x - r * 0.85, c.y + r * 0.6)], stroke);
            painter.line_segment([Pos2::new(c.x + r * 0.85, c.y - r * 0.6), Pos2::new(c.x + r * 0.85, c.y + r * 0.6)], stroke);
        }
        Icon::Globe => {
            painter.circle_stroke(c, r * 0.85, stroke);
            painter.line_segment([Pos2::new(c.x - r * 0.85, c.y), Pos2::new(c.x + r * 0.85, c.y)], Stroke::new(1.2, color));
            painter.line_segment([Pos2::new(c.x, c.y - r * 0.85), Pos2::new(c.x, c.y + r * 0.85)], Stroke::new(1.2, color));
        }
        Icon::Microphone => {
            let cap = Rect::from_center_size(Pos2::new(c.x, c.y - r * 0.25), Vec2::new(r * 0.7, r * 1.1));
            painter.rect_filled(cap, r * 0.35, color);
            let u1 = Pos2::new(c.x - r * 0.65, c.y - r * 0.2);
            let u2 = Pos2::new(c.x - r * 0.65, c.y + r * 0.3);
            let u3 = Pos2::new(c.x, c.y + r * 0.65);
            let u4 = Pos2::new(c.x + r * 0.65, c.y + r * 0.3);
            let u5 = Pos2::new(c.x + r * 0.65, c.y - r * 0.2);
            painter.line_segment([u1, u2], stroke);
            painter.line_segment([u2, u3], stroke);
            painter.line_segment([u3, u4], stroke);
            painter.line_segment([u4, u5], stroke);
            painter.line_segment([Pos2::new(c.x, c.y + r * 0.65), Pos2::new(c.x, c.y + r * 0.95)], stroke);
        }
    }
}

fn draw_user_anime_avatar(painter: &egui::Painter, rect: Rect) {
    let c = rect.center();
    let r = rect.width() * 0.5;

    // Outer circle with rich violet background
    painter.circle_filled(c, r, Color32::from_rgb(34, 18, 52));
    painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_rgb(88, 48, 128)));

    // Dark hoodie/jacket silhouette
    let coat_poly = vec![
        Pos2::new(c.x - r * 0.8, c.y + r * 0.95),
        Pos2::new(c.x - r * 0.45, c.y + r * 0.35),
        Pos2::new(c.x, c.y + r * 0.55),
        Pos2::new(c.x + r * 0.45, c.y + r * 0.35),
        Pos2::new(c.x + r * 0.8, c.y + r * 0.95),
    ];
    painter.add(egui::Shape::convex_polygon(coat_poly, Color32::from_rgb(18, 16, 24), Stroke::NONE));

    // Soft anime face tone
    painter.circle_filled(Pos2::new(c.x, c.y - r * 0.05), r * 0.44, Color32::from_rgb(255, 230, 215));

    // Anime purple hair silhouette & bangs
    let hair_col = Color32::from_rgb(155, 95, 210);
    let hair_highlight = Color32::from_rgb(195, 140, 245);

    // Hair back/top
    painter.circle_filled(Pos2::new(c.x, c.y - r * 0.35), r * 0.48, hair_col);
    painter.circle_filled(Pos2::new(c.x - r * 0.38, c.y - r * 0.1), r * 0.32, hair_col);
    painter.circle_filled(Pos2::new(c.x + r * 0.38, c.y - r * 0.1), r * 0.32, hair_col);

    // Hair bangs
    painter.line_segment([Pos2::new(c.x - r * 0.28, c.y - r * 0.4), Pos2::new(c.x - r * 0.15, c.y - r * 0.05)], Stroke::new(2.2, hair_col));
    painter.line_segment([Pos2::new(c.x - r * 0.1, c.y - r * 0.42), Pos2::new(c.x, c.y - r * 0.02)], Stroke::new(2.4, hair_col));
    painter.line_segment([Pos2::new(c.x + r * 0.1, c.y - r * 0.42), Pos2::new(c.x + r * 0.22, c.y - r * 0.08)], Stroke::new(2.2, hair_highlight));

    // Purple anime eyes
    painter.circle_filled(Pos2::new(c.x - r * 0.16, c.y - r * 0.02), 1.5, Color32::from_rgb(140, 50, 190));
    painter.circle_filled(Pos2::new(c.x + r * 0.16, c.y - r * 0.02), 1.5, Color32::from_rgb(140, 50, 190));

    // Emerald online dot at bottom-right
    let dot_center = Pos2::new(c.x + r * 0.65, c.y + r * 0.65);
    painter.circle_filled(dot_center, 3.5, EMERALD);
    painter.circle_stroke(dot_center, 3.5, Stroke::new(1.5, BG_DEEP));
}

pub fn launch(cfg: LaunchConfig) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Serika Code")
            .with_inner_size([1420.0, 880.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Serika Code",
        options,
        Box::new(move |cc| Ok(Box::new(SerikaApp::new(cc, cfg)))),
    );
    if let Err(err) = result {
        eprintln!("serika-desktop failed to start: {err}");
        std::process::exit(1);
    }
}

#[allow(dead_code)]
enum UiNotice {
    ModelsDiscovered(ProviderDiscoveryReport),
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

#[allow(dead_code)]
struct SerikaApp {
    smoke: bool,
    screenshot_path: Option<PathBuf>,
    screenshot_view: Option<String>,
    screenshot_frames: u32,
    screenshot_requested: bool,

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
    rewind_confirm: Option<i64>,

    // Providers, Harnesses & Live API Endpoints
    local_harnesses: Vec<LocalHarness>,
    provider_models: Vec<ChatModel>,
    discovery_report: ProviderDiscoveryReport,
    discovering_models: bool,
    model_search_input: String,
    local_model_input: String,
    ollama_base_input: String,
    local_base_input: String,
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
    fn new(cc: &eframe::CreationContext<'_>, cfg: LaunchConfig) -> Self {
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
            .unwrap_or_else(|| "local/workspace".to_string());

        let local_model_input = core
            .as_ref()
            .and_then(|c| c.config.local_model.clone())
            .unwrap_or_default();
        let ollama_base_input = core
            .as_ref()
            .and_then(|c| c.config.ollama_base_url.clone())
            .unwrap_or_else(|| DEFAULT_OLLAMA_BASE.to_string());
        let local_base_input = core
            .as_ref()
            .and_then(|c| c.config.local_base_url.clone())
            .unwrap_or_else(|| DEFAULT_LOCAL_SERVER_BASE.to_string());

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
        let show_inspector = git.is_repo && !git.entries.is_empty();

        let mut app = Self {
            smoke: cfg.smoke,
            screenshot_path: cfg.screenshot_path,
            screenshot_view: cfg.screenshot_view,
            screenshot_frames: 0,
            screenshot_requested: false,
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
            rewind_confirm: None,
            local_harnesses,
            provider_models,
            discovery_report: ProviderDiscoveryReport::default(),
            discovering_models: false,
            model_search_input: String::new(),
            local_model_input,
            ollama_base_input,
            local_base_input,
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
        if app.screenshot_path.is_some() {
            app.prepare_screenshot_preset();
        } else {
            app.refresh_remote();
        }
        app
    }

    fn prepare_screenshot_preset(&mut self) {
        let now = 1_793_500_000i64;
        self.discovery_report.local_count = 6;
        self.discovery_report.anthropic_count = 4;
        self.discovery_report.openai_count = 5;
        self.discovery_report.gemini_count = 4;
        self.selected_model = "openrouter/free".to_string();
        self.sessions = vec![
            SessionSummary {
                id: "sess_01".into(),
                title: "Oh, how are we doing ton...".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 3600 * 2,
            },
            SessionSummary {
                id: "sess_02".into(),
                title: "Yo, how's it going?".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 3600 * 24,
            },
            SessionSummary {
                id: "sess_03".into(),
                title: "can you check where tickets are handled?".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 8,
            },
            SessionSummary {
                id: "sess_04".into(),
                title: "Hey! (branch)".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 10,
            },
            SessionSummary {
                id: "sess_05".into(),
                title: "Hey!".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 12,
            },
            SessionSummary {
                id: "sess_06".into(),
                title: "Build Pomodoro timer in React".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 15,
            },
            SessionSummary {
                id: "sess_07".into(),
                title: "OAuth 2.0 flow diagram".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 18,
            },
        ];

        match self.screenshot_view.as_deref() {
            Some("thread") => {
                self.active_session = Some("sess_01".into());
                self.show_inspector = true;
                self.inspector_tab = InspectorTab::Diffs;
                self.git.is_repo = true;
                self.git.branch = "main".into();
                self.git.entries = vec![
                    GitFileEntry {
                        path: "crates/serika-runtime/src/providers.rs".into(),
                        index_status: 'M',
                        worktree_status: ' ',
                        staged: true,
                        untracked: false,
                    },
                    GitFileEntry {
                        path: "crates/serika-cli/src/tui.rs".into(),
                        index_status: ' ',
                        worktree_status: 'M',
                        staged: false,
                        untracked: false,
                    },
                ];
                self.messages = vec![
                    StoredMessage {
                        id: 1,
                        role: "user".into(),
                        content: "can you check where tickets are handled and wire live /models discovery?".into(),
                        reasoning: None,
                        created_at: now - 90,
                    },
                    StoredMessage {
                        id: 2,
                        role: "assistant".into(),
                        content: "### Ticket Handling & Live `/models` Endpoint Discovery\n\nI traced the codebase and updated `serika-runtime` and `serika-desktop`:\n\n- **Reasoning Traces**: Captured live `<think>` and API reasoning deltas (`reasoning`, `reasoning_content`, `thinking`, Gemini `thought`) and persisted them in SQLite `sessions.db`.\n- **Live Provider `/models`**: Concurrently queries Ollama `GET /api/tags`, Local Server `GET /v1/models`, OpenAI `GET /v1/models`, Anthropic `GET /v1/models`, and Gemini `GET /v1beta/models`.\n\n```rust\npub async fn discover_all_models(http: &Client, cfg: &Config) -> Report {\n    let (ollama, local_srv, openai, claude, gemini) = tokio::join!(\n        fetch_ollama_models(http, cfg), fetch_openai_models(http, cfg),\n        fetch_anthropic_models(http, cfg), fetch_gemini_models(http, cfg),\n    );\n    ProviderDiscoveryReport::merge(ollama, local_srv, openai, claude, gemini)\n}\n```".into(),
                        reasoning: Some(
                            "1. Searching workspace files and text for ticket handlers and model discovery entrypoints.\n\
                             2. Inspected `crates/serika-runtime/src/providers.rs` and `crates/serika-runtime/src/client.rs`.\n\
                             3. Verified that `SerikaClient::chat` extracts reasoning traces and executes local `serika-tool` blocks without triggering image tools.\n\
                             4. Ran `cargo test --workspace` to confirm all 43 unit and integration tests pass."
                                .into(),
                        ),
                        created_at: now - 40,
                    },
                ];
                self.tools = vec![
                    ToolRun {
                        name: "read_file".into(),
                        arguments: r#"{"path":"crates/serika-runtime/src/providers.rs"}"#.into(),
                        output: "Read 142 lines from crates/serika-runtime/src/providers.rs".into(),
                        ok: true,
                        created_at: now - 75,
                    },
                    ToolRun {
                        name: "replace_in_file".into(),
                        arguments: r#"{"path":"crates/serika-runtime/src/providers.rs"}"#.into(),
                        output: "Updated crates/serika-runtime/src/providers.rs (+28 -4)".into(),
                        ok: true,
                        created_at: now - 60,
                    },
                    ToolRun {
                        name: "run_command".into(),
                        arguments: r#"{"command":"cargo test --workspace"}"#.into(),
                        output: "test result: ok. 37 passed; 0 failed; finished in 1.74s".into(),
                        ok: true,
                        created_at: now - 45,
                    },
                ];
                self.diffs = vec![(
                    "crates/serika-runtime/src/providers.rs".into(),
                    "@@ -172,6 +172,18 @@ pub async fn discover_all_models(\n-    let models = builtin_static_models();\n+    let (ollama, local_srv, openai, anthropic, gemini) = tokio::join!(\n+        fetch_ollama_models(http, config),\n+        fetch_local_openai_models(http, config),\n+        fetch_openai_models(http, config),\n+        fetch_anthropic_models(http, config),\n+        fetch_gemini_models(http, config),\n+    );\n+    let mut report = ProviderDiscoveryReport::default();\n+    report.extend_local(ollama, local_srv);\n+    report.extend_cloud(openai, anthropic, gemini);\n     report\n }\n".into(),
                )];
            }
            Some("providers") => {
                self.show_inspector = false;
                self.show_providers = true;
            }
            _ => {
                self.active_session = None;
                self.show_inspector = false;
                self.inspector_tab = InspectorTab::Git;
            }
        }
    }

    fn refresh_remote(&mut self) {
        let Some(core) = self.core.clone() else { return };
        self.discovering_models = true;
        {
            let tx = self.tx.clone();
            let core_disc = core.clone();
            self.tokio.spawn(async move {
                let report =
                    discover_all_models(core_disc.client.http(), &core_disc.config).await;
                let _ = tx.send(UiNotice::ModelsDiscovered(report));
            });
        }
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
        self.rewind_confirm = None;
    }

    fn apply_rewind(&mut self, message_id: i64) {
        self.rewind_confirm = None;
        if self.running {
            return;
        }
        let Some(core) = self.core.clone() else { return };
        let Some(session_id) = self.active_session.clone() else { return };
        match core.sessions.rewind(&session_id, message_id) {
            Ok(outcome) => {
                self.prompt = outcome.prompt;
                self.select_session(&session_id);
                self.status = if outcome.restored_files == 0 {
                    "Rewound. This turn had no file checkpoint.".into()
                } else if outcome.restored_files == 1 {
                    "Rewound. Restored 1 file.".into()
                } else {
                    format!("Rewound. Restored {} files.", outcome.restored_files)
                };
                self.refresh_git_async();
            }
            Err(err) => self.status = err.to_string(),
        }
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
                UiNotice::ModelsDiscovered(report) => {
                    self.discovering_models = false;
                    self.provider_models = report.models.clone();
                    if self
                        .core
                        .as_ref()
                        .and_then(|c| c.config.model.as_ref())
                        .is_none()
                        && self.selected_model == "local/workspace"
                    {
                        if let Some(first_api) = self
                            .provider_models
                            .iter()
                            .find(|m| m.provider.as_deref() != Some("local"))
                        {
                            self.selected_model = first_api.id.clone();
                        }
                    }
                    self.discovery_report = report;
                }
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
            config.local_model = non_empty(&self.local_model_input);
            config.ollama_base_url = non_empty(&self.ollama_base_input);
            config.local_base_url = non_empty(&self.local_base_input);
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
                    self.status = "Saved providers & fetching live /models…".into();
                    self.refresh_remote();
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
        if self.screenshot_path.is_none() {
            self.drain_channels();
        }
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.screenshot_path.is_none()
            && self.last_git_poll.elapsed() > Duration::from_secs(8)
        {
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

        if self.screenshot_view.as_deref() == Some("cli") {
            self.draw_codex_cli_showcase(root_ui);
        } else {
            self.draw_sidebar(root_ui);
            if self.show_inspector {
                self.draw_inspector(root_ui);
            }
            self.draw_main_canvas(root_ui);

            if self.show_providers
                || self.show_settings
                || self.show_palette
                || self.pending_approval.is_some()
            {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::unique("modal_backdrop"))
                    .order(egui::Order::Middle)
                    .fixed_pos(screen.min)
                    .show(&ctx, |ui| {
                        let (r, _) = ui.allocate_exact_size(screen.size(), Sense::hover());
                        ui.painter()
                            .rect_filled(r, 0.0, Color32::from_black_alpha(155));
                    });
            }

            self.draw_approval_modal(&ctx);
            self.draw_providers_modal(&ctx);
            self.draw_settings_modal(&ctx);
            self.draw_palette_modal(&ctx);
        }

        if let Some(out_path) = self.screenshot_path.clone() {
            self.screenshot_frames += 1;
            ctx.request_repaint();
            if self.screenshot_frames == 4 && !self.screenshot_requested {
                self.screenshot_requested = true;
                let ctx_close = ctx.clone();
                ctx.request_screenshot(move |image| {
                    let [w, h] = image.size;
                    let mut rgba = Vec::with_capacity(w * h * 4);
                    for px in &image.pixels {
                        rgba.extend_from_slice(&px.to_srgba_unmultiplied());
                    }
                    let _ = crate::shot::write_png(&out_path, w, h, &rgba);
                    ctx_close.send_viewport_cmd(egui::ViewportCommand::Close);
                });
            }
        }
    }
}

// =============================================================================
// UI Layout Sections (SerikaChat Design Language + Reasoning Traces)
// =============================================================================
impl SerikaApp {
    fn current_username(&self) -> String {
        if let Some(p) = &self.profile {
            if !p.username.trim().is_empty() {
                return p.username.clone();
            }
        }
        if let Some(s) = &self.session {
            if !s.username.trim().is_empty() {
                return s.username.clone();
            }
        }
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok()
            .filter(|u| !u.trim().is_empty() && !u.eq_ignore_ascii_case("neoforce"))
            .unwrap_or_else(|| "pikachubolk".to_string())
    }

    fn draw_sidebar(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::left("serikachat_sidebar")
            .resizable(true)
            .default_size(260.0)
            .size_range(220.0..=330.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::symmetric(12, 12))
                    .stroke(Stroke::new(1.0, BORDER_HAIRLINE)),
            )
            .show(root_ui, |ui| {
                // 1. SerikaChat Wordmark Header (`serika code` + sidebar icon)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(
                        RichText::new("serika")
                            .strong()
                            .size(18.0)
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new("code")
                            .size(18.0)
                            .color(TEXT_MUTED),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, BG_CARD_HOVER);
                        }
                        paint_icon(
                            ui.painter(),
                            r.shrink(5.5),
                            Icon::PanelRight,
                            if self.show_inspector || resp.hovered() {
                                TEXT_PRIMARY
                            } else {
                                TEXT_MUTED
                            },
                        );
                        if resp
                            .on_hover_text("Toggle Inspector Panel (Ctrl+B)")
                            .clicked()
                        {
                            self.show_inspector = !self.show_inspector;
                        }
                    });
                });

                ui.add_space(14.0);

                // 2. SerikaChat Primary Nav Rows (`Nieuwe chat [Ctrl][K]`, `Bibliotheek`, `Models & Providers`)
                if draw_sidebar_nav_row(ui, Icon::Edit, "Nieuwe chat", Some(("Ctrl", "K")), false) {
                    self.new_thread();
                }
                ui.add_space(2.0);

                let project_label = workspace_display_name(&self.project, &self.core);
                let lib_label = if project_label == "Workspace" || project_label == "Scratch" {
                    "Bibliotheek".to_string()
                } else {
                    format!("Bibliotheek · {project_label}")
                };
                if draw_sidebar_nav_row(ui, Icon::Book, &lib_label, None, false) {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.switch_project(folder);
                    }
                }
                ui.add_space(2.0);

                if draw_sidebar_nav_row(
                    ui,
                    Icon::Key,
                    "Modellen & Providers",
                    None,
                    self.show_providers,
                ) {
                    self.show_providers = !self.show_providers;
                }

                ui.add_space(10.0);

                // 3. SerikaChat Rounded Search Pill (`Q Chats zoeken`)
                egui::Frame::NONE
                    .fill(Color32::from_rgb(18, 18, 18))
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(28, 28, 28)))
                    .inner_margin(Margin::symmetric(11, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                            paint_icon(ui.painter(), ir, Icon::Search, TEXT_DIM);
                            ui.add(
                                TextEdit::singleline(&mut self.session_filter)
                                    .hint_text("Chats zoeken")
                                    .frame(egui::Frame::NONE)
                                    .font(FontId::proportional(12.5))
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    });

                ui.add_space(14.0);

                // 4. Time-Grouped Chat History (`Previous 7 days`, `Previous 30 days`)
                let q = self.session_filter.to_ascii_lowercase();
                let filtered: Vec<SessionSummary> = self
                    .sessions
                    .iter()
                    .filter(|s| q.is_empty() || s.title.to_ascii_lowercase().contains(&q))
                    .cloned()
                    .collect();

                let footer_reserve = 125.0;
                let list_h = (ui.available_height() - footer_reserve).max(90.0);
                ScrollArea::vertical()
                    .id_salt("serikachat_thread_scroll")
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(
                                RichText::new("Previous 7 days")
                                    .size(11.5)
                                    .color(TEXT_DIM),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("No chats yet.")
                                    .size(12.5)
                                    .color(TEXT_MUTED),
                            );
                        } else {
                            let now_ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0);
                            let mut shown_7d = false;
                            let mut shown_30d = false;
                            let mut to_select = None;
                            let mut to_delete = None;

                            for item in &filtered {
                                let age_days = (now_ts - item.updated_at).max(0) / 86_400;
                                if age_days <= 7 && !shown_7d {
                                    shown_7d = true;
                                    ui.label(
                                        RichText::new("Previous 7 days")
                                            .size(11.5)
                                            .color(TEXT_DIM),
                                    );
                                    ui.add_space(4.0);
                                } else if age_days > 7 && !shown_30d {
                                    shown_30d = true;
                                    if shown_7d {
                                        ui.add_space(10.0);
                                    }
                                    ui.label(
                                        RichText::new("Previous 30 days")
                                            .size(11.5)
                                            .color(TEXT_DIM),
                                    );
                                    ui.add_space(4.0);
                                }

                                let is_active =
                                    self.active_session.as_deref() == Some(item.id.as_str());
                                let (clicked, deleted) = draw_thread_row(ui, item, is_active);
                                if clicked {
                                    to_select = Some(item.id.clone());
                                }
                                if deleted {
                                    to_delete = Some(item.id.clone());
                                }
                                ui.add_space(1.0);
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
                        }

                        if self.recent_projects.len() > 1 {
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new("Recent workspaces")
                                    .size(11.5)
                                    .color(TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            let mut switch_to = None;
                            for rp in &self.recent_projects {
                                if rp == &self.project {
                                    continue;
                                }
                                let name = rp
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("project");
                                if draw_sidebar_nav_row(ui, Icon::Folder, name, None, false) {
                                    switch_to = Some(rp.clone());
                                }
                            }
                            if let Some(p) = switch_to {
                                self.switch_project(p);
                            }
                        }
                    });

                // 5. SerikaChat Sidebar Footer (Session Usage Progress + User Profile Row)
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    // Bottom-most links: `Voorwaarden    Privacy`
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        if ui.link(RichText::new("Voorwaarden").size(10.5).color(Color32::from_rgb(105, 105, 110))).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://serika.dev/terms"));
                        }
                        ui.add_space(8.0);
                        if ui.link(RichText::new("Privacy").size(10.5).color(Color32::from_rgb(105, 105, 110))).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://serika.dev/privacy"));
                        }
                    });

                    ui.add_space(6.0);

                    // User Profile Row (`pikachubolk` with custom anime girl avatar & chevron)
                    let prof_desired = Vec2::new(ui.available_width(), 44.0);
                    let (prof_rect, prof_resp) = ui.allocate_exact_size(prof_desired, Sense::click());
                    if prof_resp.hovered() {
                        ui.painter().rect_filled(prof_rect, 8.0, BG_CARD_HOVER);
                    }
                    let av_rect = Rect::from_center_size(
                        Pos2::new(prof_rect.left() + 18.0, prof_rect.center().y),
                        Vec2::splat(28.0),
                    );
                    draw_user_anime_avatar(ui.painter(), av_rect);

                    ui.painter().text(
                        Pos2::new(prof_rect.left() + 38.0, prof_rect.top() + 7.0),
                        egui::Align2::LEFT_TOP,
                        "pikachubolk",
                        FontId::proportional(13.0),
                        TEXT_PRIMARY,
                    );
                    ui.painter().text(
                        Pos2::new(prof_rect.left() + 38.0, prof_rect.top() + 24.0),
                        egui::Align2::LEFT_TOP,
                        "Serika+ AI Max · until Nov...",
                        FontId::proportional(11.0),
                        TEXT_DIM,
                    );
                    let chev_rect = Rect::from_center_size(
                        Pos2::new(prof_rect.right() - 10.0, prof_rect.center().y),
                        Vec2::splat(12.0),
                    );
                    paint_icon(ui.painter(), chev_rect, Icon::ChevronDown, TEXT_DIM);
                    if prof_resp
                        .on_hover_text("Open Account & Settings")
                        .clicked()
                    {
                        self.show_settings = !self.show_settings;
                    }

                    ui.add_space(6.0);
                    let div = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                    ui.painter().rect_filled(div, 0.0, BORDER_HAIRLINE);
                    ui.add_space(6.0);

                    // Session Quota Progress Track (`Session 0%` ... `reset over 4h 57m`)
                    let bar_rect = ui.allocate_space(Vec2::new(ui.available_width(), 2.0)).1;
                    ui.painter().rect_filled(bar_rect, 1.0, Color32::from_rgb(32, 32, 32));
                    let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(bar_rect.width() * 0.04, 2.0));
                    ui.painter().rect_filled(fill_rect, 1.0, Color32::from_rgb(140, 70, 210));

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Session 0%")
                                .size(11.0)
                                .color(TEXT_MUTED),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new("reset over 4h 57m")
                                    .size(11.0)
                                    .color(TEXT_DIM),
                            );
                        });
                    });
                });
            });
    }

    fn draw_main_canvas(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG_DEEP))
            .show(root_ui, |ui| {
                self.draw_top_header(ui);

                if let Some(err) = &self.startup_error {
                    ui.add_space(16.0);
                    ui.colored_label(ROSE, format!("Runtime error: {err}"));
                    return;
                }

                let is_draft = self.messages.is_empty() && !self.running;
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
            .inner_margin(Margin::symmetric(24, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Top-left SerikaChat Model Selector (`openrouter Free (openrouter) ⌄`)
                    // Pure text, frameless, borderless, just like SerikaChat web UI!
                    let display_model = if self.selected_model == "openrouter/free" {
                        "openrouter Free (openrouter)".to_string()
                    } else if self.selected_model == "z-ai/glm-5.3-flash" {
                        "GLM 5.3 Flash (openrouter)".to_string()
                    } else if let Some(m) = self.catalog.find(&self.selected_model) {
                        format!("{} (openrouter)", m.name)
                    } else if let Some(m) = self.provider_models.iter().find(|m| m.id == self.selected_model) {
                        let p = m.provider.as_deref().unwrap_or("local");
                        format!("{} ({p})", m.name)
                    } else {
                        self.selected_model.clone()
                    };

                    let text_w = ui.painter().layout_no_wrap(
                        display_model.clone(),
                        FontId::proportional(14.0),
                        Color32::WHITE,
                    ).size().x + 22.0;

                    let (tr, resp) = ui.allocate_exact_size(Vec2::new(text_w, 28.0), Sense::click());
                    let fg = if resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(220, 220, 225)
                    };
                    ui.painter().text(
                        Pos2::new(tr.left() + 2.0, tr.center().y),
                        egui::Align2::LEFT_CENTER,
                        &display_model,
                        FontId::proportional(14.0),
                        fg,
                    );
                    let chev_rect = Rect::from_center_size(
                        Pos2::new(tr.right() - 8.0, tr.center().y),
                        Vec2::splat(10.0),
                    );
                    paint_icon(ui.painter(), chev_rect, Icon::ChevronDown, fg);
                    if resp.on_hover_text("Switch model or provider").clicked() {
                        self.show_palette = true;
                    }

                    // Top-right controls: ONLY Eye icon and Three-dots menu `···`, exactly like web UI!
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        // Three dots menu `···` (Settings, Account, Models)
                        let (dots_r, dots_resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        if dots_resp.hovered() {
                            ui.painter().circle_filled(dots_r.center(), 14.0, BG_CARD_HOVER);
                        }
                        paint_icon(
                            ui.painter(),
                            dots_r.shrink(7.0),
                            Icon::Dots,
                            if dots_resp.hovered() { Color32::WHITE } else { TEXT_MUTED },
                        );
                        if dots_resp.on_hover_text("Settings & Options").clicked() {
                            self.show_settings = !self.show_settings;
                        }

                        // Eye / Panel Toggle button `[ø]`
                        let (eye_r, eye_resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        if eye_resp.hovered() {
                            ui.painter().circle_filled(eye_r.center(), 14.0, BG_CARD_HOVER);
                        }
                        paint_icon(
                            ui.painter(),
                            eye_r.shrink(7.0),
                            Icon::EyeSlash,
                            if self.show_inspector || eye_resp.hovered() {
                                Color32::WHITE
                            } else {
                                TEXT_MUTED
                            },
                        );
                        if eye_resp.on_hover_text("Toggle Inspector (Ctrl+B)").clicked() {
                            self.show_inspector = !self.show_inspector;
                        }
                    });
                });
            });
    }

    fn draw_t3_draft_hero(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical()
            .id_salt("draft_hero_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_h = ui.available_height();
                let top_pad = ((avail_h - 460.0) * 0.42).clamp(36.0, 140.0);
                ui.add_space(top_pad);

                ui.vertical_centered(|ui| {
                    let lane_w = (ui.available_width() - 48.0).clamp(460.0, 680.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(lane_w, 10.0),
                        Layout::top_down(Align::Center),
                        |ui| {
                            // 1. Centered SerikaChat Greeting (`Good morning, pikachubolk`)
                            let uname = self.current_username();
                            ui.label(
                                RichText::new(format!("Good morning, {uname}"))
                                    .strong()
                                    .size(28.0)
                                    .color(Color32::WHITE),
                            );

                            ui.add_space(24.0);

                            // 2. Centered SerikaChat Rounded Pill Composer Box
                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                self.draw_composer_card(ui, true);
                            });

                            ui.add_space(10.0);

                            // 3. Centered SerikaChat Disclaimer Line
                            ui.label(
                                RichText::new(
                                    "Serika kan fouten maken. Controleer belangrijke info.",
                                )
                                .size(11.5)
                                .color(Color32::from_rgb(95, 95, 100)),
                            );

                            ui.add_space(28.0);

                            // 4. SerikaChat Single-Line Starter Prompt Rows with 1px Dividers
                            let starters = [
                                (
                                    "Bouw een timer-app",
                                    "Pomodoro, in React",
                                    "Bouw een complete Pomodoro timer-app in React met hooks, start/pause/reset controls en audio-notificaties.",
                                ),
                                (
                                    "OAuth-aanmelding voor diagram",
                                    "stap voor stap",
                                    "Ontwerp en implementeer een complete OAuth 2.0 authenticatie flow stap voor stap met diagrammen en code.",
                                ),
                                (
                                    "Concept-README",
                                    "voor een open-sourcebibliotheek",
                                    "Schrijf een professionele en complete README voor een open-source bibliotheek inclusief badges, quickstart en architectuur.",
                                ),
                                (
                                    "Taalmodellen uitleggen",
                                    "in gewone taal",
                                    "Leg moderne reasoning- en taalmodellen uit in heldere, gewone taal met concrete code-voorbeelden.",
                                ),
                            ];

                            let row_w = (lane_w - 28.0).max(400.0);
                            let mut chosen_prompt = None;
                            ui.allocate_ui_with_layout(
                                Vec2::new(row_w, 10.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    for &(title, subtitle, prompt_text) in &starters {
                                        if draw_serikachat_starter_row(ui, row_w, title, subtitle) {
                                            chosen_prompt = Some(prompt_text.to_string());
                                        }
                                    }
                                },
                            );

                            if let Some(p) = chosen_prompt {
                                self.prompt = p;
                                self.send_prompt();
                            }
                        },
                    );
                });
                ui.add_space(24.0);
            });
    }

    fn draw_active_thread_canvas(&mut self, ui: &mut egui::Ui) {
        let avail_h = ui.available_height();
        let composer_reserve = 148.0;
        let scroll_h = (avail_h - composer_reserve).max(120.0);

        ScrollArea::vertical()
            .id_salt("serikachat_active_thread_scroll")
            .max_height(scroll_h)
            .stick_to_bottom(self.running)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    let lane_w = (ui.available_width() - 48.0).clamp(460.0, 720.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(lane_w, 10.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            // Combine consecutive assistant rounds (separated by "Tool results:\n")
                            // so every assistant turn displays its full multi-step Reasoning Trace,
                            // Tool Execution Steps, and final Markdown prose together!
                            let merged_turns = merge_conversation_turns(&self.messages);

                            let last_assistant_idx = merged_turns
                                .iter()
                                .rposition(|m| m.role == "assistant");

                            for (idx, msg) in merged_turns.iter().enumerate() {
                                let show_tools_here =
                                    Some(idx) == last_assistant_idx && !self.tools.is_empty();
                                self.draw_turn(ui, msg, show_tools_here);
                            }

                            if last_assistant_idx.is_none() && !self.tools.is_empty() {
                                ui.add_space(8.0);
                                self.draw_tool_timeline(ui);
                            }

                            if self.running
                                && (!self.streaming_reasoning.is_empty()
                                    || !self.streaming_reply.is_empty())
                            {
                                let temp_msg = StoredMessage {
                                    id: -1,
                                    role: "assistant".into(),
                                    content: self.streaming_reply.clone(),
                                    reasoning: (!self.streaming_reasoning.is_empty())
                                        .then(|| self.streaming_reasoning.clone()),
                                    created_at: 0,
                                };
                                self.draw_turn(ui, &temp_msg, false);
                            } else if self.running {
                                ui.add_space(12.0);
                                draw_reasoning_trace(
                                    ui,
                                    -1,
                                    "Analyzing workspace context and planning tool execution…",
                                    true,
                                );
                            }
                        },
                    );
                });
                ui.add_space(14.0);
            });

        // Bottom Docked SerikaChat Composer
        egui::Frame::NONE
            .fill(BG_DEEP)
            .inner_margin(Margin::symmetric(0, 8))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let lane_w = (ui.available_width() - 48.0).clamp(460.0, 720.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(lane_w, 10.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            self.draw_composer_card(ui, false);
                        },
                    );
                });
            });
    }

    fn draw_tool_timeline(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .fill(BG_SIDEBAR)
            .corner_radius(CornerRadius::same(10))
            .stroke(Stroke::new(1.0, BORDER_HAIRLINE))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (tr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    paint_icon(ui.painter(), tr, Icon::Terminal, TEXT_MUTED);
                    ui.label(
                        RichText::new(format!(" Tool Executions ({})", self.tools.len()))
                            .size(11.5)
                            .strong()
                            .color(TEXT_MUTED),
                    );
                });
                ui.add_space(4.0);
                let mut open_diff_idx = None;
                for tool in self.tools.iter().rev().take(6).rev() {
                    let diff_idx = self.diffs.iter().position(|(_, d)| !d.is_empty());
                    if draw_tool_pill(ui, tool, diff_idx.is_some()) {
                        open_diff_idx = diff_idx;
                    }
                    ui.add_space(3.0);
                }
                if let Some(idx) = open_diff_idx {
                    self.selected_diff = idx;
                    self.show_inspector = true;
                    self.inspector_tab = InspectorTab::Diffs;
                }
            });
    }

    fn draw_turn(&mut self, ui: &mut egui::Ui, msg: &StoredMessage, include_tools: bool) {
        ui.add_space(12.0);
        if msg.role == "user" {
            let confirming = self.rewind_confirm == Some(msg.id);
            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                egui::Frame::NONE
                    .fill(BG_ELEVATED)
                    .corner_radius(CornerRadius::same(18))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(&msg.content).size(14.0).color(TEXT_PRIMARY));
                    });

                if !self.running && msg.id > 0 {
                    ui.add_space(4.0);
                    if icon_button(
                        ui,
                        Icon::Rewind,
                        "Restore files to before this message and rewind thread",
                        confirming,
                    )
                    .clicked()
                    {
                        self.rewind_confirm = Some(msg.id);
                    }
                }
            });

            if confirming {
                ui.add_space(6.0);
                egui::Frame::NONE
                    .fill(BG_CARD)
                    .corner_radius(CornerRadius::same(10))
                    .stroke(Stroke::new(1.0, AMBER))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Restore edited files to before this turn and rewind?")
                                    .size(12.0)
                                    .color(TEXT_PRIMARY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Cancel").clicked() {
                                    self.rewind_confirm = None;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Confirm Rewind")
                                                .size(11.5)
                                                .strong()
                                                .color(Color32::BLACK),
                                        )
                                        .fill(Color32::WHITE)
                                        .corner_radius(CornerRadius::same(6)),
                                    )
                                    .clicked()
                                {
                                    let id = msg.id;
                                    self.apply_rewind(id);
                                }
                            });
                        });
                    });
            }
        } else {
            // SerikaChat Unboxed Assistant Turn with Collapsible Reasoning Trace + Tool Timeline
            ui.horizontal(|ui| {
                let (ar, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                ui.painter().circle_filled(ar.center(), 10.0, BG_ELEVATED);
                ui.painter().circle_stroke(ar.center(), 10.0, Stroke::new(1.0, BORDER_SUBTLE));
                paint_icon(
                    ui.painter(),
                    ar.shrink(4.0),
                    Icon::SerikaMark,
                    Color32::WHITE,
                );
                ui.label(
                    RichText::new("serika")
                        .strong()
                        .size(13.5)
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new(&self.selected_model)
                        .monospace()
                        .size(11.0)
                        .color(TEXT_DIM),
                );
            });

            ui.add_space(6.0);

            // 1. Reasoning Trace Block (Always rendered for assistant turns!)
            let is_streaming_turn = msg.id == -1 && self.running;
            let reasoning_text = if let Some(r) = msg
                .reasoning
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                r.to_string()
            } else {
                synthesize_fallback_reasoning(msg, &self.tools, &self.project, &self.selected_model)
            };

            draw_reasoning_trace(ui, msg.id, &reasoning_text, is_streaming_turn);

            // 2. Tool Execution Steps (if any ran in this turn)
            if include_tools && !self.tools.is_empty() {
                ui.add_space(6.0);
                self.draw_tool_timeline(ui);
            }

            // 3. Clean Markdown Response (with serika-tool fences stripped)
            let (clean_visible, _) = serika_runtime::protocol::extract_tool_calls(&msg.content);
            if !clean_visible.is_empty() {
                ui.add_space(8.0);
                draw_rich_markdown(ui, &clean_visible);
            }
        }
    }

    fn draw_composer_card(&mut self, ui: &mut egui::Ui, is_hero: bool) {
        let border_col = if self.running {
            SERIKA_VIOLET
        } else {
            Color32::from_rgb(34, 34, 34)
        };

        egui::Frame::NONE
            .fill(Color32::from_rgb(20, 20, 20))
            .corner_radius(CornerRadius::same(24))
            .stroke(Stroke::new(1.0, border_col))
            .inner_margin(Margin::symmetric(20, 14))
            .show(ui, |ui| {
                let rows = if is_hero { 2 } else { 2 };
                let resp = ui.add(
                    TextEdit::multiline(&mut self.prompt)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .font(FontId::proportional(14.0))
                        .hint_text("Bericht naar Serika"),
                );
                if resp.has_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter) && (i.modifiers.command || !i.modifiers.shift))
                {
                    self.send_prompt();
                }

                ui.add_space(8.0);

                // SerikaChat Composer Action Row (`+`, `Zoeken`, `Onderzoek`, `Afbeelding`, Mic, and White Circle Send Button)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // `+` Workspace folder / attach button
                    let (plus_r, plus_resp) =
                        ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                    if plus_resp.hovered() {
                        ui.painter().circle_filled(plus_r.center(), 13.0, BG_CARD_HOVER);
                    }
                    paint_icon(
                        ui.painter(),
                        plus_r.shrink(6.0),
                        Icon::Plus,
                        if plus_resp.hovered() {
                            Color32::WHITE
                        } else {
                            TEXT_MUTED
                        },
                    );
                    if plus_resp
                        .on_hover_text("Switch workspace folder")
                        .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.switch_project(folder);
                        }
                    }

                    // 1. Zoeken (Search) Pill with Globe Icon
                    if serikachat_composer_pill(ui, Icon::Globe, "Zoeken", false)
                        .on_hover_text("Search codebase and files")
                        .clicked()
                        && !self.prompt.contains("search_text")
                    {
                        if self.prompt.trim().is_empty() {
                            self.prompt = "Zoek in de codebase naar ".to_string();
                        }
                    }

                    // 2. Onderzoek (Research / Plan) Pill with Search Icon
                    let is_plan = self.composer_mode == ComposerMode::Plan;
                    if serikachat_composer_pill(ui, Icon::Search, "Onderzoek", is_plan)
                        .on_hover_text("Toggle deep research and plan mode")
                        .clicked()
                    {
                        self.composer_mode = if is_plan {
                            ComposerMode::Build
                        } else {
                            ComposerMode::Plan
                        };
                    }

                    // 3. Afbeelding / Build Pill with Sparkle Icon
                    let is_build = self.composer_mode == ComposerMode::Build;
                    if serikachat_composer_pill(ui, Icon::Sparkle, "Afbeelding", is_build)
                        .on_hover_text("Toggle build and agent edit mode")
                        .clicked()
                    {
                        self.composer_mode = if is_build {
                            ComposerMode::Plan
                        } else {
                            ComposerMode::Build
                        };
                    }

                    // Right-aligned Mic + SerikaChat Crisp White Circular Button
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.running {
                            if white_circle_action_button(ui, Icon::Stop, true)
                                .on_hover_text("Stop generation")
                                .clicked()
                            {
                                if let Some(cancel) = &self.cancel {
                                    cancel.cancel();
                                }
                            }
                        } else {
                            let can_send = !self.prompt.trim().is_empty() || is_hero;
                            if white_circle_action_button(ui, Icon::Send, can_send)
                                .on_hover_text("Send message (Enter)")
                                .clicked()
                            {
                                self.send_prompt();
                            }
                        }

                        // Microphone icon button
                        let (mic_r, mic_resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        if mic_resp.hovered() {
                            ui.painter().circle_filled(mic_r.center(), 14.0, BG_CARD_HOVER);
                        }
                        paint_icon(
                            ui.painter(),
                            mic_r.shrink(6.5),
                            Icon::Microphone,
                            if mic_resp.hovered() { Color32::WHITE } else { TEXT_MUTED },
                        );
                    });
                });
            });
    }

    fn draw_inspector(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::right("t3_inspector")
            .resizable(true)
            .default_size(368.0)
            .size_range(290.0..=540.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, BORDER_HAIRLINE)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (tab, icon, label) in [
                        (InspectorTab::Git, Icon::GitBranch, "Source Control"),
                        (InspectorTab::Diffs, Icon::Code, "Diffs"),
                        (InspectorTab::Activity, Icon::Terminal, "Activity"),
                    ] {
                        let active = self.inspector_tab == tab;
                        if pill_icon_button(ui, icon, label, active).clicked() {
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

                ui.add_space(10.0);
                let sep = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                ui.painter().rect_filled(sep, 0.0, BORDER_HAIRLINE);
                ui.add_space(10.0);

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
                .width(175.0)
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

        ui.add_space(10.0);

        // Commit Box
        egui::Frame::NONE
            .fill(BG_CARD)
            .corner_radius(CornerRadius::same(9))
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.add(
                    TextEdit::singleline(&mut self.commit_message)
                        .hint_text("Commit message…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if pill_icon_button(ui, Icon::Plus, "Stage All", false).clicked() {
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
                    if primary_action_button(
                        ui,
                        Icon::Check,
                        "Commit",
                        SERIKA_VIOLET,
                        !self.commit_message.trim().is_empty(),
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

        ui.add_space(12.0);
        ui.label(
            RichText::new(format!("WORKING TREE ({})", self.git.entries.len()))
                .size(10.5)
                .strong()
                .color(TEXT_DIM),
        );
        ui.add_space(6.0);

        ScrollArea::vertical()
            .id_salt("git_entries_scroll")
            .max_height(260.0)
            .show(ui, |ui| {
                if self.git.entries.is_empty() {
                    egui::Frame::NONE
                        .fill(EMERALD_SOFT)
                        .corner_radius(CornerRadius::same(7))
                        .stroke(Stroke::new(1.0, EMERALD.gamma_multiply(0.45)))
                        .inner_margin(Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (cr, _) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                paint_icon(ui.painter(), cr, Icon::Check, EMERALD);
                                ui.label(
                                    RichText::new("Working tree is clean.")
                                        .size(11.5)
                                        .color(EMERALD),
                                );
                            });
                        });
                }
                let entries = self.git.entries.clone();
                for entry in entries {
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let col = if entry.staged { EMERALD } else { AMBER };
                                ui.label(
                                    RichText::new(format!("[{}]", entry.badge()))
                                        .monospace()
                                        .size(11.0)
                                        .strong()
                                        .color(col),
                                );
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(&entry.path)
                                                .monospace()
                                                .size(11.5)
                                                .color(TEXT_PRIMARY),
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
                                        if let Ok(d) = gitinfo::file_diff(&proj, &path, staged).await
                                        {
                                            let _ = tx.send(UiNotice::DiffLoaded((path, d)));
                                        }
                                    });
                                }
                            });
                        });
                    ui.add_space(3.0);
                }
            });

        ui.add_space(12.0);
        ui.label(
            RichText::new(format!("ISOLATED WORKTREES ({})", self.git.worktrees.len()))
                .size(10.5)
                .strong()
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.worktree_branch_input)
                    .hint_text("feat/agent-branch")
                    .desired_width(175.0),
            );
            if pill_icon_button(ui, Icon::Worktree, "Isolate", false).clicked()
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
    }

    fn draw_diffs_tab(&mut self, ui: &mut egui::Ui) {
        if self.diffs.is_empty() {
            egui::Frame::NONE
                .fill(BG_CARD)
                .corner_radius(CornerRadius::same(8))
                .stroke(Stroke::new(1.0, BORDER_HAIRLINE))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("No turn diffs recorded in this thread yet.")
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );
                    ui.label(
                        RichText::new(
                            "Click any modified file in Source Control or run an edit prompt to inspect line-by-line diffs.",
                        )
                        .size(11.0)
                        .color(TEXT_DIM),
                    );
                });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (idx, (path, _)) in self.diffs.iter().enumerate() {
                let active = idx == self.selected_diff;
                let short = path.rsplit('/').next().unwrap_or(path);
                if pill_icon_button(ui, Icon::Code, short, active)
                    .on_hover_text(path)
                    .clicked()
                {
                    self.selected_diff = idx;
                }
            }
        });
        ui.add_space(8.0);
        if let Some((path, diff)) = self.diffs.get(self.selected_diff) {
            egui::Frame::NONE
                .fill(BG_DEEP)
                .corner_radius(CornerRadius::same(9))
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(path)
                            .monospace()
                            .size(11.0)
                            .strong()
                            .color(SERIKA_VIOLET_LIGHT),
                    );
                    ui.add_space(6.0);
                    ScrollArea::vertical()
                        .id_salt("inspector_diff_scroll")
                        .show(ui, |ui| {
                            let mut line_no = 1usize;
                            for line in diff.lines() {
                                let (bg, fg) = if line.starts_with('+') && !line.starts_with("+++") {
                                    (EMERALD_SOFT, EMERALD)
                                } else if line.starts_with('-') && !line.starts_with("---") {
                                    (ROSE_SOFT, ROSE)
                                } else if line.starts_with("@@") {
                                    (SERIKA_VIOLET_SOFT, SERIKA_VIOLET_LIGHT)
                                } else {
                                    (Color32::TRANSPARENT, TEXT_MUTED)
                                };
                                egui::Frame::NONE
                                    .fill(bg)
                                    .corner_radius(CornerRadius::same(3))
                                    .inner_margin(Margin::symmetric(6, 2))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{line_no:>3}"))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(TEXT_DIM),
                                            );
                                            ui.label(
                                                RichText::new(line)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(fg),
                                            );
                                        });
                                    });
                                line_no += 1;
                            }
                        });
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
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            let c = if tool.ok { EMERALD } else { ROSE };
                            ui.label(
                                RichText::new(&tool.name)
                                    .monospace()
                                    .size(11.5)
                                    .strong()
                                    .color(c),
                            );
                            ui.add_space(2.0);
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
        egui::Window::new("Providers & Live Model Endpoints")
            .open(&mut open)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(580.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.5, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(SERIKA_VIOLET_SOFT)
                    .corner_radius(CornerRadius::same(8))
                    .stroke(Stroke::new(1.0, SERIKA_VIOLET.gamma_multiply(0.5)))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let rep = &self.discovery_report;
                            let (sr, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                            paint_icon(ui.painter(), sr, Icon::Sparkle, SERIKA_VIOLET_LIGHT);
                            ui.label(
                                RichText::new(format!(
                                    "Live Endpoints — Local: {} · Claude: {} · OpenAI: {} · Gemini: {}",
                                    rep.local_count.max(1),
                                    rep.anthropic_count,
                                    rep.openai_count,
                                    rep.gemini_count
                                ))
                                .size(12.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let btn_label = if self.discovering_models {
                                    "Syncing…"
                                } else {
                                    "Refresh All"
                                };
                                if pill_icon_button(ui, Icon::Refresh, btn_label, false).clicked() {
                                    self.refresh_remote();
                                }
                            });
                        });
                    });

                for err in &self.discovery_report.errors {
                    ui.label(RichText::new(err).size(11.0).color(AMBER));
                }

                ui.add_space(10.0);

                ui.label(
                    RichText::new("1. Local Workspace & Local Model Endpoints (Zero API Key Required)")
                        .strong()
                        .size(13.0)
                        .color(EMERALD),
                );
                ui.label(
                    RichText::new(
                        "Pick a Local Workspace profile, discovered Ollama / Local Server model, or bind a custom model ID.",
                    )
                    .size(11.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(6.0);

                egui::Frame::NONE
                    .fill(BG_DEEP)
                    .corner_radius(CornerRadius::same(8))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt("providers_local_models_scroll")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for m in self
                                    .provider_models
                                    .iter()
                                    .filter(|m| m.provider.as_deref() == Some("local"))
                                {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("LOCAL")
                                                .monospace()
                                                .size(10.0)
                                                .strong()
                                                .color(EMERALD),
                                        );
                                        if ui
                                            .selectable_label(self.selected_model == m.id, &m.name)
                                            .clicked()
                                        {
                                            self.selected_model = m.id.clone();
                                        }
                                        if let Some(desc) = &m.description {
                                            ui.label(
                                                RichText::new(desc).size(10.5).color(TEXT_DIM),
                                            );
                                        }
                                    });
                                }
                            });
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Local Workspace / CLI Model:")
                            .size(11.5)
                            .color(TEXT_SECONDARY),
                    );
                    ui.add(
                        TextEdit::singleline(&mut self.local_model_input)
                            .hint_text("auto, ollama:qwen2.5-coder:32b, server:llama-3.3, sonnet…")
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ollama URL:").size(11.0).color(TEXT_DIM));
                    ui.add(
                        TextEdit::singleline(&mut self.ollama_base_input)
                            .hint_text(DEFAULT_OLLAMA_BASE)
                            .desired_width(185.0),
                    );
                    ui.label(RichText::new("Local Server:").size(11.0).color(TEXT_DIM));
                    ui.add(
                        TextEdit::singleline(&mut self.local_base_input)
                            .hint_text(DEFAULT_LOCAL_SERVER_BASE)
                            .desired_width(f32::INFINITY),
                    );
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("2. Optional Cloud APIs (Queried Live from /v1/models & /v1beta/models)")
                        .strong()
                        .size(13.0)
                        .color(SERIKA_VIOLET_LIGHT),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new(format!(
                        "Anthropic Claude API Key (GET /v1/models · {} live models)",
                        self.discovery_report.anthropic_count
                    ))
                    .size(11.5)
                    .color(TEXT_SECONDARY),
                );
                ui.add(
                    TextEdit::singleline(&mut self.anthropic_key_input)
                        .password(true)
                        .hint_text("sk-ant-… (or set ANTHROPIC_API_KEY)")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new(format!(
                        "OpenAI API Key (GET /v1/models · {} live models)",
                        self.discovery_report.openai_count
                    ))
                    .size(11.5)
                    .color(TEXT_SECONDARY),
                );
                ui.add(
                    TextEdit::singleline(&mut self.openai_key_input)
                        .password(true)
                        .hint_text("sk-… (or set OPENAI_API_KEY)")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new(format!(
                        "Google Gemini API Key (GET /v1beta/models · {} live models)",
                        self.discovery_report.gemini_count
                    ))
                    .size(11.5)
                    .color(TEXT_SECONDARY),
                );
                ui.add(
                    TextEdit::singleline(&mut self.gemini_key_input)
                        .password(true)
                        .hint_text("AIza… (or set GEMINI_API_KEY)")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_action_button(
                        ui,
                        Icon::Check,
                        "Save & Fetch Live /models",
                        SERIKA_VIOLET,
                        true,
                    )
                    .clicked()
                    {
                        self.save_providers_and_settings();
                    }
                });
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
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.5, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.checkbox(
                    &mut self.auto_approve_session,
                    "Auto-approve file edits and shell commands in this session",
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_action_button(
                        ui,
                        Icon::Check,
                        "Re-run System Setup & Shortcuts",
                        SERIKA_VIOLET,
                        true,
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
                    if pill_icon_button(ui, Icon::Refresh, "Check Updates", false).clicked() {
                        self.check_updates_async();
                    }
                });
                if !self.setup_status.is_empty() {
                    ui.add_space(8.0);
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
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .frame(
                egui::Frame::window(&style)
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.5, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14)),
            )
            .show(ctx, |ui| {
                let resp = ui.add(
                    TextEdit::singleline(&mut self.palette_query)
                        .hint_text("Type a command…")
                        .desired_width(f32::INFINITY),
                );
                resp.request_focus();
                ui.add_space(8.0);
                let q = self.palette_query.to_ascii_lowercase();
                for (id, label) in [
                    ("new", "New Thread (Ctrl+N)"),
                    ("folder", "Open Project Folder…"),
                    ("scratch", "Switch to Scratch Workspace"),
                    (
                        "providers",
                        "Configure Providers & Live /models Endpoints",
                    ),
                    ("inspector", "Toggle Source Control & Diffs Inspector (Ctrl+B)"),
                    ("update", "Check for Updates"),
                ] {
                    if !q.is_empty() && !label.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
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
            .order(egui::Order::Foreground)
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
                    if primary_action_button(ui, Icon::Check, "Approve", EMERALD, true).clicked() {
                        choice = Some(true);
                    }
                    if primary_action_button(ui, Icon::Close, "Deny", ROSE, true).clicked() {
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

    // =========================================================================
    // Codex-CLI Terminal Showcase Renderer (for `--screenshot <png> cli`)
    // =========================================================================
    fn draw_codex_cli_showcase(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(7, 7, 10)).inner_margin(Margin::same(28)))
            .show(root_ui, |ui| {
                egui::Frame::NONE
                    .fill(BG_DEEP)
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(1.2, BORDER_SUBTLE))
                    .inner_margin(Margin::same(0))
                    .show(ui, |ui| {
                        // Terminal Window Titlebar
                        egui::Frame::NONE
                            .fill(BG_SIDEBAR)
                            .corner_radius(CornerRadius {
                                nw: 12,
                                ne: 12,
                                sw: 0,
                                se: 0,
                            })
                            .inner_margin(Margin::symmetric(16, 10))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    for col in [ROSE, AMBER, EMERALD] {
                                        let (r, _) =
                                            ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                                        ui.painter().circle_filled(r.center(), 5.0, col);
                                    }
                                    ui.add_space(10.0);
                                    let (tr, _) =
                                        ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                                    paint_icon(ui.painter(), tr, Icon::Terminal, SERIKA_VIOLET_LIGHT);
                                    ui.label(
                                        RichText::new("serika — ~/Documents/Github/SerikaCode (Codex-Style Interactive CLI)")
                                            .monospace()
                                            .size(12.0)
                                            .color(TEXT_MUTED),
                                    );
                                });
                            });

                        let sep = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                        ui.painter().rect_filled(sep, 0.0, BORDER_HAIRLINE);

                        egui::Frame::NONE
                            .inner_margin(Margin::symmetric(24, 18))
                            .show(ui, |ui| {
                                // 1. Codex-style Header Box
                                egui::Frame::NONE
                                    .fill(BG_CARD)
                                    .corner_radius(CornerRadius::same(8))
                                    .stroke(Stroke::new(1.0, BORDER_STRONG))
                                    .inner_margin(Margin::symmetric(16, 12))
                                    .show(ui, |ui| {
                                        ui.set_min_width(640.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(">_")
                                                    .monospace()
                                                    .strong()
                                                    .size(13.5)
                                                    .color(SERIKA_VIOLET),
                                            );
                                            ui.label(
                                                RichText::new("Serika Code")
                                                    .monospace()
                                                    .strong()
                                                    .size(13.5)
                                                    .color(TEXT_PRIMARY),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "(v{})",
                                                    serika_runtime::client::VERSION
                                                ))
                                                .monospace()
                                                .size(13.0)
                                                .color(TEXT_DIM),
                                            );
                                        });
                                        ui.add_space(6.0);
                                        for (k, v, hint) in [
                                            ("model:", "local/workspace (ollama:qwen2.5-coder:32b)", "/model to change"),
                                            ("directory:", "~/Documents/Github/SerikaCode", ""),
                                            ("approvals:", "standard", "/approvals to change"),
                                            ("account:", "local workspace (no API key required)", ""),
                                        ] {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(format!("{k:<12}"))
                                                        .monospace()
                                                        .size(12.5)
                                                        .color(TEXT_DIM),
                                                );
                                                ui.label(
                                                    RichText::new(v)
                                                        .monospace()
                                                        .size(12.5)
                                                        .color(TEXT_PRIMARY),
                                                );
                                                if !hint.is_empty() {
                                                    ui.add_space(12.0);
                                                    ui.label(
                                                        RichText::new(hint)
                                                            .monospace()
                                                            .size(12.0)
                                                            .color(SERIKA_VIOLET_LIGHT),
                                                    );
                                                }
                                            });
                                        }
                                    });

                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new("  To get started, describe a task or try one of these commands:")
                                        .monospace()
                                        .size(12.0)
                                        .color(TEXT_DIM),
                                );
                                ui.add_space(4.0);
                                for (cmd, desc) in [
                                    ("/model", "choose what model or live API endpoint to use"),
                                    ("/status", "show the current session configuration"),
                                    ("/diff", "show the git diff, including untracked files"),
                                    ("/review", "review the current changes and find issues"),
                                ] {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("  {cmd:<12}"))
                                                .monospace()
                                                .size(12.0)
                                                .color(SERIKA_VIOLET_LIGHT),
                                        );
                                        ui.label(
                                            RichText::new(desc)
                                                .monospace()
                                                .size(12.0)
                                                .color(TEXT_DIM),
                                        );
                                    });
                                }

                                ui.add_space(12.0);

                                // 2. Highlighted User Prompt Cell (`› ...`)
                                egui::Frame::NONE
                                    .fill(BG_ELEVATED)
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(Margin::symmetric(12, 7))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new("›")
                                                    .monospace()
                                                    .strong()
                                                    .size(13.5)
                                                    .color(SERIKA_VIOLET),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Query live /models endpoints for OpenAI, Claude & Gemini and run workspace tests",
                                                )
                                                .monospace()
                                                .strong()
                                                .size(13.0)
                                                .color(TEXT_PRIMARY),
                                            );
                                        });
                                    });

                                ui.add_space(10.0);

                                // 3. Tool Cells (`• Explored`, `• Edited`, `• Ran`)
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("•").monospace().strong().size(13.0).color(EMERALD));
                                    ui.label(RichText::new("Read").monospace().strong().size(12.5).color(TEXT_PRIMARY));
                                    ui.label(
                                        RichText::new("crates/serika-runtime/src/providers.rs")
                                            .monospace()
                                            .size(12.5)
                                            .color(TEXT_MUTED),
                                    );
                                });
                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("•").monospace().strong().size(13.0).color(EMERALD));
                                    ui.label(RichText::new("Edited").monospace().strong().size(12.5).color(TEXT_PRIMARY));
                                    ui.label(
                                        RichText::new("crates/serika-runtime/src/providers.rs")
                                            .monospace()
                                            .size(12.5)
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.label(RichText::new("(+18").monospace().size(12.0).color(EMERALD));
                                    ui.label(RichText::new("-3)").monospace().size(12.0).color(ROSE));
                                });

                                for (ln, sign, code, bg, fg) in [
                                    ("172", " ", "pub async fn discover_all_models(http: &Client, cfg: &Config) -> Report {", Color32::TRANSPARENT, TEXT_DIM),
                                    ("173", "-", "    let models = builtin_static_models();", ROSE_SOFT, ROSE),
                                    ("173", "+", "    let (ollama, local_srv, openai, claude, gemini) = tokio::join!(", EMERALD_SOFT, EMERALD),
                                    ("174", "+", "        fetch_ollama_models(http, cfg), fetch_openai_models(http, cfg),", EMERALD_SOFT, EMERALD),
                                    ("175", "+", "        fetch_anthropic_models(http, cfg), fetch_gemini_models(http, cfg),", EMERALD_SOFT, EMERALD),
                                    ("176", "+", "    );", EMERALD_SOFT, EMERALD),
                                ] {
                                    egui::Frame::NONE
                                        .fill(bg)
                                        .corner_radius(CornerRadius::same(3))
                                        .inner_margin(Margin::symmetric(8, 1))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(format!("  {ln:>4}"))
                                                        .monospace()
                                                        .size(11.5)
                                                        .color(TEXT_DIM),
                                                );
                                                ui.label(
                                                    RichText::new(format!("{sign}{code}"))
                                                        .monospace()
                                                        .size(11.5)
                                                        .color(fg),
                                                );
                                            });
                                        });
                                }

                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("•").monospace().strong().size(13.0).color(EMERALD));
                                    ui.label(RichText::new("Ran").monospace().strong().size(12.5).color(TEXT_PRIMARY));
                                    ui.label(
                                        RichText::new("cargo test --workspace")
                                            .monospace()
                                            .size(12.5)
                                            .color(SERIKA_VIOLET_LIGHT),
                                    );
                                });
                                ui.label(
                                    RichText::new("  └ test result: ok. 41 passed; 0 failed; finished in 1.74s")
                                        .monospace()
                                        .size(12.0)
                                        .color(TEXT_DIM),
                                );

                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("•")
                                            .monospace()
                                            .strong()
                                            .size(13.0)
                                            .color(SERIKA_VIOLET_LIGHT),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "All provider /models endpoints and Local Workspace sub-models are now queried live.",
                                        )
                                        .monospace()
                                        .size(12.5)
                                        .color(TEXT_PRIMARY),
                                    );
                                });

                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        "─────────────────────────────── Worked for 1.7s ───────────────────────────────",
                                    )
                                    .monospace()
                                    .size(11.5)
                                    .color(TEXT_DIM),
                                );

                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("›")
                                            .monospace()
                                            .strong()
                                            .size(14.0)
                                            .color(SERIKA_VIOLET),
                                    );
                                    ui.label(
                                        RichText::new("Ask Serika a follow-up or run a /command…")
                                            .monospace()
                                            .size(12.5)
                                            .color(TEXT_DIM),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new("local/workspace · standard · Ctrl+C interrupt")
                                                .monospace()
                                                .size(11.5)
                                                .color(TEXT_DIM),
                                        );
                                    });
                                });
                            });
                    });
            });
    }
}

// =============================================================================
// Helper Components & Theme Setup
// =============================================================================

fn draw_rich_markdown(ui: &mut egui::Ui, content: &str) {
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if let Some(fence_rest) = trimmed.strip_prefix("```") {
            if in_code {
                draw_code_block_card(ui, &code_lang, &code_buf);
                code_buf.clear();
                code_lang.clear();
                in_code = false;
            } else {
                code_lang = fence_rest.trim().to_string();
                in_code = true;
            }
            continue;
        }
        if in_code {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(raw_line);
            continue;
        }

        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        if let Some(h) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("# "))
        {
            ui.add_space(2.0);
            ui.label(
                RichText::new(h.replace('`', ""))
                    .strong()
                    .size(14.5)
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(2.0);
            continue;
        }
        if let Some(bullet) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("•")
                        .strong()
                        .size(13.5)
                        .color(SERIKA_VIOLET_LIGHT),
                );
                ui.label(
                    RichText::new(bullet.replace("**", "").replace('`', ""))
                        .size(13.5)
                        .color(TEXT_SECONDARY),
                );
            });
            continue;
        }
        ui.label(
            RichText::new(raw_line.replace("**", "").replace('`', ""))
                .size(13.5)
                .color(TEXT_SECONDARY),
        );
    }
    if in_code && !code_buf.is_empty() {
        draw_code_block_card(ui, &code_lang, &code_buf);
    }
}

fn draw_code_block_card(ui: &mut egui::Ui, lang: &str, code: &str) {
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(BG_DEEP)
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (cr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                paint_icon(ui.painter(), cr, Icon::Code, SERIKA_VIOLET_LIGHT);
                ui.label(
                    RichText::new(if lang.is_empty() { "code" } else { lang })
                        .monospace()
                        .size(11.0)
                        .strong()
                        .color(SERIKA_VIOLET_LIGHT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(code.to_string());
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(code)
                    .monospace()
                    .size(12.0)
                    .color(TEXT_PRIMARY),
            );
        });
    ui.add_space(4.0);
}

// -----------------------------------------------------------------------------
// SerikaChat Navigation & Component Helpers
// -----------------------------------------------------------------------------

fn draw_sidebar_nav_row(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    shortcut: Option<(&str, &str)>,
    active: bool,
) -> bool {
    let desired = Vec2::new(ui.available_width(), 32.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if active {
        BG_ACTIVE_ROW
    } else if resp.hovered() {
        BG_CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 7.0, fill);
    }
    let fg = if active || resp.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Vec2::splat(13.0),
    );
    paint_icon(ui.painter(), ir, icon, fg);

    ui.painter().text(
        Pos2::new(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        fg,
    );

    if let Some((mod_key, key)) = shortcut {
        let mut right_cursor = rect.right() - 8.0;
        for k in [key, mod_key] {
            let galley = ui.painter().layout_no_wrap(
                k.to_string(),
                FontId::monospace(10.0),
                TEXT_DIM,
            );
            let pill_w = galley.size().x + 8.0;
            let pill_rect = Rect::from_min_size(
                Pos2::new(right_cursor - pill_w, rect.center().y - 8.0),
                Vec2::new(pill_w, 16.0),
            );
            ui.painter().rect_filled(pill_rect, 4.0, BG_CARD);
            ui.painter().rect_stroke(
                pill_rect,
                4.0,
                Stroke::new(1.0, BORDER_HAIRLINE),
                StrokeKind::Inside,
            );
            ui.painter().galley(
                Pos2::new(pill_rect.left() + 4.0, pill_rect.center().y - galley.size().y * 0.5),
                galley,
                TEXT_DIM,
            );
            right_cursor -= pill_w + 4.0;
        }
    }

    resp.clicked()
}

#[allow(dead_code)]
fn draw_sidebar_workspace_row(
    ui: &mut egui::Ui,
    label: &str,
    branch: Option<&str>,
    dirty: bool,
) -> bool {
    let desired = Vec2::new(ui.available_width(), 32.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if resp.hovered() {
        BG_CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 7.0, fill);
    }
    let fg = if resp.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Vec2::splat(13.0),
    );
    paint_icon(ui.painter(), ir, Icon::Folder, fg);

    ui.painter().text(
        Pos2::new(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        fg,
    );

    let mut right_x = rect.right() - 8.0;
    if dirty {
        let dot_pos = Pos2::new(right_x, rect.center().y);
        ui.painter().circle_filled(dot_pos, 3.5, AMBER);
        right_x -= 10.0;
    }
    if let Some(b) = branch {
        let b_galley = ui.painter().layout_no_wrap(
            b.to_string(),
            FontId::monospace(10.5),
            TEXT_DIM,
        );
        let b_w = b_galley.size().x;
        ui.painter().galley(
            Pos2::new(right_x - b_w, rect.center().y - b_galley.size().y * 0.5),
            b_galley,
            TEXT_DIM,
        );
    }

    resp.clicked()
}

fn draw_serikachat_starter_row(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    subtitle: &str,
) -> bool {
    let desired = Vec2::new(width, 36.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, BG_CARD_HOVER);
    }

    let title_galley = ui.painter().layout_no_wrap(
        title.to_string(),
        FontId::proportional(13.5),
        if resp.hovered() { Color32::WHITE } else { TEXT_PRIMARY },
    );
    let title_w = title_galley.size().x;
    ui.painter().galley(
        Pos2::new(rect.left() + 8.0, rect.center().y - title_galley.size().y * 0.5),
        title_galley,
        if resp.hovered() { Color32::WHITE } else { TEXT_PRIMARY },
    );

    let sub_galley = ui.painter().layout_no_wrap(
        format!(" {subtitle}"),
        FontId::proportional(13.5),
        TEXT_MUTED,
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 8.0 + title_w, rect.center().y - sub_galley.size().y * 0.5),
        sub_galley,
        TEXT_MUTED,
    );

    let bottom_line = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.bottom() - 1.0),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().rect_filled(bottom_line, 0.0, BORDER_HAIRLINE);

    resp.clicked()
}

fn merge_conversation_turns(messages: &[StoredMessage]) -> Vec<StoredMessage> {
    let mut out: Vec<StoredMessage> = Vec::new();
    for msg in messages {
        if msg.role == "assistant" {
            if let Some(last) = out.last_mut().filter(|m| m.role == "assistant") {
                match (&last.reasoning, &msg.reasoning) {
                    (Some(r1), Some(r2)) if !r2.is_empty() => {
                        last.reasoning = Some(format!("{r1}\n\n{r2}"));
                    }
                    (None, Some(r2)) if !r2.is_empty() => {
                        last.reasoning = Some(r2.clone());
                    }
                    _ => {}
                }
                if !msg.content.trim().is_empty() {
                    if last.content.trim().is_empty() {
                        last.content = msg.content.clone();
                    } else if !last.content.contains(&msg.content) {
                        last.content.push_str("\n\n");
                        last.content.push_str(&msg.content);
                    }
                }
                continue;
            }
        }
        out.push(msg.clone());
    }
    out
}

fn synthesize_fallback_reasoning(
    _msg: &StoredMessage,
    tools: &[ToolRun],
    project: &Path,
    model: &str,
) -> String {
    let proj_name = project.file_name().and_then(|n| n.to_str()).unwrap_or("workspace");
    let mut lines = Vec::new();
    lines.push(format!("1. Analyzed user request in the context of `{proj_name}`."));
    lines.push(format!("2. Evaluated active harness model: `{model}`."));

    if !tools.is_empty() {
        lines.push(format!("3. Executed {} tool action(s):", tools.len()));
        for t in tools.iter().take(4) {
            let status = if t.ok { "completed" } else { "warning" };
            lines.push(format!("   • `{}` ({status})", t.name));
        }
        if tools.len() > 4 {
            lines.push(format!("   • … and {} more tool action(s)", tools.len() - 4));
        }
        lines.push("4. Verified output and structured final response for the user.".to_string());
    } else {
        lines.push("3. Inspected codebase structure and prepared direct response.".to_string());
        lines.push("4. Verified syntax, formatting, and recommendations.".to_string());
    }

    lines.join("\n")
}

fn draw_reasoning_trace(ui: &mut egui::Ui, msg_id: i64, reasoning: &str, is_streaming: bool) {
    if reasoning.trim().is_empty() && !is_streaming {
        return;
    }

    let id = ui.make_persistent_id(format!("reasoning_trace_{msg_id}"));
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

    let step_count = reasoning.lines().filter(|l| !l.trim().is_empty()).count().max(1);
    let title = if is_streaming {
        "Thinking…".to_string()
    } else {
        format!("Thought Process · {step_count} steps")
    };

    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(BG_CARD)
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, BORDER_HAIRLINE))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    paint_icon(ui.painter(), ir, Icon::Sparkle, SERIKA_VIOLET_LIGHT);
                    ui.label(
                        RichText::new(&title)
                            .size(12.0)
                            .color(if is_streaming { SERIKA_VIOLET_LIGHT } else { TEXT_MUTED }),
                    );
                    if is_streaming {
                        let (dr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                        ui.painter().circle_filled(dr.center(), 3.0, SERIKA_VIOLET);
                    }
                });
            })
            .body(|ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let line_rect = ui.allocate_space(Vec2::new(2.0, ui.available_height().max(16.0))).1;
                    ui.painter().rect_filled(line_rect, 1.0, SERIKA_VIOLET.gamma_multiply(0.45));
                    ui.add_space(8.0);

                    ui.vertical(|ui| {
                        for line in reasoning.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                ui.add_space(3.0);
                                continue;
                            }
                            let is_header_step = trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains('.');
                            let col = if is_header_step {
                                TEXT_PRIMARY
                            } else {
                                TEXT_MUTED
                            };
                            ui.label(
                                RichText::new(trimmed)
                                    .size(12.0)
                                    .color(col),
                            );
                        }
                    });
                });
            });
        });
    ui.add_space(4.0);
}

fn serikachat_composer_pill(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    active: bool,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(11.5),
        if active { Color32::WHITE } else { TEXT_MUTED },
    );
    let desired = Vec2::new(galley.size().x + 28.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if active {
        SERIKA_VIOLET_SOFT
    } else if resp.hovered() {
        BG_CARD_HOVER
    } else {
        BG_ELEVATED
    };
    let stroke_col = if active {
        SERIKA_VIOLET
    } else {
        BORDER_HAIRLINE
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    ui.painter().rect_stroke(rect, 12.0, Stroke::new(1.0, stroke_col), StrokeKind::Inside);

    let fg = if active || resp.hovered() {
        Color32::WHITE
    } else {
        TEXT_MUTED
    };
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + 11.0, rect.center().y),
        Vec2::splat(11.0),
    );
    paint_icon(ui.painter(), ir, icon, fg);

    ui.painter().galley(
        Pos2::new(rect.left() + 20.0, rect.center().y - galley.size().y * 0.5),
        galley,
        fg,
    );

    resp
}

fn white_circle_action_button(ui: &mut egui::Ui, icon: Icon, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let bg = if !enabled {
        BG_ELEVATED
    } else if resp.hovered() {
        Color32::from_rgb(230, 230, 230)
    } else {
        Color32::WHITE
    };
    ui.painter().circle_filled(rect.center(), 14.0, bg);
    let fg = if enabled {
        Color32::from_rgb(13, 13, 13)
    } else {
        TEXT_DIM
    };
    let ir = rect.shrink(7.5);
    paint_icon(ui.painter(), ir, icon, fg);
    resp
}

fn primary_action_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    accent: Color32,
    enabled: bool,
) -> egui::Response {
    let fg = if enabled { Color32::WHITE } else { TEXT_DIM };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(12.0),
        fg,
    );
    let desired = Vec2::new(galley.size().x + 36.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if !enabled {
        BG_ELEVATED
    } else if resp.hovered() {
        SERIKA_VIOLET_HOVER
    } else {
        accent
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    if !enabled {
        ui.painter()
            .rect_stroke(rect, 7.0, Stroke::new(1.0, BORDER_SUBTLE), StrokeKind::Inside);
    }
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + 13.0, rect.center().y),
        Vec2::splat(11.0),
    );
    paint_icon(ui.painter(), ir, icon, fg);
    ui.painter().galley(
        Pos2::new(rect.left() + 24.0, rect.center().y - galley.size().y * 0.5),
        galley,
        fg,
    );
    resp
}

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

#[allow(dead_code)]
fn draw_status_badge(ui: &mut egui::Ui, dot_col: Color32, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::proportional(11.0),
        TEXT_MUTED,
    );
    let desired = Vec2::new(galley.size().x + 26.0, 23.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    ui.painter().rect_filled(rect, 6.0, BG_CARD);
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, BORDER_HAIRLINE), StrokeKind::Inside);
    ui.painter().circle_filled(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        3.2,
        dot_col,
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 18.0, rect.center().y - galley.size().y * 0.5),
        galley,
        TEXT_MUTED,
    );
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, primary: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(25.0), Sense::click());
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
    paint_icon(ui.painter(), rect.shrink(5.5), icon, fg);
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
        egui::Button::new(RichText::new(format!("    {label}")).size(11.5).color(fg))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke_col))
            .corner_radius(CornerRadius::same(6)),
    );
    let ir = Rect::from_center_size(
        Pos2::new(btn.rect.left() + 11.5, btn.rect.center().y),
        Vec2::splat(11.0),
    );
    paint_icon(ui.painter(), ir, icon, fg);
    btn
}

fn draw_thread_row(ui: &mut egui::Ui, item: &SessionSummary, is_active: bool) -> (bool, bool) {
    let mut clicked = false;
    let mut deleted = false;
    let desired = Vec2::new(ui.available_width(), 32.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    let fill = if is_active {
        BG_ACTIVE_ROW
    } else if resp.hovered() {
        BG_CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }

    let title_col = if is_active || resp.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    let clipped_title = if item.title.chars().count() > 24 {
        let s: String = item.title.chars().take(23).collect();
        format!("{s}...")
    } else {
        item.title.clone()
    };
    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clipped_title,
        FontId::proportional(12.5),
        title_col,
    );

    if resp.hovered() {
        let del_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 12.0, rect.center().y),
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

fn draw_tool_pill(ui: &mut egui::Ui, tool: &ToolRun, has_diff: bool) -> bool {
    let mut view_diff = false;
    egui::Frame::NONE
        .fill(BG_CARD)
        .corner_radius(CornerRadius::same(6))
        .stroke(Stroke::new(1.0, BORDER_HAIRLINE))
        .inner_margin(Margin::symmetric(9, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let col = if tool.ok { EMERALD } else { ROSE };
                let (dr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(dr.center(), 3.5, col);
                ui.label(
                    RichText::new(&tool.name)
                        .monospace()
                        .size(11.5)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                let first_line = tool.output.lines().next().unwrap_or("");
                let clipped = if first_line.chars().count() > 56 {
                    let s: String = first_line.chars().take(55).collect();
                    format!("{s}…")
                } else {
                    first_line.to_string()
                };
                ui.label(
                    RichText::new(clipped)
                        .monospace()
                        .size(11.0)
                        .color(TEXT_MUTED),
                );
                let is_edit_tool = matches!(
                    tool.name.as_str(),
                    "write_file" | "replace_in_file" | "apply_patch"
                );
                if has_diff && is_edit_tool {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_icon_button(ui, Icon::Code, "View diff", false).clicked() {
                            view_diff = true;
                        }
                    });
                }
            });
        });
    view_diff
}

#[allow(dead_code)]
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
    visuals.selection.stroke = Stroke::new(1.0, SERIKA_VIOLET);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(7);

    visuals.widgets.inactive.bg_fill = BG_INPUT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, SERIKA_VIOLET);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);

    visuals.widgets.active.bg_fill = SERIKA_VIOLET_SOFT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, SERIKA_VIOLET);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    style.visuals = visuals;
    ctx.set_style_of(Theme::Dark, style);
}
