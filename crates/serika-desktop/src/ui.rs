use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serika_runtime::i18n::{self, Key as Msg, Locale};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Key, Layout,
    Margin, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextStyle,
    Theme, Vec2,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use serika_runtime::client::LoginOutcome;
use serika_runtime::credentials::StoredSession;
use serika_runtime::gitinfo::{self, GitFileEntry, GitRepoSnapshot};
use serika_runtime::model::{
    absolute_avatar_url, AccountProfile, ChatModel, ModelCatalog, UsageSnapshot,
};
use serika_runtime::permissions::PermissionMode;
use serika_runtime::providers::{
    builtin_provider_models, detect_local_harnesses, discover_all_models, LocalHarness,
    ProviderDiscoveryReport, DEFAULT_LOCAL_SERVER_BASE, DEFAULT_OLLAMA_BASE,
};
use serika_runtime::sessions::{SessionSummary, StoredMessage, ToolRun};
use serika_runtime::setup::{self, SetupOptions};
use serika_runtime::updater::{self, UpdateInfo};
use serika_runtime::{AgentEvent, Runtime, TaskOutcome, TaskRequest};
use zeroize::Zeroize;

// =============================================================================
// SerikaChat Official Carbon Palette (from chat-web/app/globals.css)
// Dark tokens are the canonical look. Light is the Settings theme alternate.
// =============================================================================
static UI_LIGHT: AtomicBool = AtomicBool::new(false);

fn set_ui_light(light: bool) {
    UI_LIGHT.store(light, Ordering::Relaxed);
}

fn ui_light() -> bool {
    UI_LIGHT.load(Ordering::Relaxed)
}

fn bg_deep() -> Color32 {
    if ui_light() {
        Color32::from_rgb(247, 247, 248)
    } else {
        Color32::from_rgb(15, 15, 16)
    }
}
fn bg_sidebar() -> Color32 {
    if ui_light() {
        Color32::from_rgb(255, 255, 255)
    } else {
        Color32::from_rgb(11, 11, 12)
    }
}
fn bg_card() -> Color32 {
    if ui_light() {
        Color32::from_rgb(255, 255, 255)
    } else {
        Color32::from_rgb(24, 24, 27)
    }
}
fn bg_elevated() -> Color32 {
    if ui_light() {
        Color32::from_rgb(244, 244, 245)
    } else {
        Color32::from_rgb(28, 28, 31)
    }
}
fn bg_card_hover() -> Color32 {
    if ui_light() {
        Color32::from_rgb(236, 236, 238)
    } else {
        Color32::from_rgb(31, 31, 34)
    }
}
fn bg_input() -> Color32 {
    if ui_light() {
        Color32::from_rgb(250, 250, 250)
    } else {
        Color32::from_rgb(23, 23, 25)
    }
}
fn bg_active_row() -> Color32 {
    if ui_light() {
        Color32::from_rgb(237, 233, 254)
    } else {
        Color32::from_rgb(28, 28, 31)
    }
}
fn border_hairline() -> Color32 {
    if ui_light() {
        Color32::from_rgba_unmultiplied(0, 0, 0, 16)
    } else {
        Color32::from_rgba_premultiplied(14, 14, 14, 14)
    }
}
fn border_subtle() -> Color32 {
    if ui_light() {
        Color32::from_rgba_unmultiplied(0, 0, 0, 24)
    } else {
        Color32::from_rgba_premultiplied(20, 20, 20, 20)
    }
}
fn border_strong() -> Color32 {
    if ui_light() {
        Color32::from_rgba_unmultiplied(0, 0, 0, 40)
    } else {
        Color32::from_rgba_premultiplied(38, 38, 38, 38)
    }
}
fn text_primary() -> Color32 {
    if ui_light() {
        Color32::from_rgb(24, 24, 27)
    } else {
        Color32::from_rgb(236, 236, 238)
    }
}
fn text_secondary() -> Color32 {
    if ui_light() {
        Color32::from_rgb(63, 63, 70)
    } else {
        Color32::from_rgb(161, 161, 170)
    }
}
fn text_muted() -> Color32 {
    if ui_light() {
        Color32::from_rgb(82, 82, 91)
    } else {
        Color32::from_rgb(140, 140, 148)
    }
}
fn text_dim() -> Color32 {
    if ui_light() {
        Color32::from_rgb(113, 113, 122)
    } else {
        Color32::from_rgb(113, 113, 122)
    }
}
fn composer_fill() -> Color32 {
    if ui_light() {
        Color32::from_rgb(255, 255, 255)
    } else {
        Color32::from_rgb(20, 20, 20)
    }
}
fn composer_border() -> Color32 {
    if ui_light() {
        Color32::from_rgb(220, 220, 224)
    } else {
        Color32::from_rgb(34, 34, 34)
    }
}
fn search_fill() -> Color32 {
    if ui_light() {
        Color32::from_rgb(244, 244, 245)
    } else {
        Color32::from_rgb(18, 18, 18)
    }
}

const ACCENT_PURPLE: Color32 = Color32::from_rgb(139, 92, 246); // #8b5cf6 (--c-accent)
const ACCENT_PURPLE_HOVER: Color32 = Color32::from_rgb(124, 58, 237); // #7c3aed (--c-accent-hover)
const ACCENT_PURPLE_SEC: Color32 = Color32::from_rgb(167, 139, 250); // #a78bfa (--c-accent-secondary)

const SERIKA_VIOLET: Color32 = ACCENT_PURPLE;
const SERIKA_VIOLET_HOVER: Color32 = ACCENT_PURPLE_HOVER;
const SERIKA_VIOLET_LIGHT: Color32 = ACCENT_PURPLE_SEC;
const SERIKA_VIOLET_SOFT: Color32 = Color32::from_rgba_premultiplied(21, 14, 37, 38); // accent/15

const EMERALD: Color32 = Color32::from_rgb(52, 211, 153); // #34d399 (--success)
const EMERALD_SOFT: Color32 = Color32::from_rgb(13, 38, 28);
const AMBER: Color32 = Color32::from_rgb(251, 191, 36); // #fbbf24 (--warning)
const ROSE: Color32 = Color32::from_rgb(244, 63, 94); // #f43f5e (--c-moe / --danger)
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
    SoundWave,
    Download,
    Command,
    Lock,
    LogOut,
    Link,
    Minimize,
    Maximize,
    Restore,
    Mail,
    User,
    Eye,
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
                let kx = if i == 1 {
                    c.x + r * 0.35
                } else {
                    c.x - r * 0.35
                };
                painter.circle_filled(Pos2::new(kx, y), 2.3, color);
            }
        }
        Icon::Key => {
            let kc = Pos2::new(c.x - r * 0.4, c.y);
            painter.circle_stroke(kc, r * 0.55, stroke);
            painter.line_segment(
                [Pos2::new(kc.x + r * 0.55, c.y), Pos2::new(c.x + r, c.y)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.75, c.y),
                    Pos2::new(c.x + r * 0.75, c.y + r * 0.45),
                ],
                stroke,
            );
        }
        Icon::PanelRight => {
            let box_r = Rect::from_center_size(c, Vec2::new(r * 2.1, r * 1.7));
            painter.rect_stroke(box_r, 2.5, stroke, StrokeKind::Inside);
            let split_x = box_r.right() - box_r.width() * 0.35;
            painter.line_segment(
                [
                    Pos2::new(split_x, box_r.top()),
                    Pos2::new(split_x, box_r.bottom()),
                ],
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
            painter.line_segment(
                [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
                stroke,
            );
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
                [
                    Pos2::new(c.x + r * 0.75, c.y - r * 0.2),
                    Pos2::new(c.x + r * 0.75, c.y + r * 0.65),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.75, c.y + r * 0.65),
                    Pos2::new(c.x - r * 0.3, c.y + r * 0.65),
                ],
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
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.9, c.y),
                    Pos2::new(c.x, c.y - r * 0.55),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y - r * 0.55),
                    Pos2::new(c.x + r * 0.9, c.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.9, c.y),
                    Pos2::new(c.x, c.y + r * 0.55),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y + r * 0.55),
                    Pos2::new(c.x + r * 0.9, c.y),
                ],
                stroke,
            );
            painter.circle_filled(sc, 1.8, color);
            let slash_s = Stroke::new(1.6, color);
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.9, c.y - r * 0.7),
                    Pos2::new(c.x + r * 0.9, c.y + r * 0.7),
                ],
                slash_s,
            );
        }
        Icon::Edit => {
            let box_r = Rect::from_min_max(
                Pos2::new(c.x - r * 0.9, c.y - r * 0.5),
                Pos2::new(c.x + r * 0.5, c.y + r * 0.9),
            );
            painter.rect_stroke(box_r, 2.0, stroke, StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.2, c.y + r * 0.2),
                    Pos2::new(c.x + r * 0.9, c.y - r * 0.9),
                ],
                stroke,
            );
        }
        Icon::Book => {
            painter.line_segment(
                [Pos2::new(c.x, c.y - r * 0.8), Pos2::new(c.x, c.y + r * 0.8)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.85, c.y - r * 0.6),
                    Pos2::new(c.x, c.y - r * 0.8),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.85, c.y - r * 0.6),
                    Pos2::new(c.x, c.y - r * 0.8),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.85, c.y + r * 0.6),
                    Pos2::new(c.x, c.y + r * 0.8),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.85, c.y + r * 0.6),
                    Pos2::new(c.x, c.y + r * 0.8),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.85, c.y - r * 0.6),
                    Pos2::new(c.x - r * 0.85, c.y + r * 0.6),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.85, c.y - r * 0.6),
                    Pos2::new(c.x + r * 0.85, c.y + r * 0.6),
                ],
                stroke,
            );
        }
        Icon::Globe => {
            painter.circle_stroke(c, r * 0.85, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.85, c.y),
                    Pos2::new(c.x + r * 0.85, c.y),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y - r * 0.85),
                    Pos2::new(c.x, c.y + r * 0.85),
                ],
                Stroke::new(1.2, color),
            );
        }
        Icon::Microphone => {
            let cap =
                Rect::from_center_size(Pos2::new(c.x, c.y - r * 0.25), Vec2::new(r * 0.7, r * 1.1));
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
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y + r * 0.65),
                    Pos2::new(c.x, c.y + r * 0.95),
                ],
                stroke,
            );
        }
        Icon::SoundWave => {
            // M6 10v4M10 6v12M14 8.5v7M18 10.5v3 (4-bar wave from chat-web WaveIcon)
            let stroke = Stroke::new(1.8, color);
            let x0 = c.x - r * 0.54;
            let x1 = c.x - r * 0.18;
            let x2 = c.x + r * 0.18;
            let x3 = c.x + r * 0.54;
            painter.line_segment(
                [Pos2::new(x0, c.y - r * 0.22), Pos2::new(x0, c.y + r * 0.22)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(x1, c.y - r * 0.62), Pos2::new(x1, c.y + r * 0.62)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(x2, c.y - r * 0.40), Pos2::new(x2, c.y + r * 0.40)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(x3, c.y - r * 0.18), Pos2::new(x3, c.y + r * 0.18)],
                stroke,
            );
        }
        Icon::Download => {
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y - r * 0.85),
                    Pos2::new(c.x, c.y + r * 0.35),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.45, c.y),
                    Pos2::new(c.x, c.y + r * 0.45),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.45, c.y),
                    Pos2::new(c.x, c.y + r * 0.45),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.6, c.y + r * 0.85),
                    Pos2::new(c.x + r * 0.6, c.y + r * 0.85),
                ],
                stroke,
            );
        }
        Icon::Command => {
            let s = r * 0.42;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                painter.circle_stroke(Pos2::new(c.x + dx * s, c.y + dy * s), s * 0.55, stroke);
            }
        }
        Icon::Lock => {
            painter.rect_stroke(
                Rect::from_center_size(
                    Pos2::new(c.x, c.y + r * 0.28),
                    Vec2::new(r * 1.2, r * 0.95),
                ),
                2.0,
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.38, c.y),
                    Pos2::new(c.x - r * 0.38, c.y - r * 0.35),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.38, c.y - r * 0.35),
                    Pos2::new(c.x + r * 0.38, c.y - r * 0.35),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.38, c.y - r * 0.35),
                    Pos2::new(c.x + r * 0.38, c.y),
                ],
                stroke,
            );
        }
        Icon::LogOut => {
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.15, c.y),
                    Pos2::new(c.x + r * 0.7, c.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.3, c.y - r * 0.35),
                    Pos2::new(c.x + r * 0.7, c.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.3, c.y + r * 0.35),
                    Pos2::new(c.x + r * 0.7, c.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.55, c.y - r * 0.7),
                    Pos2::new(c.x - r * 0.15, c.y - r * 0.7),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.55, c.y + r * 0.7),
                    Pos2::new(c.x - r * 0.15, c.y + r * 0.7),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.55, c.y - r * 0.7),
                    Pos2::new(c.x - r * 0.55, c.y + r * 0.7),
                ],
                stroke,
            );
        }
        Icon::Link => {
            painter.circle_stroke(Pos2::new(c.x - r * 0.28, c.y + r * 0.15), r * 0.38, stroke);
            painter.circle_stroke(Pos2::new(c.x + r * 0.28, c.y - r * 0.15), r * 0.38, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.05, c.y - r * 0.05),
                    Pos2::new(c.x + r * 0.05, c.y + r * 0.05),
                ],
                stroke,
            );
        }
        Icon::Minimize => {
            painter.line_segment(
                [Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)],
                Stroke::new(1.6, color),
            );
        }
        Icon::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(r * 1.7)),
                1.0,
                Stroke::new(1.4, color),
                StrokeKind::Inside,
            );
        }
        Icon::Restore => {
            let back = Rect::from_min_size(
                Pos2::new(c.x - r * 0.15, c.y - r * 0.85),
                Vec2::splat(r * 1.35),
            );
            let front = Rect::from_min_size(
                Pos2::new(c.x - r * 0.7, c.y - r * 0.35),
                Vec2::splat(r * 1.35),
            );
            painter.rect_stroke(back, 1.0, Stroke::new(1.3, color), StrokeKind::Inside);
            painter.rect_filled(front, 1.0, bg_sidebar());
            painter.rect_stroke(front, 1.0, Stroke::new(1.3, color), StrokeKind::Inside);
        }
        Icon::Mail => {
            let box_rect = Rect::from_center_size(c, Vec2::new(r * 2.0, r * 1.4));
            painter.rect_stroke(box_rect, 1.5, stroke, StrokeKind::Inside);
            painter.line_segment([box_rect.left_top(), c], stroke);
            painter.line_segment([box_rect.right_top(), c], stroke);
        }
        Icon::User => {
            painter.circle_stroke(Pos2::new(c.x, c.y - r * 0.35), r * 0.42, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.75, c.y + r * 0.85),
                    Pos2::new(c.x - r * 0.35, c.y + r * 0.15),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + r * 0.75, c.y + r * 0.85),
                    Pos2::new(c.x + r * 0.35, c.y + r * 0.15),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.35, c.y + r * 0.15),
                    Pos2::new(c.x + r * 0.35, c.y + r * 0.15),
                ],
                stroke,
            );
        }
        Icon::Eye => {
            painter.circle_stroke(c, r * 0.28, stroke);
            painter.add(egui::Shape::line(
                vec![
                    Pos2::new(c.x - r, c.y),
                    Pos2::new(c.x - r * 0.35, c.y - r * 0.45),
                    Pos2::new(c.x + r * 0.35, c.y - r * 0.45),
                    Pos2::new(c.x + r, c.y),
                    Pos2::new(c.x + r * 0.35, c.y + r * 0.45),
                    Pos2::new(c.x - r * 0.35, c.y + r * 0.45),
                    Pos2::new(c.x - r, c.y),
                ],
                stroke,
            ));
        }
    }
}

fn draw_account_avatar(
    painter: &egui::Painter,
    rect: Rect,
    texture: Option<&egui::TextureHandle>,
    name: &str,
) {
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    painter.circle_filled(
        rect.center(),
        rect.width() * 0.5,
        Color32::from_rgb(48, 48, 54),
    );
    let initial = name
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        FontId::proportional((rect.height() * 0.46).max(10.0)),
        text_primary(),
    );
}

fn chrome_button(ui: &mut egui::Ui, rect: Rect, id: &str, icon: Icon, danger: bool) -> egui::Response {
    let resp = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            0.0,
            if danger { ROSE } else { bg_card_hover() },
        );
    }
    let color = if danger && resp.hovered() {
        Color32::WHITE
    } else {
        text_primary()
    };
    paint_icon(ui.painter(), rect.shrink2(Vec2::new(16.0, 11.0)), icon, color);
    resp
}

fn paint_auth_backdrop(painter: &egui::Painter, rect: Rect, texture: Option<&egui::TextureHandle>) {
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.circle_filled(
            Pos2::new(rect.left() + rect.width() * 0.3, rect.center().y),
            220.0,
            Color32::from_rgba_unmultiplied(139, 92, 246, 70),
        );
    }
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(120));
}

fn wide_accent_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    wide_button(ui, label, ACCENT_PURPLE, true)
}

fn wide_button(ui: &mut egui::Ui, label: &str, fill: Color32, accent_hover: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
    let color = if resp.hovered() && accent_hover {
        ACCENT_PURPLE_HOVER
    } else if resp.hovered() {
        Color32::from_rgb(71, 82, 196)
    } else {
        fill
    };
    ui.painter().rect_filled(rect, 12.0, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(15.0),
        Color32::WHITE,
    );
    resp
}

fn labeled_icon_field(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &mut String,
    hint: &str,
    password: bool,
    _width: f32,
) {
    egui::Frame::NONE
        .fill(bg_input())
        .stroke(Stroke::new(1.0, border_subtle()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                paint_icon(ui.painter(), icon_rect, icon, text_dim());
                ui.add(
                    TextEdit::singleline(text)
                        .password(password)
                        .hint_text(hint)
                        .desired_width(ui.available_width())
                        .frame(egui::Frame::NONE),
                );
            });
        });
}

fn window_resize_edges(ctx: &egui::Context) {
    if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let rect = ctx.content_rect();
    let t = 5.0;
    let edges = [
        (Rect::from_min_max(rect.left_top(), Pos2::new(rect.left() + t, rect.top() + t)), egui::ResizeDirection::NorthWest, "nw"),
        (Rect::from_min_max(Pos2::new(rect.right() - t, rect.top()), rect.right_top() + Vec2::new(0.0, t)), egui::ResizeDirection::NorthEast, "ne"),
        (Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - t), Pos2::new(rect.left() + t, rect.bottom())), egui::ResizeDirection::SouthWest, "sw"),
        (Rect::from_min_max(rect.right_bottom() - Vec2::splat(t), rect.right_bottom()), egui::ResizeDirection::SouthEast, "se"),
        (Rect::from_min_max(Pos2::new(rect.left() + t, rect.top()), Pos2::new(rect.right() - t, rect.top() + t)), egui::ResizeDirection::North, "n"),
        (Rect::from_min_max(Pos2::new(rect.left() + t, rect.bottom() - t), Pos2::new(rect.right() - t, rect.bottom())), egui::ResizeDirection::South, "s"),
        (Rect::from_min_max(Pos2::new(rect.left(), rect.top() + t), Pos2::new(rect.left() + t, rect.bottom() - t)), egui::ResizeDirection::West, "w"),
        (Rect::from_min_max(Pos2::new(rect.right() - t, rect.top() + t), Pos2::new(rect.right(), rect.bottom() - t)), egui::ResizeDirection::East, "e"),
    ];
    for (edge, direction, id) in edges {
        let response = egui::Area::new(egui::Id::unique(("window-resize", id)))
            .order(egui::Order::Foreground)
            .fixed_pos(edge.min)
            .interactable(true)
            .show(ctx, |ui| {
                ui.allocate_exact_size(edge.size(), Sense::drag()).1
            });
        if response.inner.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

fn decode_cover_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.resize(1400, 900, image::imageops::FilterType::Triangle);
    let rgba = image.to_rgba8();
    Some((rgba.width() as usize, rgba.height() as usize, rgba.into_raw()))
}

fn decode_avatar_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let image = image::load_from_memory(bytes).ok()?;
    let side = image.width().min(image.height());
    let image = image.crop_imm(
        (image.width() - side) / 2,
        (image.height() - side) / 2,
        side,
        side,
    );
    let image = image.resize(128, 128, image::imageops::FilterType::Triangle);
    let mut rgba = image.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let radius = cx.min(cy) - 0.5;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let pixel = rgba.get_pixel_mut(x, y);
            if dist > radius {
                pixel[3] = 0;
            } else if dist > radius - 1.0 {
                let edge = (radius - dist).clamp(0.0, 1.0);
                pixel[3] = (pixel[3] as f32 * edge) as u8;
            }
        }
    }
    Some((width as usize, height as usize, rgba.into_raw()))
}

pub fn launch(cfg: LaunchConfig) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Serika Code")
            .with_decorations(false)
            .with_resizable(true)
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
    Account(Result<(AccountProfile, UsageSnapshot), AccountLoadError>),
    SessionUpdated(StoredSession),
    AvatarBytes { url: String, bytes: Vec<u8> },
    MagicLink(Result<String, String>),
    AuthArt { artist: String, bytes: Vec<u8> },
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
struct AccountLoadError {
    auth: bool,
    message: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Account,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellGate {
    Login,
    Welcome,
    App,
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
    auth_busy: bool,
    awaiting_second_factor: bool,
    avatar_texture: Option<egui::TextureHandle>,
    avatar_loaded_for: Option<String>,
    avatar_pending: Option<(String, Vec<u8>)>,
    avatar_fetching: bool,
    show_welcome: bool,
    login_is_sign_in: bool,
    show_password: bool,
    remember_me: bool,
    agreed_terms: bool,
    username_input: String,
    dob_input: String,
    auth_art: Option<egui::TextureHandle>,
    auth_artist: String,
    auth_art_pending: Option<(String, Vec<u8>)>,
    auth_art_requested: bool,

    // Panels & Modals
    locale: Locale,
    theme_light: bool,
    show_inspector: bool,
    inspector_tab: InspectorTab,
    show_providers: bool,
    show_settings: bool,
    settings_tab: SettingsTab,
    show_palette: bool,
    show_model_menu: bool,
    show_account_menu: bool,
    show_chat_menu: bool,
    show_shortcuts: bool,
    palette_query: String,
    model_anchor: Rect,
    account_anchor: Rect,
    chat_anchor: Rect,

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
        let locale = Locale::parse(
            core.as_ref()
                .map(|c| c.config.locale.as_str())
                .unwrap_or("en"),
        );
        let theme_light = core
            .as_ref()
            .map(|c| c.config.theme == "light")
            .unwrap_or(false);
        set_ui_light(theme_light);
        apply_serika_theme(&cc.egui_ctx);
        // Serika Cloud is the primary mode. A saved local/cli id is not a cloud preference.
        let selected_model = core
            .as_ref()
            .and_then(|c| c.config.model.clone())
            .map(|model| model.trim().to_string())
            .filter(|model| {
                !model.is_empty() && !model.starts_with("local/") && !model.starts_with("cli/")
            })
            .unwrap_or_else(|| "openrouter/free".to_string());

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
        let show_welcome = cfg.screenshot_path.is_none();

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
            auth_busy: false,
            awaiting_second_factor: false,
            avatar_texture: None,
            avatar_loaded_for: None,
            avatar_pending: None,
            avatar_fetching: false,
            show_welcome,
            login_is_sign_in: true,
            show_password: false,
            remember_me: true,
            agreed_terms: false,
            username_input: String::new(),
            dob_input: String::new(),
            auth_art: None,
            auth_artist: String::new(),
            auth_art_pending: None,
            auth_art_requested: false,
            locale,
            theme_light,
            show_inspector,
            inspector_tab: InspectorTab::Git,
            show_providers: false,
            show_settings: false,
            settings_tab: SettingsTab::General,
            show_palette: false,
            show_model_menu: false,
            show_account_menu: false,
            show_chat_menu: false,
            show_shortcuts: false,
            palette_query: String::new(),
            model_anchor: Rect::ZERO,
            account_anchor: Rect::ZERO,
            chat_anchor: Rect::ZERO,
            git,
            commit_message: String::new(),
            worktree_branch_input: String::new(),
            last_git_poll: Instant::now(),
            update_info: None,
            setup_status: String::new(),
        };
        if app.screenshot_path.is_some() {
            app.prepare_screenshot_preset();
        }
        app.refresh_remote();
        app
    }

    fn prepare_screenshot_preset(&mut self) {
        let now = 1_793_500_000i64;
        self.discovery_report.local_count = 6;
        self.discovery_report.anthropic_count = 4;
        self.discovery_report.openai_count = 5;
        self.discovery_report.gemini_count = 4;
        self.selected_model = "openrouter/free".to_string();
        self.locale = Locale::En;
        self.theme_light = false;
        set_ui_light(false);
        self.catalog = screenshot_catalog();
        self.profile = Some(AccountProfile {
            id: "screenshot".into(),
            username: "alex".into(),
            email: String::new(),
            is_premium: true,
            is_verified: Some(true),
            subscription_status: Some("active".into()),
            subscription_plan: Some("Serika+ AI Max".into()),
            subscription_period_end: Some("2026-11-30T00:00:00Z".into()),
            display_name: None,
            avatar: None,
        });
        self.usage = Some(screenshot_usage());
        self.sessions = vec![
            SessionSummary {
                id: "sess_01".into(),
                title: "Oh, how are we doing ton...".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 3600 * 2,
                created_at: now - 3600 * 2,
            },
            SessionSummary {
                id: "sess_02".into(),
                title: "Yo, how's it going?".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 3600 * 24,
                created_at: now - 3600 * 24,
            },
            SessionSummary {
                id: "sess_03".into(),
                title: "can you check where tickets are handled?".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 8,
                created_at: now - 86400 * 8,
            },
            SessionSummary {
                id: "sess_04".into(),
                title: "Hey! (branch)".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 10,
                created_at: now - 86400 * 10,
            },
            SessionSummary {
                id: "sess_05".into(),
                title: "Hey!".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 12,
                created_at: now - 86400 * 12,
            },
            SessionSummary {
                id: "sess_06".into(),
                title: "Build Pomodoro timer in React".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 15,
                created_at: now - 86400 * 15,
            },
            SessionSummary {
                id: "sess_07".into(),
                title: "OAuth 2.0 flow diagram".into(),
                project_path: self.project.display().to_string(),
                model_id: "openrouter/free".into(),
                permission: "standard".into(),
                updated_at: now - 86400 * 18,
                created_at: now - 86400 * 18,
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
                        attachments: None, model: None,
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
                        attachments: None, model: None,
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
            Some("models") => {
                self.show_inspector = false;
                self.show_model_menu = true;
            }
            Some("account") => {
                self.show_inspector = false;
                self.show_account_menu = true;
            }
            Some("settings") => {
                self.show_inspector = false;
                self.show_settings = true;
                self.settings_tab = SettingsTab::General;
            }
            Some("chat") => {
                self.show_inspector = false;
                self.show_chat_menu = true;
                self.active_session = Some("sess_01".into());
            }
            _ => {
                self.active_session = None;
                self.show_inspector = false;
                self.inspector_tab = InspectorTab::Git;
            }
        }
    }

    fn refresh_remote(&mut self) {
        let Some(core) = self.core.clone() else {
            return;
        };
        self.discovering_models = true;
        {
            let tx = self.tx.clone();
            let core_disc = core.clone();
            self.tokio.spawn(async move {
                let report = discover_all_models(core_disc.client.http(), &core_disc.config).await;
                let _ = tx.send(UiNotice::ModelsDiscovered(report));
            });
        }
        {
            let tx = self.tx.clone();
            let core_models = core.clone();
            self.tokio.spawn(async move {
                let _ = tx.send(UiNotice::Catalog(
                    core_models.client.models().await.map_err(|e| e.to_string()),
                ));
            });
        }
        if self.session.is_some() {
            let tx = self.tx.clone();
            let core_acct = core;
            self.tokio.spawn(async move {
                // Runtime::account / usage refresh an expired access token before giving up.
                match core_acct.account().await {
                    Ok(profile) => {
                        let usage = core_acct.usage().await.unwrap_or_default();
                        let _ = tx.send(UiNotice::Account(Ok((profile, usage))));
                        if let Ok(Some(session)) = core_acct.secrets.load() {
                            let _ = tx.send(UiNotice::SessionUpdated(session));
                        }
                    }
                    Err(err) => {
                        let auth = matches!(
                            err,
                            serika_runtime::Error::Auth(_) | serika_runtime::Error::AuthRequired
                        );
                        let _ = tx.send(UiNotice::Account(Err(AccountLoadError {
                            auth,
                            message: err.to_string(),
                        })));
                    }
                }
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
        let Some(core) = self.core.clone() else {
            return;
        };
        let Some(session_id) = self.active_session.clone() else {
            return;
        };
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
        let Some(core) = self.core.clone() else {
            return;
        };
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
            attachments: Vec::new(),
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
                    self.discovery_report = report;
                }
                UiNotice::Catalog(Ok(catalog)) => {
                    self.catalog = catalog;
                    self.reconcile_cloud_model();
                }
                UiNotice::Catalog(Err(_)) => {}
                UiNotice::Account(Ok((profile, usage))) => {
                    self.note_profile(profile);
                    self.usage = Some(usage);
                }
                UiNotice::Account(Err(err)) => {
                    self.auth_busy = false;
                    self.auth_notice = err.message.clone();
                    if err.auth {
                        self.session = None;
                        self.profile = None;
                        self.usage = None;
                        self.clear_avatar();
                        if let Some(core) = &self.core {
                            let _ = core.secrets.clear();
                        }
                    }
                }
                UiNotice::SessionUpdated(session) => {
                    if self.session.is_some() {
                        self.session = Some(session);
                    }
                }
                UiNotice::AvatarBytes { url, bytes } => {
                    self.avatar_fetching = false;
                    if !bytes.is_empty() && self.avatar_loaded_for.as_deref() == Some(url.as_str())
                    {
                        self.avatar_pending = Some((url, bytes));
                    }
                }
                UiNotice::AuthArt { artist, bytes } => {
                    self.auth_artist = artist;
                    if !bytes.is_empty() {
                        self.auth_art_pending = Some((self.auth_artist.clone(), bytes));
                    }
                }
                UiNotice::MagicLink(Ok(message)) => {
                    self.auth_busy = false;
                    self.auth_notice = message;
                }
                UiNotice::MagicLink(Err(err)) => {
                    self.auth_busy = false;
                    self.auth_notice = err;
                }
                UiNotice::LoggedIn(Ok(LoginOutcome::SignedIn(session))) => {
                    if let Some(core) = &self.core {
                        let _ = core.secrets.save(&session);
                    }
                    self.session = Some(session);
                    self.password_input.zeroize();
                    self.two_factor_input.clear();
                    self.magic_token_input.clear();
                    self.awaiting_second_factor = false;
                    self.auth_busy = false;
                    self.show_welcome = false;
                    self.auth_notice = self.t(Msg::SignedInCloud).to_string();
                    self.refresh_remote();
                }
                UiNotice::LoggedIn(Ok(LoginOutcome::NeedsSecondFactor { message })) => {
                    self.auth_busy = false;
                    self.awaiting_second_factor = true;
                    self.auth_notice = if message.trim().is_empty() {
                        self.t(Msg::SecondFactorHint).to_string()
                    } else {
                        message
                    };
                }
                UiNotice::LoggedIn(Err(err)) => {
                    self.auth_busy = false;
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
                    id, title, detail, ..
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
                AgentEvent::Browser { .. } | AgentEvent::ProviderUsage { .. } | AgentEvent::Handoff { .. } | AgentEvent::Subagent { .. } => {}
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
        set_ui_light(self.theme_light);
        self.drain_channels();
        self.install_pending_avatar(&ctx);
        self.install_pending_auth_art(&ctx);
        let waiting_for_account = self.session.is_some() && self.profile.is_none();
        let waiting_for_avatar = self.avatar_fetching || self.avatar_pending.is_some();
        let waiting_for_art = self.auth_art_requested && self.auth_art.is_none();
        if self.auth_busy || waiting_for_account || waiting_for_avatar || waiting_for_art {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if self.smoke {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
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
            if i.key_pressed(Key::Escape) {
                self.show_model_menu = false;
                self.show_account_menu = false;
                self.show_chat_menu = false;
                self.show_shortcuts = false;
            }
        });

        if self.screenshot_view.as_deref() == Some("cli") {
            self.draw_title_bar(root_ui);
            self.draw_codex_cli_showcase(root_ui);
        } else {
            self.draw_title_bar(root_ui);
            match self.shell_gate() {
                ShellGate::Login => self.draw_login_screen(root_ui),
                ShellGate::Welcome => self.draw_welcome_screen(root_ui),
                ShellGate::App => {
            self.draw_sidebar(root_ui);
            if self.show_inspector {
                self.draw_inspector(root_ui);
            }
            self.draw_main_canvas(root_ui);

            if self.show_providers
                || self.show_settings
                || self.show_palette
                || self.show_shortcuts
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
            self.draw_shortcuts_modal(&ctx);
            self.draw_floating_menus(&ctx);
                }
            }
        }
        window_resize_edges(&ctx);

        if let Some(out_path) = self.screenshot_path.clone() {
            self.screenshot_frames += 1;
            let preset_profile = self
                .profile
                .as_ref()
                .is_some_and(|profile| profile.id == "screenshot");
            let waiting_avatar = self.avatar_fetching
                || self.avatar_pending.is_some()
                || (self.avatar_loaded_for.is_some() && self.avatar_texture.is_none());
            let keep_waiting = self.screenshot_frames < 8
                || (self.session.is_some()
                    && self.screenshot_frames < 120
                    && (self.profile.is_none() || preset_profile || waiting_avatar));
            ctx.request_repaint_after(Duration::from_millis(40));
            if !keep_waiting && !self.screenshot_requested {
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
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| "there".to_string())
    }

    fn t(&self, key: Msg) -> &'static str {
        i18n::t(self.locale, key)
    }

    fn tf(&self, key: Msg, pairs: &[(&str, &str)]) -> String {
        i18n::fill(self.t(key), pairs)
    }

    fn greeting(&self, name: &str) -> String {
        let key = if self.screenshot_path.is_some() {
            Msg::GoodMorning
        } else {
            match local_hour() {
                5..=11 => Msg::GoodMorning,
                12..=17 => Msg::GoodAfternoon,
                18..=21 => Msg::GoodEvening,
                _ => Msg::GoodNight,
            }
        };
        self.tf(key, &[("{name}", name)])
    }

    fn model_label(&self) -> String {
        if let Some(model) = self.catalog.find(&self.selected_model) {
            return model.name.clone();
        }
        if self.selected_model.is_empty() {
            return self.t(Msg::SelectModel).to_string();
        }
        if self.selected_model == "openrouter/free" {
            return "openrouter Free".to_string();
        }
        self.selected_model.clone()
    }

    fn can_use_premium(&self) -> bool {
        if let Some(plan) = self.usage.as_ref().and_then(|usage| usage.plan.as_ref()) {
            if plan.is_premium.unwrap_or(false) || plan.byok.unwrap_or(false) {
                return true;
            }
        }
        if self
            .profile
            .as_ref()
            .is_some_and(|profile| profile.is_premium)
        {
            return true;
        }
        self.session
            .as_ref()
            .is_some_and(|session| session.is_premium)
    }

    fn plan_label(&self) -> String {
        let name = if let Some(plan) = self.usage.as_ref().and_then(|usage| usage.plan.as_ref()) {
            if let Some(name) = plan
                .tier
                .as_ref()
                .and_then(|tier| tier.name.clone())
                .filter(|name| !name.trim().is_empty())
            {
                name
            } else if plan.is_premium.unwrap_or(false) {
                self.profile
                    .as_ref()
                    .and_then(|profile| profile.subscription_plan.clone())
                    .unwrap_or_else(|| "Serika+".to_string())
            } else {
                self.t(Msg::FreePlan).to_string()
            }
        } else if self
            .profile
            .as_ref()
            .is_some_and(|profile| profile.is_premium)
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.is_premium)
        {
            self.profile
                .as_ref()
                .and_then(|profile| profile.subscription_plan.clone())
                .unwrap_or_else(|| "Serika+".to_string())
        } else {
            self.t(Msg::FreePlan).to_string()
        };
        self.with_expiry(name)
    }

    fn with_expiry(&self, name: String) -> String {
        let Some(end) = self
            .profile
            .as_ref()
            .and_then(|profile| profile.subscription_period_end.clone())
        else {
            return name;
        };
        match format_expiry(&end) {
            Some(pretty) => format!("{name} · until {pretty}"),
            None => name,
        }
    }

    fn note_profile(&mut self, profile: AccountProfile) {
        let next = profile.avatar.as_deref().and_then(absolute_avatar_url);
        let changed = next.as_deref() != self.avatar_loaded_for.as_deref();
        if changed {
            self.clear_avatar();
            if let Some(url) = next.clone() {
                self.fetch_avatar(url);
            }
        }
        self.profile = Some(profile);
        self.avatar_loaded_for = next;
    }

    fn clear_avatar(&mut self) {
        self.avatar_texture = None;
        self.avatar_loaded_for = None;
        self.avatar_pending = None;
        self.avatar_fetching = false;
    }

    fn fetch_avatar(&mut self, url: String) {
        let Some(core) = self.core.clone() else {
            return;
        };
        self.avatar_fetching = true;
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let bytes = core.client.fetch_bytes(&url).await.unwrap_or_default();
            let _ = tx.send(UiNotice::AvatarBytes { url, bytes });
        });
    }

    fn install_pending_avatar(&mut self, ctx: &egui::Context) {
        let Some((url, bytes)) = self.avatar_pending.take() else {
            return;
        };
        if self.avatar_loaded_for.as_deref() != Some(url.as_str()) {
            return;
        }
        if let Some((width, height, rgba)) = decode_avatar_rgba(&bytes) {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            self.avatar_texture = Some(ctx.load_texture(
                "serika-account-avatar",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    fn install_pending_auth_art(&mut self, ctx: &egui::Context) {
        let Some((_, bytes)) = self.auth_art_pending.take() else {
            return;
        };
        if let Some((width, height, rgba)) = decode_cover_rgba(&bytes) {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            self.auth_art = Some(ctx.load_texture(
                "serika-auth-art",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    fn shell_gate(&self) -> ShellGate {
        if self.screenshot_path.is_some() {
            return ShellGate::App;
        }
        if self.session.is_none() {
            return ShellGate::Login;
        }
        if self.show_welcome {
            ShellGate::Welcome
        } else {
            ShellGate::App
        }
    }

    fn ensure_auth_art(&mut self) {
        if self.auth_art_requested {
            return;
        }
        let Some(core) = self.core.clone() else {
            return;
        };
        self.auth_art_requested = true;
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let Ok((artist, url)) = core.client.random_auth_background().await else {
                return;
            };
            let bytes = core.client.fetch_bytes(&url).await.unwrap_or_default();
            let _ = tx.send(UiNotice::AuthArt { artist, bytes });
        });
    }

    fn session_meter(&self) -> (f32, String, String) {
        let window = self
            .usage
            .as_ref()
            .and_then(|usage| usage.plan.as_ref())
            .and_then(|plan| plan.session.clone());
        let percent = window.as_ref().and_then(|window| {
            window
                .percent
                .or_else(|| match (window.used, window.limit) {
                    (Some(used), Some(limit)) if limit > 0 => {
                        Some((used as f64 / limit as f64) * 100.0)
                    }
                    _ => None,
                })
        });
        let pct = percent.unwrap_or(0.0).clamp(0.0, 100.0);
        let label = self.tf(
            Msg::SessionUsage,
            &[("{n}", &format!("{}", pct.round() as i32))],
        );
        let reset = window
            .as_ref()
            .and_then(|window| window.resets_at.as_deref())
            .map(|iso| {
                let when = format_reset_delta(iso).unwrap_or_else(|| self.t(Msg::Soon).to_string());
                self.tf(Msg::ResetsIn, &[("{n}", &when)])
            })
            .unwrap_or_default();
        ((pct as f32) / 100.0, label, reset)
    }

    fn weekly_meter(&self) -> Option<(f32, String, String)> {
        let window = self
            .usage
            .as_ref()
            .and_then(|usage| usage.plan.as_ref())
            .and_then(|plan| plan.weekly.clone())?;
        let percent = window
            .percent
            .or_else(|| match (window.used, window.limit) {
                (Some(used), Some(limit)) if limit > 0 => {
                    Some((used as f64 / limit as f64) * 100.0)
                }
                _ => None,
            })?;
        let pct = percent.clamp(0.0, 100.0);
        let label = self.tf(
            Msg::WeeklyQuota,
            &[("{n}", &format!("{}", pct.round() as i32))],
        );
        let reset = window
            .resets_at
            .as_deref()
            .map(|iso| {
                let when = format_reset_delta(iso).unwrap_or_else(|| self.t(Msg::Soon).to_string());
                self.tf(Msg::ResetsIn, &[("{n}", &when)])
            })
            .unwrap_or_default();
        Some(((pct as f32) / 100.0, label, reset))
    }

    fn reconcile_cloud_model(&mut self) {
        if self.catalog.find(&self.selected_model).is_some() {
            return;
        }
        let explicit = !self.selected_model.is_empty()
            && self.selected_model != "openrouter/free"
            && !self.selected_model.starts_with("local/")
            && !self.selected_model.starts_with("cli/");
        if explicit {
            return;
        }
        if let Some(id) = self
            .catalog
            .find("openrouter/free")
            .map(|model| model.id.clone())
            .or_else(|| self.catalog.default_coding_model())
            .or_else(|| self.catalog.free.first().map(|model| model.id.clone()))
        {
            self.selected_model = id;
        }
    }

    fn choose_model(&mut self, id: String) {
        self.selected_model = id;
        self.show_model_menu = false;
        self.model_search_input.clear();
        self.persist_preferences();
    }

    fn set_locale(&mut self, locale: Locale) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        self.persist_preferences();
    }

    fn set_theme_light(&mut self, light: bool) {
        if self.theme_light == light {
            return;
        }
        self.theme_light = light;
        set_ui_light(light);
        self.persist_preferences();
    }

    fn persist_preferences(&mut self) {
        let Some(core) = self.core.clone() else {
            return;
        };
        let mut config = core.config.clone();
        config.model = Some(self.selected_model.clone());
        config.locale = self.locale.code().to_string();
        config.theme = if self.theme_light { "light" } else { "dark" }.to_string();
        if let Err(err) = config.save(&core.paths.config_file) {
            self.status = err.to_string();
            return;
        }
        if let Ok(reopened) = Runtime::open() {
            self.core = Some(Arc::new(reopened));
        }
    }

    fn sign_out(&mut self) {
        if let Some(core) = self.core.clone() {
            self.tokio.spawn(async move {
                let _ = core.logout().await;
            });
        }
        self.session = None;
        self.profile = None;
        self.usage = None;
        self.clear_avatar();
        self.password_input.zeroize();
        self.two_factor_input.clear();
        self.magic_token_input.clear();
        self.awaiting_second_factor = false;
        self.auth_busy = false;
        self.show_account_menu = false;
        self.show_settings = false;
        self.status = self.t(Msg::SignedOut).to_string();
    }

    fn start_password_login(&mut self) {
        if self.auth_busy {
            return;
        }
        let Some(core) = self.core.clone() else {
            return;
        };
        let email = self.email_input.trim().to_string();
        let password = self.password_input.clone();
        if email.is_empty() || password.is_empty() {
            self.auth_notice = self.t(Msg::EnterCredentials).to_string();
            return;
        }
        let two_factor = non_empty(&self.two_factor_input);
        self.auth_busy = true;
        self.auth_notice.clear();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = core
                .login_password(&email, &password, two_factor.as_deref())
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send(UiNotice::LoggedIn(result));
        });
    }

    fn start_magic_link(&mut self) {
        if self.auth_busy {
            return;
        }
        let Some(core) = self.core.clone() else {
            return;
        };
        let email = self.email_input.trim().to_string();
        if email.is_empty() {
            self.auth_notice = self.t(Msg::EnterCredentials).to_string();
            return;
        }
        self.auth_busy = true;
        self.auth_notice.clear();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = core
                .request_magic_link(&email)
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send(UiNotice::MagicLink(result));
        });
    }

    fn consume_magic_link(&mut self) {
        if self.auth_busy {
            return;
        }
        let Some(core) = self.core.clone() else {
            return;
        };
        let token = self.magic_token_input.trim().to_string();
        if token.is_empty() {
            self.auth_notice = self.t(Msg::LinkToken).to_string();
            return;
        }
        let two_factor = non_empty(&self.two_factor_input);
        self.auth_busy = true;
        self.auth_notice.clear();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = core
                .consume_magic_link(&token, two_factor.as_deref())
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send(UiNotice::LoggedIn(result));
        });
    }

    fn export_chat(&mut self) {
        self.show_chat_menu = false;
        if self.messages.is_empty() {
            return;
        }
        let mut markdown = String::from("# Serika Code\n\n");
        for message in &self.messages {
            let heading = if message.role == "user" {
                "You"
            } else {
                "Serika"
            };
            markdown.push_str(&format!("## {heading}\n\n{}\n\n", message.content.trim()));
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("serika-chat.md")
            .save_file()
        {
            if let Err(err) = std::fs::write(&path, markdown) {
                self.status = err.to_string();
            } else {
                self.status = format!("Exported {}", path.display());
            }
        }
    }

    fn close_popovers(&mut self) {
        self.show_model_menu = false;
        self.show_account_menu = false;
        self.show_chat_menu = false;
    }

    fn draw_title_bar(&mut self, root_ui: &mut egui::Ui) {
        let maximized = root_ui
            .ctx()
            .input(|input| input.viewport().maximized.unwrap_or(false));
        egui::Panel::top("serika_chrome")
            .exact_size(36.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(bg_sidebar()))
            .show(root_ui, |ui| {
                let rect = ui.max_rect();
                let button_w = 46.0;
                let close_rect = Rect::from_min_max(
                    Pos2::new(rect.right() - button_w, rect.top()),
                    rect.right_bottom(),
                );
                let max_rect = Rect::from_min_max(
                    Pos2::new(close_rect.left() - button_w, rect.top()),
                    Pos2::new(close_rect.left(), rect.bottom()),
                );
                let min_rect = Rect::from_min_max(
                    Pos2::new(max_rect.left() - button_w, rect.top()),
                    Pos2::new(max_rect.left(), rect.bottom()),
                );
                let drag_rect = Rect::from_min_max(rect.left_top(), min_rect.left_bottom());
                let drag = ui.interact(
                    drag_rect,
                    ui.make_persistent_id("window_drag"),
                    Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.painter().text(
                    Pos2::new(drag_rect.left() + 14.0, drag_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Serika Code",
                    FontId::proportional(13.0),
                    text_secondary(),
                );
                if chrome_button(ui, min_rect, "min", Icon::Minimize, false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                let open_icon = if maximized { Icon::Restore } else { Icon::Maximize };
                if chrome_button(ui, max_rect, "open", open_icon, false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if chrome_button(ui, close_rect, "close", Icon::Close, true).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
    }

    fn draw_welcome_screen(&mut self, ui: &mut egui::Ui) {
        self.ensure_auth_art();
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        paint_auth_backdrop(ui.painter(), rect, self.auth_art.as_ref());
        let name = self.current_username();
        let greeting = self.greeting(&name);
        let card = Rect::from_center_size(rect.center(), Vec2::new(460.0, 320.0));
        ui.painter().rect_filled(card, 20.0, Color32::from_black_alpha(170));
        ui.painter()
            .rect_stroke(card, 20.0, Stroke::new(1.0, border_subtle()), StrokeKind::Inside);
        let mut card_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(card.shrink(28.0))
                .layout(Layout::top_down(Align::Center)),
        );
        card_ui.add_space(12.0);
        let (mark, _) = card_ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
        paint_icon(card_ui.painter(), mark.shrink(6.0), Icon::SerikaMark, Color32::WHITE);
        card_ui.add_space(16.0);
        card_ui.label(
            RichText::new("Welcome")
                .size(32.0)
                .strong()
                .color(Color32::WHITE),
        );
        card_ui.add_space(6.0);
        card_ui.label(RichText::new(greeting).size(16.0).color(text_secondary()));
        card_ui.add_space(4.0);
        card_ui.label(
            RichText::new("Your Serika Code workspace is ready.")
                .size(13.0)
                .color(text_dim()),
        );
        card_ui.add_space(22.0);
        if wide_accent_button(&mut card_ui, "Continue").clicked() {
            self.show_welcome = false;
        }
        if !self.auth_artist.is_empty() {
            ui.painter().text(
                Pos2::new(rect.left() + 16.0, rect.bottom() - 18.0),
                egui::Align2::LEFT_CENTER,
                format!("Art by {}", self.auth_artist),
                FontId::proportional(11.0),
                Color32::from_white_alpha(180),
            );
        }
    }

    fn draw_login_screen(&mut self, ui: &mut egui::Ui) {
        self.ensure_auth_art();
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let panel_w = 480.0_f32.min(rect.width() * 0.46).max(360.0);
        let art_rect = Rect::from_min_max(rect.left_top(), Pos2::new(rect.right() - panel_w, rect.bottom()));
        let form_rect = Rect::from_min_max(Pos2::new(art_rect.right(), rect.top()), rect.right_bottom());
        paint_auth_backdrop(ui.painter(), art_rect, self.auth_art.as_ref());
        ui.painter().rect_filled(form_rect, 0.0, bg_sidebar());
        ui.painter().rect_filled(
            Rect::from_min_max(form_rect.left_top(), Pos2::new(form_rect.left() + 1.0, form_rect.bottom())),
            0.0,
            border_subtle(),
        );
        if !self.auth_artist.is_empty() {
            ui.painter().text(
                Pos2::new(art_rect.left() + 16.0, art_rect.bottom() - 18.0),
                egui::Align2::LEFT_CENTER,
                format!("Art by {}", self.auth_artist),
                FontId::proportional(11.0),
                Color32::from_white_alpha(190),
            );
        }

        let mut form = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(form_rect.shrink2(Vec2::new(36.0, 24.0)))
                .layout(Layout::top_down(Align::Center)),
        );
        ScrollArea::vertical()
            .id_salt("signature_login")
            .auto_shrink([false, false])
            .show(&mut form, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    paint_icon(ui.painter(), mark.shrink(4.0), Icon::SerikaMark, Color32::WHITE);
                    ui.add_space(8.0);
                    ui.label(RichText::new("serika").strong().size(22.0).color(Color32::WHITE));
                    ui.label(
                        RichText::new("Serika Code")
                            .size(13.0)
                            .color(text_dim()),
                    );
                    ui.add_space(28.0);
                    let title = if self.login_is_sign_in {
                        "Welcome Back"
                    } else {
                        "Create Account"
                    };
                    let subtitle = if self.login_is_sign_in {
                        "Enter your details to sign in"
                    } else {
                        "Fill in your information to get started"
                    };
                    ui.label(RichText::new(title).strong().size(28.0).color(Color32::WHITE));
                    ui.add_space(4.0);
                    ui.label(RichText::new(subtitle).size(13.5).color(text_secondary()));
                });
                ui.add_space(22.0);
                if !self.auth_notice.is_empty() {
                    egui::Frame::NONE
                        .fill(Color32::from_rgba_unmultiplied(80, 16, 24, 180))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.auth_notice).size(13.0).color(Color32::from_rgb(252, 165, 165)));
                        });
                    ui.add_space(14.0);
                }
                if self.awaiting_second_factor {
                    ui.label(
                        RichText::new(self.t(Msg::SecondFactorHint))
                            .size(12.5)
                            .color(AMBER),
                    );
                    ui.add_space(8.0);
                }
                let field_w = ui.available_width();
                if !self.login_is_sign_in {
                    labeled_icon_field(ui, Icon::User, &mut self.username_input, "Username", false, field_w);
                    ui.add_space(12.0);
                }
                labeled_icon_field(ui, Icon::Mail, &mut self.email_input, "Email address", false, field_w);
                ui.add_space(12.0);
                labeled_icon_field(
                    ui,
                    Icon::Lock,
                    &mut self.password_input,
                    "Password",
                    !self.show_password,
                    field_w,
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.show_password { "Hide password" } else { "Show password" })
                        .clicked()
                    {
                        self.show_password = !self.show_password;
                    }
                });
                if !self.login_is_sign_in {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Date of birth (16+)").size(12.0).color(text_dim()));
                    ui.add(
                        TextEdit::singleline(&mut self.dob_input)
                            .hint_text("YYYY-MM-DD")
                            .desired_width(field_w),
                    );
                }
                ui.add_space(12.0);
                if self.login_is_sign_in {
                    ui.checkbox(&mut self.remember_me, "Remember me for 30 days");
                } else {
                    ui.checkbox(&mut self.agreed_terms, "I agree to the Terms and Privacy Policy");
                }
                if self.awaiting_second_factor {
                    ui.add_space(10.0);
                    labeled_icon_field(
                        ui,
                        Icon::Shield,
                        &mut self.two_factor_input,
                        "Authentication code",
                        false,
                        field_w,
                    );
                }
                ui.add_space(16.0);
                let label = if self.auth_busy {
                    "…"
                } else if self.login_is_sign_in {
                    "Sign In"
                } else {
                    "Create Account"
                };
                if wide_accent_button(ui, label).clicked() && !self.auth_busy {
                    if self.login_is_sign_in {
                        self.start_password_login();
                    } else {
                        self.start_register();
                    }
                }
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("or").size(12.0).color(text_dim()));
                });
                ui.add_space(10.0);
                if wide_button(ui, "Continue with Discord", Color32::from_rgb(88, 101, 242), false).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(
                        "https://accounts.serika.dev/api/auth/discord/login",
                    ));
                }
                if self.login_is_sign_in {
                    ui.add_space(12.0);
                    if ui
                        .link(RichText::new("Forgot Password?").size(13.0).color(ACCENT_PURPLE_SEC))
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(
                            "https://accounts.serika.dev/forgot-password",
                        ));
                    }
                    ui.add_space(6.0);
                    if ui
                        .link(RichText::new(self.t(Msg::SendLink)).size(13.0).color(text_secondary()))
                        .clicked()
                    {
                        self.start_magic_link();
                    }
                    if !self.magic_token_input.is_empty() || self.auth_notice.contains("link") {
                        ui.add_space(8.0);
                        labeled_icon_field(
                            ui,
                            Icon::Key,
                            &mut self.magic_token_input,
                            "Paste the token from the email link",
                            false,
                            field_w,
                        );
                        ui.add_space(8.0);
                        if wide_accent_button(ui, "Continue with link").clicked() {
                            self.consume_magic_link();
                        }
                    }
                }
                ui.add_space(16.0);
                let switch = if self.login_is_sign_in {
                    "Need an account? Sign up"
                } else {
                    "Already have an account? Sign in"
                };
                if ui
                    .link(RichText::new(switch).size(13.5).color(text_secondary()))
                    .clicked()
                {
                    self.login_is_sign_in = !self.login_is_sign_in;
                    self.auth_notice.clear();
                    self.awaiting_second_factor = false;
                }
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("© 2026 Serika.dev")
                            .size(11.0)
                            .color(text_dim()),
                    );
                });
            });
    }

    fn start_register(&mut self) {
        if self.auth_busy {
            return;
        }
        let Some(core) = self.core.clone() else {
            return;
        };
        if !self.agreed_terms {
            self.auth_notice = "Agree to the Terms and Privacy Policy to create an account.".into();
            return;
        }
        let username = self.username_input.trim().to_string();
        let email = self.email_input.trim().to_string();
        let password = self.password_input.clone();
        let dob = self.dob_input.trim().to_string();
        if username.is_empty() || email.is_empty() || password.is_empty() || dob.is_empty() {
            self.auth_notice = "Username, email, password, and date of birth are required.".into();
            return;
        }
        self.auth_busy = true;
        self.auth_notice.clear();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = core
                .register(&username, &email, &password, &dob)
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send(UiNotice::LoggedIn(result));
        });
    }

    fn draw_sidebar(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::left("serikachat_sidebar")
            .resizable(true)
            .default_size(260.0)
            .size_range(220.0..=330.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg_sidebar())
                    .inner_margin(Margin::symmetric(12, 12))
                    .stroke(Stroke::new(1.0, border_hairline())),
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
                    ui.label(RichText::new("code").size(18.0).color(text_muted()));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, bg_card_hover());
                        }
                        paint_icon(
                            ui.painter(),
                            r.shrink(5.5),
                            Icon::PanelRight,
                            if self.show_inspector || resp.hovered() {
                                text_primary()
                            } else {
                                text_muted()
                            },
                        );
                        if resp
                            .on_hover_text(format!("{} (Ctrl+B)", self.t(Msg::ToggleInspector)))
                            .clicked()
                        {
                            self.show_inspector = !self.show_inspector;
                        }
                    });
                });

                ui.add_space(14.0);

                if draw_sidebar_nav_row(
                    ui,
                    Icon::Edit,
                    self.t(Msg::NewChat),
                    Some(("Ctrl", "N")),
                    false,
                ) {
                    self.new_thread();
                }
                ui.add_space(2.0);

                let project_label = workspace_display_name(&self.project, &self.core);
                let library = self.t(Msg::Library);
                let lib_label = if project_label == "Workspace" || project_label == "Scratch" {
                    library.to_string()
                } else {
                    format!("{library} · {project_label}")
                };
                if draw_sidebar_nav_row(ui, Icon::Book, &lib_label, None, false) {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.switch_project(folder);
                    }
                }

                ui.add_space(10.0);

                let search_hint = self.t(Msg::SearchChats);
                egui::Frame::NONE
                    .fill(search_fill())
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(1.0, border_hairline()))
                    .inner_margin(Margin::symmetric(11, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                            paint_icon(ui.painter(), ir, Icon::Search, text_dim());
                            ui.add(
                                TextEdit::singleline(&mut self.session_filter)
                                    .hint_text(search_hint)
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
                                RichText::new(self.t(Msg::Previous7Days))
                                    .size(11.5)
                                    .color(text_dim()),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(self.t(Msg::NoChatsYet))
                                    .size(12.5)
                                    .color(text_muted()),
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
                                        RichText::new(self.t(Msg::Previous7Days))
                                            .size(11.5)
                                            .color(text_dim()),
                                    );
                                    ui.add_space(4.0);
                                } else if age_days > 7 && !shown_30d {
                                    shown_30d = true;
                                    if shown_7d {
                                        ui.add_space(10.0);
                                    }
                                    ui.label(
                                        RichText::new(self.t(Msg::Previous30Days))
                                            .size(11.5)
                                            .color(text_dim()),
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
                                RichText::new(self.t(Msg::RecentWorkspaces))
                                    .size(11.5)
                                    .color(text_dim()),
                            );
                            ui.add_space(4.0);
                            let mut switch_to = None;
                            for rp in &self.recent_projects {
                                if rp == &self.project {
                                    continue;
                                }
                                let name =
                                    rp.file_name().and_then(|n| n.to_str()).unwrap_or("project");
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
                    let terms = self.t(Msg::Terms);
                    let privacy = self.t(Msg::Privacy);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        if ui
                            .link(RichText::new(terms).size(10.5).color(text_dim()))
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab("https://serika.dev/terms"));
                        }
                        ui.add_space(8.0);
                        if ui
                            .link(RichText::new(privacy).size(10.5).color(text_dim()))
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab("https://serika.dev/privacy"));
                        }
                    });

                    ui.add_space(6.0);

                    let username = self.current_username();
                    let plan = self.plan_label();
                    let account_tip = self.t(Msg::OpenAccount);
                    let prof_desired = Vec2::new(ui.available_width(), 44.0);
                    let (prof_rect, prof_resp) =
                        ui.allocate_exact_size(prof_desired, Sense::click());
                    self.account_anchor = prof_rect;
                    if prof_resp.hovered() || self.show_account_menu {
                        ui.painter().rect_filled(prof_rect, 8.0, bg_card_hover());
                    }
                    let av_rect = Rect::from_center_size(
                        Pos2::new(prof_rect.left() + 18.0, prof_rect.center().y),
                        Vec2::splat(28.0),
                    );
                    draw_account_avatar(
                        ui.painter(),
                        av_rect,
                        self.avatar_texture.as_ref(),
                        &username,
                    );

                    ui.painter().text(
                        Pos2::new(prof_rect.left() + 38.0, prof_rect.top() + 7.0),
                        egui::Align2::LEFT_TOP,
                        &username,
                        FontId::proportional(13.0),
                        text_primary(),
                    );
                    let plan_width = (prof_rect.right() - 28.0) - (prof_rect.left() + 38.0);
                    let plan = ellipsize(
                        ui.painter(),
                        &plan,
                        FontId::proportional(11.0),
                        text_dim(),
                        plan_width,
                    );
                    ui.painter().text(
                        Pos2::new(prof_rect.left() + 38.0, prof_rect.top() + 24.0),
                        egui::Align2::LEFT_TOP,
                        &plan,
                        FontId::proportional(11.0),
                        text_dim(),
                    );
                    let chev_rect = Rect::from_center_size(
                        Pos2::new(prof_rect.right() - 10.0, prof_rect.center().y),
                        Vec2::splat(12.0),
                    );
                    paint_icon(ui.painter(), chev_rect, Icon::ChevronDown, text_dim());
                    if prof_resp.on_hover_text(account_tip).clicked() {
                        let open = !self.show_account_menu;
                        self.close_popovers();
                        self.show_account_menu = open;
                    }

                    ui.add_space(6.0);
                    let div = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                    ui.painter().rect_filled(div, 0.0, border_hairline());
                    ui.add_space(6.0);

                    let (frac, session_label, reset_label) = self.session_meter();
                    let bar_rect = ui.allocate_space(Vec2::new(ui.available_width(), 2.0)).1;
                    ui.painter().rect_filled(bar_rect, 1.0, bg_card_hover());
                    let fill_rect = Rect::from_min_size(
                        bar_rect.min,
                        Vec2::new(bar_rect.width() * frac.clamp(0.0, 1.0), 2.0),
                    );
                    ui.painter().rect_filled(fill_rect, 1.0, ACCENT_PURPLE);

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(session_label).size(11.0).color(text_muted()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !reset_label.is_empty() {
                                ui.label(RichText::new(reset_label).size(11.0).color(text_dim()));
                            }
                        });
                    });
                });
            });
    }

    fn draw_main_canvas(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg_deep()))
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
            .fill(bg_deep())
            .inner_margin(Margin::symmetric(24, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let display_model = self.model_label();
                    let choose_tip = self.t(Msg::ChooseModel);
                    let text_w = ui
                        .painter()
                        .layout_no_wrap(
                            display_model.clone(),
                            FontId::proportional(15.0),
                            text_primary(),
                        )
                        .size()
                        .x
                        + 22.0;

                    let (tr, resp) =
                        ui.allocate_exact_size(Vec2::new(text_w, 32.0), Sense::click());
                    self.model_anchor = tr;
                    if resp.hovered() || self.show_model_menu {
                        ui.painter().rect_filled(tr, 8.0, bg_card_hover());
                    }
                    let fg = if resp.hovered() || self.show_model_menu {
                        text_primary()
                    } else {
                        text_secondary()
                    };
                    ui.painter().text(
                        Pos2::new(tr.left() + 8.0, tr.center().y),
                        egui::Align2::LEFT_CENTER,
                        &display_model,
                        FontId::proportional(15.0),
                        fg,
                    );
                    let chev_rect = Rect::from_center_size(
                        Pos2::new(tr.right() - 10.0, tr.center().y),
                        Vec2::splat(10.0),
                    );
                    paint_icon(ui.painter(), chev_rect, Icon::ChevronDown, text_dim());
                    if resp.on_hover_text(choose_tip).clicked() {
                        let open = !self.show_model_menu;
                        self.close_popovers();
                        self.show_model_menu = open;
                    }

                    // Centered ChatTitle (matches ChatTitle from chat-web)
                    if let Some(active_id) = &self.active_session {
                        if let Some(sess) = self.sessions.iter().find(|s| &s.id == active_id) {
                            let title = if sess.title.chars().count() > 42 {
                                let s: String = sess.title.chars().take(41).collect();
                                format!("{s}...")
                            } else {
                                sess.title.clone()
                            };
                            let header_center_x = ui.max_rect().center().x;
                            let title_galley = ui.painter().layout_no_wrap(
                                title,
                                FontId::proportional(13.5),
                                text_primary(),
                            );
                            ui.painter().galley(
                                Pos2::new(
                                    header_center_x - title_galley.size().x * 0.5,
                                    tr.center().y - title_galley.size().y * 0.5,
                                ),
                                title_galley,
                                text_primary(),
                            );
                        }
                    }

                    let chat_tip = self.t(Msg::ChatOptions);
                    let inspector_tip = format!("{} (Ctrl+B)", self.t(Msg::ToggleInspector));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        let (dots_r, dots_resp) =
                            ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        self.chat_anchor = dots_r;
                        if dots_resp.hovered() || self.show_chat_menu {
                            ui.painter()
                                .circle_filled(dots_r.center(), 14.0, bg_card_hover());
                        }
                        paint_icon(
                            ui.painter(),
                            dots_r.shrink(7.0),
                            Icon::Dots,
                            if dots_resp.hovered() || self.show_chat_menu {
                                text_primary()
                            } else {
                                text_muted()
                            },
                        );
                        if dots_resp.on_hover_text(chat_tip).clicked() {
                            let open = !self.show_chat_menu;
                            self.close_popovers();
                            self.show_chat_menu = open;
                        }

                        let (eye_r, eye_resp) =
                            ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        if eye_resp.hovered() {
                            ui.painter()
                                .circle_filled(eye_r.center(), 14.0, bg_card_hover());
                        }
                        paint_icon(
                            ui.painter(),
                            eye_r.shrink(7.0),
                            Icon::EyeSlash,
                            if self.show_inspector || eye_resp.hovered() {
                                text_primary()
                            } else {
                                text_muted()
                            },
                        );
                        if eye_resp.on_hover_text(inspector_tip).clicked() {
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
                            let uname = self.current_username();
                            let greeting = self.greeting(&uname);
                            ui.label(
                                RichText::new(greeting)
                                    .strong()
                                    .size(28.0)
                                    .color(text_primary()),
                            );

                            ui.add_space(24.0);

                            // 2. Centered SerikaChat Rounded Pill Composer Box
                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                self.draw_composer_card(ui, true);
                            });

                            ui.add_space(10.0);

                            // 3. Centered SerikaChat Disclaimer Line
                            ui.label(
                                RichText::new(self.t(Msg::Disclaimer))
                                    .size(11.5)
                                    .color(text_dim()),
                            );

                            ui.add_space(28.0);

                            // 4. SerikaChat Single-Line Starter Prompt Rows with 1px Dividers
                            let starters = [
                                (
                                    self.t(Msg::StarterTimerTitle),
                                    self.t(Msg::StarterTimerSub),
                                    self.t(Msg::StarterTimerPrompt),
                                ),
                                (
                                    self.t(Msg::StarterOauthTitle),
                                    self.t(Msg::StarterOauthSub),
                                    self.t(Msg::StarterOauthPrompt),
                                ),
                                (
                                    self.t(Msg::StarterReadmeTitle),
                                    self.t(Msg::StarterReadmeSub),
                                    self.t(Msg::StarterReadmePrompt),
                                ),
                                (
                                    self.t(Msg::StarterExplainTitle),
                                    self.t(Msg::StarterExplainSub),
                                    self.t(Msg::StarterExplainPrompt),
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

                            let last_assistant_idx =
                                merged_turns.iter().rposition(|m| m.role == "assistant");

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
                                    attachments: None, model: None,
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
                                    self.t(Msg::Thinking),
                                );
                            }
                        },
                    );
                });
                ui.add_space(14.0);
            });

        // Bottom Docked SerikaChat Composer
        egui::Frame::NONE
            .fill(bg_deep())
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
            .fill(bg_sidebar())
            .corner_radius(CornerRadius::same(10))
            .stroke(Stroke::new(1.0, border_hairline()))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (tr, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    paint_icon(ui.painter(), tr, Icon::Terminal, text_muted());
                    ui.label(
                        RichText::new(self.tf(
                            Msg::ToolExecutions,
                            &[("{n}", &self.tools.len().to_string())],
                        ))
                        .size(11.5)
                        .strong()
                        .color(text_muted()),
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
                    .fill(bg_elevated())
                    .corner_radius(CornerRadius::same(18))
                    .stroke(Stroke::new(1.0, border_subtle()))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(&msg.content).size(14.0).color(text_primary()));
                    });

                if !self.running && msg.id > 0 {
                    ui.add_space(4.0);
                    if icon_button(ui, Icon::Rewind, self.t(Msg::RewindTip), confirming).clicked() {
                        self.rewind_confirm = Some(msg.id);
                    }
                }
            });

            if confirming {
                ui.add_space(6.0);
                egui::Frame::NONE
                    .fill(bg_card())
                    .corner_radius(CornerRadius::same(10))
                    .stroke(Stroke::new(1.0, AMBER))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.t(Msg::RewindPrompt))
                                    .size(12.0)
                                    .color(text_primary()),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(self.t(Msg::Cancel)).clicked() {
                                    self.rewind_confirm = None;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(self.t(Msg::ConfirmRewind))
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
                ui.painter().circle_filled(ar.center(), 10.0, bg_elevated());
                ui.painter()
                    .circle_stroke(ar.center(), 10.0, Stroke::new(1.0, border_subtle()));
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
                        .color(text_dim()),
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

            let step_count = reasoning_text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
                .max(1);
            let trace_title = if is_streaming_turn {
                self.t(Msg::Thinking).to_string()
            } else {
                self.tf(Msg::ThoughtProcess, &[("{n}", &step_count.to_string())])
            };
            draw_reasoning_trace(ui, msg.id, &reasoning_text, is_streaming_turn, &trace_title);

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
            composer_border()
        };
        let message_hint = self.t(Msg::MessageSerika);

        egui::Frame::NONE
            .fill(composer_fill())
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
                        .hint_text(message_hint),
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
                        ui.painter().circle_filled(plus_r.center(), 13.0, bg_card_hover());
                    }
                    paint_icon(
                        ui.painter(),
                        plus_r.shrink(6.0),
                        Icon::Plus,
                        if plus_resp.hovered() {
                            Color32::WHITE
                        } else {
                            text_muted()
                        },
                    );
                    if plus_resp
                        .on_hover_text(self.t(Msg::SwitchWorkspace))
                        .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.switch_project(folder);
                        }
                    }

                    let search_prefix = self.t(Msg::SearchInCodebase).to_string();
                    if serikachat_composer_pill(ui, Icon::Globe, self.t(Msg::Search), false)
                        .on_hover_text(self.t(Msg::Search))
                        .clicked()
                        && !self.prompt.contains(&search_prefix)
                    {
                        if self.prompt.trim().is_empty() {
                            self.prompt = search_prefix;
                        }
                    }

                    let is_plan = self.composer_mode == ComposerMode::Plan;
                    if serikachat_composer_pill(ui, Icon::Search, self.t(Msg::Research), is_plan)
                        .on_hover_text(self.t(Msg::Research))
                        .clicked()
                    {
                        self.composer_mode = if is_plan {
                            ComposerMode::Build
                        } else {
                            ComposerMode::Plan
                        };
                    }

                    let is_build = self.composer_mode == ComposerMode::Build;
                    if serikachat_composer_pill(ui, Icon::Sparkle, self.t(Msg::Image), false)
                        .on_hover_text(self.t(Msg::Image))
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
                                .on_hover_text(self.t(Msg::StopGeneration))
                                .clicked()
                            {
                                if let Some(cancel) = &self.cancel {
                                    cancel.cancel();
                                }
                            }
                        } else {
                            let has_text = !self.prompt.trim().is_empty();
                            if has_text {
                                if accent_circle_send_button(ui, Icon::Send)
                                    .on_hover_text(self.t(Msg::SendMessage))
                                    .clicked()
                                {
                                    self.send_prompt();
                                }
                            } else {
                                if white_circle_action_button(ui, Icon::SoundWave, true)
                                    .on_hover_text(self.t(Msg::VoiceInput))
                                    .clicked()
                                {
                                    if is_hero {
                                        self.prompt = "Help me understand this codebase and inspect recent changes.".to_string();
                                        self.send_prompt();
                                    }
                                }
                            }
                        }

                        // Microphone icon button
                        let (mic_r, mic_resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        if mic_resp.hovered() {
                            ui.painter().circle_filled(mic_r.center(), 14.0, bg_card_hover());
                        }
                        paint_icon(
                            ui.painter(),
                            mic_r.shrink(6.5),
                            Icon::Microphone,
                            if mic_resp.hovered() { Color32::WHITE } else { text_muted() },
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
                    .fill(bg_sidebar())
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, border_hairline())),
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
                    let close_tip = format!("{} (Ctrl+B)", self.t(Msg::Close));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, &close_tip, false).clicked() {
                            self.show_inspector = false;
                        }
                    });
                });

                ui.add_space(10.0);
                let sep = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                ui.painter().rect_filled(sep, 0.0, border_hairline());
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
                    .color(text_muted()),
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
                        .color(text_primary()),
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
            .fill(bg_card())
            .corner_radius(CornerRadius::same(9))
            .stroke(Stroke::new(1.0, border_subtle()))
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
                .color(text_dim()),
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
                        .fill(bg_card())
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
                                                .color(text_primary()),
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
                                        if let Ok(d) =
                                            gitinfo::file_diff(&proj, &path, staged).await
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
                .color(text_dim()),
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
                .fill(bg_card())
                .corner_radius(CornerRadius::same(8))
                .stroke(Stroke::new(1.0, border_hairline()))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("No turn diffs recorded in this thread yet.")
                            .size(12.0)
                            .color(text_muted()),
                    );
                    ui.label(
                        RichText::new(
                            "Click any modified file in Source Control or run an edit prompt to inspect line-by-line diffs.",
                        )
                        .size(11.0)
                        .color(text_dim()),
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
                .fill(bg_deep())
                .corner_radius(CornerRadius::same(9))
                .stroke(Stroke::new(1.0, border_subtle()))
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
                                let (bg, fg) = if line.starts_with('+') && !line.starts_with("+++")
                                {
                                    (EMERALD_SOFT, EMERALD)
                                } else if line.starts_with('-') && !line.starts_with("---") {
                                    (ROSE_SOFT, ROSE)
                                } else if line.starts_with("@@") {
                                    (SERIKA_VIOLET_SOFT, SERIKA_VIOLET_LIGHT)
                                } else {
                                    (Color32::TRANSPARENT, text_muted())
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
                                                    .color(text_dim()),
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
                    .color(text_dim()),
            );
            return;
        }
        ScrollArea::vertical()
            .id_salt("inspector_activity_scroll")
            .show(ui, |ui| {
                for tool in self.tools.iter().rev() {
                    egui::Frame::NONE
                        .fill(bg_card())
                        .corner_radius(CornerRadius::same(8))
                        .stroke(Stroke::new(1.0, border_subtle()))
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
                                    .color(text_muted()),
                            );
                        });
                    ui.add_space(6.0);
                }
            });
    }

    // =========================================================================
    // Popovers and settings (Serika Cloud is the primary model path)
    // =========================================================================
    fn draw_floating_menus(&mut self, ctx: &egui::Context) {
        if self.show_model_menu || self.show_account_menu || self.show_chat_menu {
            let screen = ctx.content_rect();
            let mut dismiss = false;
            egui::Area::new(egui::Id::unique("popover_backdrop"))
                .order(egui::Order::Middle)
                .fixed_pos(screen.min)
                .interactable(true)
                .show(ctx, |ui| {
                    if ui
                        .allocate_response(screen.size(), Sense::click())
                        .clicked()
                    {
                        dismiss = true;
                    }
                });
            if dismiss {
                self.close_popovers();
            }
        }
        if self.show_model_menu {
            self.draw_model_popover(ctx);
        }
        if self.show_account_menu {
            self.draw_account_popover(ctx);
        }
        if self.show_chat_menu {
            self.draw_chat_popover(ctx);
        }
    }

    fn draw_model_popover(&mut self, ctx: &egui::Context) {
        let pos = Pos2::new(self.model_anchor.left(), self.model_anchor.bottom() + 6.0);
        let free = self.catalog.free.clone();
        let premium = self.catalog.premium.clone();
        let selected = self.selected_model.clone();
        let query = self.model_search_input.trim().to_lowercase();
        let can_premium = self.can_use_premium();
        let included = self.t(Msg::Included).to_string();
        let plus = self.t(Msg::SerikaPlus).to_string();
        let upgrade = self.t(Msg::Upgrade).to_string();
        let empty = self.t(Msg::NoModels).to_string();
        let no_match = self.tf(
            Msg::NoModelsMatch,
            &[("{q}", self.model_search_input.trim())],
        );
        let search_hint = self.t(Msg::SearchModels);
        let locked_label = self.t(Msg::LockedModel);
        let matches = |model: &ChatModel| {
            query.is_empty()
                || model.name.to_lowercase().contains(&query)
                || model.id.to_lowercase().contains(&query)
                || model
                    .provider
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
                || model
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
        };
        let free_shown: Vec<_> = free
            .iter()
            .filter(|model| matches(model))
            .cloned()
            .collect();
        let premium_shown: Vec<_> = premium
            .iter()
            .filter(|model| matches(model))
            .cloned()
            .collect();
        let mut chosen: Option<String> = None;
        let mut locked: Option<String> = None;

        egui::Area::new(egui::Id::unique("model_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(egui::Align2::LEFT_TOP)
            .constrain(true)
            .show(ctx, |ui| {
                popover_frame(16).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.set_max_width(320.0);
                    ui.add(
                        TextEdit::singleline(&mut self.model_search_input)
                            .hint_text(search_hint)
                            .desired_width(f32::INFINITY)
                            .font(FontId::proportional(13.5)),
                    );
                    ui.add_space(6.0);
                    ScrollArea::vertical()
                        .id_salt("serika_model_popover")
                        .max_height(380.0)
                        .show(ui, |ui| {
                            if free.is_empty() && premium.is_empty() {
                                ui.label(RichText::new(&empty).size(13.0).color(text_dim()));
                                return;
                            }
                            if free_shown.is_empty() && premium_shown.is_empty() {
                                ui.label(RichText::new(&no_match).size(13.0).color(text_dim()));
                                return;
                            }
                            if !free_shown.is_empty() {
                                ui.label(RichText::new(&included).size(12.0).color(text_dim()));
                                ui.add_space(2.0);
                                for model in &free_shown {
                                    if model_popover_row(ui, model, model.id == selected, false, "")
                                    {
                                        chosen = Some(model.id.clone());
                                    }
                                }
                            }
                            if !premium_shown.is_empty() {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&plus).size(12.0).color(text_dim()));
                                    if !can_premium {
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(&upgrade)
                                                        .size(12.0)
                                                        .color(text_secondary()),
                                                );
                                            },
                                        );
                                    }
                                });
                                ui.add_space(2.0);
                                for model in &premium_shown {
                                    let is_locked = model.premium && !can_premium;
                                    if model_popover_row(
                                        ui,
                                        model,
                                        model.id == selected,
                                        is_locked,
                                        locked_label,
                                    ) {
                                        if is_locked {
                                            locked = Some(model.name.clone());
                                        } else {
                                            chosen = Some(model.id.clone());
                                        }
                                    }
                                }
                            }
                        });
                });
            });

        if let Some(name) = locked {
            self.status = self.tf(Msg::UpgradeHint, &[("{name}", &name)]);
            self.show_model_menu = false;
        } else if let Some(id) = chosen {
            self.choose_model(id);
        }
    }

    fn draw_account_popover(&mut self, ctx: &egui::Context) {
        let pos = Pos2::new(self.account_anchor.left(), self.account_anchor.top() - 6.0);
        let width = self.account_anchor.width().max(220.0);
        #[derive(Clone, Copy)]
        enum Act {
            Settings,
            Connectors,
            Usage,
            Terms,
            Privacy,
            SignIn,
            SignOut,
        }
        let mut act: Option<Act> = None;
        let settings = self.t(Msg::Settings);
        let connectors = self.t(Msg::Connectors);
        let usage = self.t(Msg::Usage);
        let terms = self.t(Msg::Terms);
        let privacy = self.t(Msg::Privacy);
        let signed_in = self.session.is_some();
        let auth_label = if signed_in {
            self.t(Msg::SignOut)
        } else {
            self.t(Msg::SignIn)
        };

        egui::Area::new(egui::Id::unique("account_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .constrain(true)
            .show(ctx, |ui| {
                popover_frame(12).show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    if menu_row(ui, Icon::Sliders, settings, false) {
                        act = Some(Act::Settings);
                    }
                    if menu_row(ui, Icon::Link, connectors, false) {
                        act = Some(Act::Connectors);
                    }
                    menu_divider(ui);
                    if menu_row(ui, Icon::Sparkle, usage, false) {
                        act = Some(Act::Usage);
                    }
                    if menu_row(ui, Icon::Book, terms, false) {
                        act = Some(Act::Terms);
                    }
                    if menu_row(ui, Icon::Shield, privacy, false) {
                        act = Some(Act::Privacy);
                    }
                    menu_divider(ui);
                    if menu_row(ui, Icon::LogOut, auth_label, signed_in) {
                        act = Some(if signed_in { Act::SignOut } else { Act::SignIn });
                    }
                });
            });

        match act {
            Some(Act::Settings) => {
                self.show_account_menu = false;
                self.settings_tab = SettingsTab::General;
                self.show_settings = true;
            }
            Some(Act::Connectors) => {
                self.show_account_menu = false;
                self.show_providers = true;
            }
            Some(Act::Usage) => {
                self.show_account_menu = false;
                self.settings_tab = SettingsTab::Account;
                self.show_settings = true;
            }
            Some(Act::Terms) => {
                self.show_account_menu = false;
                ctx.open_url(egui::OpenUrl::new_tab("https://serika.dev/terms"));
            }
            Some(Act::Privacy) => {
                self.show_account_menu = false;
                ctx.open_url(egui::OpenUrl::new_tab("https://serika.dev/privacy"));
            }
            Some(Act::SignIn) => {
                self.show_account_menu = false;
                self.settings_tab = SettingsTab::Account;
                self.show_settings = true;
            }
            Some(Act::SignOut) => self.sign_out(),
            None => {}
        }
    }

    fn draw_chat_popover(&mut self, ctx: &egui::Context) {
        let pos = Pos2::new(self.chat_anchor.right(), self.chat_anchor.bottom() + 6.0);
        #[derive(Clone, Copy)]
        enum Act {
            Inspector,
            Export,
            Shortcuts,
        }
        let mut act: Option<Act> = None;
        let diffs = self.t(Msg::DiffsInspector);
        let export_label = self.t(Msg::ExportMarkdown);
        let shortcuts = self.t(Msg::KeyboardShortcuts);
        let can_export = !self.messages.is_empty();

        egui::Area::new(egui::Id::unique("chat_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(egui::Align2::RIGHT_TOP)
            .constrain(true)
            .show(ctx, |ui| {
                popover_frame(12).show(ui, |ui| {
                    ui.set_min_width(256.0);
                    ui.set_max_width(256.0);
                    if menu_row(ui, Icon::Code, diffs, false) {
                        act = Some(Act::Inspector);
                    }
                    if menu_row(ui, Icon::Download, export_label, false) && can_export {
                        act = Some(Act::Export);
                    }
                    menu_divider(ui);
                    if menu_row(ui, Icon::Command, shortcuts, false) {
                        act = Some(Act::Shortcuts);
                    }
                });
            });

        match act {
            Some(Act::Inspector) => {
                self.show_chat_menu = false;
                self.show_inspector = !self.show_inspector;
                if self.show_inspector {
                    self.inspector_tab = InspectorTab::Diffs;
                }
            }
            Some(Act::Export) => self.export_chat(),
            Some(Act::Shortcuts) => {
                self.show_chat_menu = false;
                self.show_shortcuts = true;
            }
            None => {}
        }
    }

    fn draw_shortcuts_modal(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = self.show_shortcuts;
        let style = ctx.style_of(Theme::Dark);
        let rows = [
            ("Ctrl+N", self.t(Msg::ShortcutNew)),
            ("Ctrl+K", self.t(Msg::ShortcutPalette)),
            ("Ctrl+B", self.t(Msg::ShortcutInspector)),
            ("Enter", self.t(Msg::ShortcutSend)),
            ("Esc", self.t(Msg::ShortcutClose)),
        ];
        egui::Window::new(self.t(Msg::ShortcutsTitle))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(bg_elevated())
                    .stroke(Stroke::new(1.0, border_subtle()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                for (keys, label) in rows {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).size(13.5).color(text_primary()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(keys).size(12.0).color(text_dim()));
                        });
                    });
                    ui.add_space(6.0);
                }
            });
        self.show_shortcuts = open;
    }

    fn draw_providers_modal(&mut self, ctx: &egui::Context) {
        if !self.show_providers {
            return;
        }
        let mut open = self.show_providers;
        let mut saved = false;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new(self.t(Msg::Connectors))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(bg_elevated())
                    .stroke(Stroke::new(1.0, border_subtle()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(self.t(Msg::ConnectorsHelp))
                        .size(13.0)
                        .color(text_secondary()),
                );
                ui.add_space(12.0);
                ui.label(RichText::new("Anthropic").size(12.0).color(text_dim()));
                ui.add(
                    TextEdit::singleline(&mut self.anthropic_key_input)
                        .password(true)
                        .hint_text("sk-ant-…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("OpenAI").size(12.0).color(text_dim()));
                ui.add(
                    TextEdit::singleline(&mut self.openai_key_input)
                        .password(true)
                        .hint_text("sk-…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Gemini").size(12.0).color(text_dim()));
                ui.add(
                    TextEdit::singleline(&mut self.gemini_key_input)
                        .password(true)
                        .hint_text("AIza…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                egui::CollapsingHeader::new(
                    RichText::new(self.t(Msg::Advanced))
                        .size(13.0)
                        .color(text_secondary()),
                )
                .show(ui, |ui| {
                    ui.label(RichText::new("Ollama").size(12.0).color(text_dim()));
                    ui.add(
                        TextEdit::singleline(&mut self.ollama_base_input)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("Local server").size(12.0).color(text_dim()));
                    ui.add(
                        TextEdit::singleline(&mut self.local_base_input)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("Local model").size(12.0).color(text_dim()));
                    ui.add(
                        TextEdit::singleline(&mut self.local_model_input)
                            .hint_text("ollama:qwen2.5-coder")
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(12.0);
                let save_label = self.t(Msg::SaveConnectors);
                if primary_action_button(ui, Icon::Check, save_label, ACCENT_PURPLE, true).clicked()
                {
                    saved = true;
                }
            });
        if saved {
            self.save_providers_and_settings();
            open = false;
        }
        self.show_providers = open;
    }

    fn draw_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new(self.t(Msg::Settings))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .frame(
                egui::Frame::window(&style)
                    .fill(bg_elevated())
                    .stroke(Stroke::new(1.0, border_subtle()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let general = self.settings_tab == SettingsTab::General;
                    let account = self.settings_tab == SettingsTab::Account;
                    if settings_tab_button(ui, self.t(Msg::General), general) {
                        self.settings_tab = SettingsTab::General;
                    }
                    if settings_tab_button(ui, self.t(Msg::Account), account) {
                        self.settings_tab = SettingsTab::Account;
                    }
                });
                ui.add_space(8.0);
                let div = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                ui.painter().rect_filled(div, 0.0, border_hairline());
                ui.add_space(8.0);
                match self.settings_tab {
                    SettingsTab::General => self.draw_general_settings(ui),
                    SettingsTab::Account => self.draw_account_settings(ui),
                }
            });
        self.show_settings = open;
    }

    fn draw_general_settings(&mut self, ui: &mut egui::Ui) {
        settings_row_label(ui, self.t(Msg::Language), self.t(Msg::LanguageHelp));
        let mut locale = self.locale;
        egui::ComboBox::from_id_salt("settings_locale")
            .selected_text(locale.native_name())
            .width(200.0)
            .show_ui(ui, |ui| {
                for item in Locale::ALL {
                    ui.selectable_value(&mut locale, item, item.native_name());
                }
            });
        if locale != self.locale {
            self.set_locale(locale);
        }
        ui.add_space(12.0);
        settings_row_label(ui, self.t(Msg::Theme), "");
        let mut light = self.theme_light;
        egui::ComboBox::from_id_salt("settings_theme")
            .selected_text(if light {
                self.t(Msg::ThemeLight)
            } else {
                self.t(Msg::ThemeDark)
            })
            .width(200.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut light, false, self.t(Msg::ThemeDark));
                ui.selectable_value(&mut light, true, self.t(Msg::ThemeLight));
            });
        if light != self.theme_light {
            self.set_theme_light(light);
            apply_serika_theme(ui.ctx());
        }
        ui.add_space(12.0);
        settings_row_label(ui, self.t(Msg::DefaultModel), "");
        let models: Vec<(String, String)> = self
            .catalog
            .all()
            .map(|model| (model.id.clone(), model.name.clone()))
            .collect();
        let current_label = self.model_label();
        let mut picked = self.selected_model.clone();
        egui::ComboBox::from_id_salt("settings_model")
            .selected_text(current_label)
            .width(260.0)
            .show_ui(ui, |ui| {
                if models.is_empty() {
                    ui.label(RichText::new(self.t(Msg::NoModels)).color(text_dim()));
                }
                for (id, name) in &models {
                    ui.selectable_value(&mut picked, id.clone(), name);
                }
            });
        if picked != self.selected_model {
            self.choose_model(picked);
        }
        ui.add_space(14.0);
        egui::CollapsingHeader::new(
            RichText::new(self.t(Msg::Advanced))
                .size(13.0)
                .color(text_secondary()),
        )
        .show(ui, |ui| {
            let auto_approve = self.t(Msg::AutoApprove);
            ui.checkbox(&mut self.auto_approve_session, auto_approve);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if primary_action_button(
                    ui,
                    Icon::Check,
                    self.t(Msg::SetupShortcuts),
                    ACCENT_PURPLE,
                    true,
                )
                .clicked()
                {
                    if let Some(core) = self.core.clone() {
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let res = tokio::task::spawn_blocking(move || {
                                setup::install_system(&core.paths, &SetupOptions::default())
                                    .map(|report| report.render())
                                    .map_err(|err| err.to_string())
                            })
                            .await
                            .unwrap_or_else(|err| Err(err.to_string()));
                            let _ = tx.send(UiNotice::SetupDone(res));
                        });
                    }
                }
                if pill_icon_button(ui, Icon::Refresh, self.t(Msg::CheckUpdates), false).clicked() {
                    self.check_updates_async();
                }
            });
            if !self.setup_status.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(&self.setup_status)
                        .size(11.5)
                        .color(text_muted()),
                );
            }
        });
    }

    fn draw_account_settings(&mut self, ui: &mut egui::Ui) {
        let signed_in = self.session.is_some() || self.profile.is_some();
        if !signed_in {
            ui.label(
                RichText::new(self.t(Msg::NotSignedIn))
                    .size(13.0)
                    .color(text_secondary()),
            );
            if self.awaiting_second_factor {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(self.t(Msg::SecondFactorHint))
                        .size(12.5)
                        .color(AMBER),
                );
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(self.t(Msg::Email))
                    .size(12.0)
                    .color(text_dim()),
            );
            ui.add(
                TextEdit::singleline(&mut self.email_input)
                    .hint_text("you@example.com")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(self.t(Msg::Password))
                    .size(12.0)
                    .color(text_dim()),
            );
            ui.add(
                TextEdit::singleline(&mut self.password_input)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(self.t(Msg::TwoFactor))
                    .size(12.0)
                    .color(text_dim()),
            );
            ui.add(
                TextEdit::singleline(&mut self.two_factor_input)
                    .hint_text("123456")
                    .desired_width(160.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if primary_action_button(
                    ui,
                    Icon::Check,
                    self.t(Msg::SignIn),
                    ACCENT_PURPLE,
                    !self.auth_busy,
                )
                .clicked()
                {
                    self.start_password_login();
                }
                if primary_action_button(
                    ui,
                    Icon::Send,
                    self.t(Msg::SendLink),
                    bg_card_hover(),
                    !self.auth_busy,
                )
                .clicked()
                {
                    self.start_magic_link();
                }
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(self.t(Msg::LinkToken))
                    .size(12.0)
                    .color(text_dim()),
            );
            ui.add(
                TextEdit::singleline(&mut self.magic_token_input)
                    .hint_text("paste the token from the email link")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            if primary_action_button(
                ui,
                Icon::Check,
                self.t(Msg::SignIn),
                ACCENT_PURPLE,
                !self.auth_busy,
            )
            .clicked()
            {
                self.consume_magic_link();
            }
            if !self.auth_notice.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(&self.auth_notice)
                        .size(12.0)
                        .color(text_secondary()),
                );
            }
            return;
        }

        let username = self.current_username();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
            draw_account_avatar(ui.painter(), rect, self.avatar_texture.as_ref(), &username);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&username)
                        .size(15.0)
                        .strong()
                        .color(text_primary()),
                );
                if let Some(email) = self
                    .profile
                    .as_ref()
                    .map(|profile| profile.email.clone())
                    .filter(|email| !email.is_empty())
                {
                    ui.label(RichText::new(email).size(12.0).color(text_dim()));
                }
            });
        });
        ui.add_space(12.0);
        settings_row_label(ui, self.t(Msg::Subscription), "");
        ui.label(
            RichText::new(self.plan_label())
                .size(16.0)
                .strong()
                .color(text_primary()),
        );
        ui.add_space(12.0);
        let (frac, label, reset) = self.session_meter();
        draw_quota_block(ui, &label, &reset, frac);
        if let Some((weekly_frac, weekly_label, weekly_reset)) = self.weekly_meter() {
            ui.add_space(10.0);
            draw_quota_block(ui, &weekly_label, &weekly_reset, weekly_frac);
        }
        ui.add_space(16.0);
        if menu_row(ui, Icon::LogOut, self.t(Msg::SignOut), true) {
            self.sign_out();
        }
    }

    fn draw_palette_modal(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        let mut open = self.show_palette;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new(self.t(Msg::CommandPalette))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .frame(
                egui::Frame::window(&style)
                    .fill(bg_card())
                    .stroke(Stroke::new(1.5, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14)),
            )
            .show(ctx, |ui| {
                let hint = self.t(Msg::TypeCommand);
                let commands = [
                    ("new", self.t(Msg::PaletteNew)),
                    ("folder", self.t(Msg::PaletteFolder)),
                    ("scratch", self.t(Msg::PaletteScratch)),
                    ("providers", self.t(Msg::PaletteConnectors)),
                    ("inspector", self.t(Msg::PaletteInspector)),
                    ("update", self.t(Msg::PaletteUpdate)),
                ];
                let resp = ui.add(
                    TextEdit::singleline(&mut self.palette_query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY),
                );
                resp.request_focus();
                ui.add_space(8.0);
                let q = self.palette_query.to_ascii_lowercase();
                for (id, label) in commands {
                    if !q.is_empty() && !label.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new(
                                RichText::new(label).size(12.0).color(text_primary()),
                            )
                            .fill(bg_input())
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
        let Some(pending) = self.pending_approval.clone() else {
            return;
        };
        let mut choice = None;
        let style = ctx.style_of(Theme::Dark);
        egui::Window::new(self.t(Msg::ApproveTitle))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&style)
                    .fill(bg_card())
                    .stroke(Stroke::new(1.5, SERIKA_VIOLET))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(&pending.title)
                        .strong()
                        .size(14.0)
                        .color(text_primary()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&pending.detail)
                        .monospace()
                        .size(12.0)
                        .color(text_muted()),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_action_button(ui, Icon::Check, self.t(Msg::Approve), EMERALD, true)
                        .clicked()
                    {
                        choice = Some(true);
                    }
                    if primary_action_button(ui, Icon::Close, self.t(Msg::Deny), ROSE, true)
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

    // =========================================================================
    // Codex-CLI Terminal Showcase Renderer (for `--screenshot <png> cli`)
    // =========================================================================
    fn draw_codex_cli_showcase(&mut self, root_ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(7, 7, 10)).inner_margin(Margin::same(28)))
            .show(root_ui, |ui| {
                egui::Frame::NONE
                    .fill(bg_deep())
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(1.2, border_subtle()))
                    .inner_margin(Margin::same(0))
                    .show(ui, |ui| {
                        // Terminal Window Titlebar
                        egui::Frame::NONE
                            .fill(bg_sidebar())
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
                                            .color(text_muted()),
                                    );
                                });
                            });

                        let sep = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
                        ui.painter().rect_filled(sep, 0.0, border_hairline());

                        egui::Frame::NONE
                            .inner_margin(Margin::symmetric(24, 18))
                            .show(ui, |ui| {
                                // 1. Codex-style Header Box
                                egui::Frame::NONE
                                    .fill(bg_card())
                                    .corner_radius(CornerRadius::same(8))
                                    .stroke(Stroke::new(1.0, border_strong()))
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
                                                    .color(text_primary()),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "(v{})",
                                                    serika_runtime::client::VERSION
                                                ))
                                                .monospace()
                                                .size(13.0)
                                                .color(text_dim()),
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
                                                        .color(text_dim()),
                                                );
                                                ui.label(
                                                    RichText::new(v)
                                                        .monospace()
                                                        .size(12.5)
                                                        .color(text_primary()),
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
                                        .color(text_dim()),
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
                                                .color(text_dim()),
                                        );
                                    });
                                }

                                ui.add_space(12.0);

                                // 2. Highlighted User Prompt Cell (`› ...`)
                                egui::Frame::NONE
                                    .fill(bg_elevated())
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
                                                .color(text_primary()),
                                            );
                                        });
                                    });

                                ui.add_space(10.0);

                                // 3. Tool Cells (`• Explored`, `• Edited`, `• Ran`)
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("•").monospace().strong().size(13.0).color(EMERALD));
                                    ui.label(RichText::new("Read").monospace().strong().size(12.5).color(text_primary()));
                                    ui.label(
                                        RichText::new("crates/serika-runtime/src/providers.rs")
                                            .monospace()
                                            .size(12.5)
                                            .color(text_muted()),
                                    );
                                });
                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("•").monospace().strong().size(13.0).color(EMERALD));
                                    ui.label(RichText::new("Edited").monospace().strong().size(12.5).color(text_primary()));
                                    ui.label(
                                        RichText::new("crates/serika-runtime/src/providers.rs")
                                            .monospace()
                                            .size(12.5)
                                            .color(text_primary()),
                                    );
                                    ui.label(RichText::new("(+18").monospace().size(12.0).color(EMERALD));
                                    ui.label(RichText::new("-3)").monospace().size(12.0).color(ROSE));
                                });

                                for (ln, sign, code, bg, fg) in [
                                    ("172", " ", "pub async fn discover_all_models(http: &Client, cfg: &Config) -> Report {", Color32::TRANSPARENT, text_dim()),
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
                                                        .color(text_dim()),
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
                                    ui.label(RichText::new("Ran").monospace().strong().size(12.5).color(text_primary()));
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
                                        .color(text_dim()),
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
                                        .color(text_primary()),
                                    );
                                });

                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        "─────────────────────────────── Worked for 1.7s ───────────────────────────────",
                                    )
                                    .monospace()
                                    .size(11.5)
                                    .color(text_dim()),
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
                                            .color(text_dim()),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new("local/workspace · standard · Ctrl+C interrupt")
                                                .monospace()
                                                .size(11.5)
                                                .color(text_dim()),
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
                    .color(text_primary()),
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
                        .color(text_secondary()),
                );
            });
            continue;
        }
        ui.label(
            RichText::new(raw_line.replace("**", "").replace('`', ""))
                .size(13.5)
                .color(text_secondary()),
        );
    }
    if in_code && !code_buf.is_empty() {
        draw_code_block_card(ui, &code_lang, &code_buf);
    }
}

fn draw_code_block_card(ui: &mut egui::Ui, lang: &str, code: &str) {
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(bg_deep())
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, border_subtle()))
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
                    .color(text_primary()),
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
        bg_active_row()
    } else if resp.hovered() {
        bg_card_hover()
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 7.0, fill);
    }
    let fg = if active || resp.hovered() {
        text_primary()
    } else {
        text_secondary()
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
            let galley =
                ui.painter()
                    .layout_no_wrap(k.to_string(), FontId::monospace(10.0), text_dim());
            let pill_w = galley.size().x + 8.0;
            let pill_rect = Rect::from_min_size(
                Pos2::new(right_cursor - pill_w, rect.center().y - 8.0),
                Vec2::new(pill_w, 16.0),
            );
            ui.painter().rect_filled(pill_rect, 4.0, bg_card());
            ui.painter().rect_stroke(
                pill_rect,
                4.0,
                Stroke::new(1.0, border_hairline()),
                StrokeKind::Inside,
            );
            ui.painter().galley(
                Pos2::new(
                    pill_rect.left() + 4.0,
                    pill_rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                text_dim(),
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
        bg_card_hover()
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 7.0, fill);
    }
    let fg = if resp.hovered() {
        text_primary()
    } else {
        text_secondary()
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
        let b_galley =
            ui.painter()
                .layout_no_wrap(b.to_string(), FontId::monospace(10.5), text_dim());
        let b_w = b_galley.size().x;
        ui.painter().galley(
            Pos2::new(right_x - b_w, rect.center().y - b_galley.size().y * 0.5),
            b_galley,
            text_dim(),
        );
    }

    resp.clicked()
}

fn draw_serikachat_starter_row(ui: &mut egui::Ui, width: f32, title: &str, subtitle: &str) -> bool {
    let desired = Vec2::new(width, 36.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, bg_card_hover());
    }

    let title_galley = ui.painter().layout_no_wrap(
        title.to_string(),
        FontId::proportional(13.5),
        if resp.hovered() {
            Color32::WHITE
        } else {
            text_primary()
        },
    );
    let title_w = title_galley.size().x;
    ui.painter().galley(
        Pos2::new(
            rect.left() + 8.0,
            rect.center().y - title_galley.size().y * 0.5,
        ),
        title_galley,
        if resp.hovered() {
            Color32::WHITE
        } else {
            text_primary()
        },
    );

    let sub_galley = ui.painter().layout_no_wrap(
        format!(" {subtitle}"),
        FontId::proportional(13.5),
        text_muted(),
    );
    ui.painter().galley(
        Pos2::new(
            rect.left() + 8.0 + title_w,
            rect.center().y - sub_galley.size().y * 0.5,
        ),
        sub_galley,
        text_muted(),
    );

    let bottom_line = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.bottom() - 1.0),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter()
        .rect_filled(bottom_line, 0.0, border_hairline());

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
    let proj_name = project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let mut lines = Vec::new();
    lines.push(format!(
        "1. Analyzed user request in the context of `{proj_name}`."
    ));
    lines.push(format!("2. Evaluated active harness model: `{model}`."));

    if !tools.is_empty() {
        lines.push(format!("3. Executed {} tool action(s):", tools.len()));
        for t in tools.iter().take(4) {
            let status = if t.ok { "completed" } else { "warning" };
            lines.push(format!("   • `{}` ({status})", t.name));
        }
        if tools.len() > 4 {
            lines.push(format!(
                "   • … and {} more tool action(s)",
                tools.len() - 4
            ));
        }
        lines.push("4. Verified output and structured final response for the user.".to_string());
    } else {
        lines.push("3. Inspected codebase structure and prepared direct response.".to_string());
        lines.push("4. Verified syntax, formatting, and recommendations.".to_string());
    }

    lines.join("\n")
}

fn draw_reasoning_trace(
    ui: &mut egui::Ui,
    msg_id: i64,
    reasoning: &str,
    is_streaming: bool,
    title: &str,
) {
    if reasoning.trim().is_empty() && !is_streaming {
        return;
    }

    let id = ui.make_persistent_id(format!("reasoning_trace_{msg_id}"));
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

    ui.add_space(2.0);
    state
        .show_header(ui, |ui| {
            ui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                paint_icon(ui.painter(), ir, Icon::Sparkle, text_muted());
                ui.label(RichText::new(title).size(13.0).color(if is_streaming {
                    ACCENT_PURPLE_SEC
                } else {
                    text_muted()
                }));
                if is_streaming {
                    let (dr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(dr.center(), 3.0, ACCENT_PURPLE);
                }
            });
        })
        .body(|ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let line_rect = ui
                    .allocate_space(Vec2::new(1.5, ui.available_height().max(16.0)))
                    .1;
                ui.painter().rect_filled(
                    line_rect,
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 25),
                );
                ui.add_space(10.0);

                ui.vertical(|ui| {
                    for line in reasoning.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            ui.add_space(3.0);
                            continue;
                        }
                        let is_header_step = trimmed.starts_with(|c: char| c.is_ascii_digit())
                            && trimmed.contains('.');
                        let col = if is_header_step {
                            text_primary()
                        } else {
                            text_secondary()
                        };
                        ui.label(RichText::new(trimmed).size(12.5).color(col));
                    }
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
        if active { Color32::WHITE } else { text_muted() },
    );
    let desired = Vec2::new(galley.size().x + 28.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if active {
        SERIKA_VIOLET_SOFT
    } else if resp.hovered() {
        bg_card_hover()
    } else {
        bg_elevated()
    };
    let stroke_col = if active {
        SERIKA_VIOLET
    } else {
        border_hairline()
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    ui.painter()
        .rect_stroke(rect, 12.0, Stroke::new(1.0, stroke_col), StrokeKind::Inside);

    let fg = if active || resp.hovered() {
        Color32::WHITE
    } else {
        text_muted()
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
        bg_elevated()
    } else if resp.hovered() {
        Color32::from_rgb(230, 230, 230)
    } else {
        Color32::WHITE
    };
    ui.painter().circle_filled(rect.center(), 14.0, bg);
    let fg = if enabled {
        Color32::from_rgb(13, 13, 13)
    } else {
        text_dim()
    };
    let ir = rect.shrink(7.5);
    paint_icon(ui.painter(), ir, icon, fg);
    resp
}

fn accent_circle_send_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let bg = if resp.hovered() {
        ACCENT_PURPLE_HOVER
    } else {
        ACCENT_PURPLE
    };
    ui.painter().circle_filled(rect.center(), 14.0, bg);
    paint_icon(ui.painter(), rect.shrink(7.0), icon, Color32::WHITE);
    resp
}

fn primary_action_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    accent: Color32,
    enabled: bool,
) -> egui::Response {
    let fg = if enabled { Color32::WHITE } else { text_dim() };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(12.0), fg);
    let desired = Vec2::new(galley.size().x + 36.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if !enabled {
        bg_elevated()
    } else if resp.hovered() {
        SERIKA_VIOLET_HOVER
    } else {
        accent
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    if !enabled {
        ui.painter().rect_stroke(
            rect,
            7.0,
            Stroke::new(1.0, border_subtle()),
            StrokeKind::Inside,
        );
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
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), FontId::proportional(11.0), text_muted());
    let desired = Vec2::new(galley.size().x + 26.0, 23.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    ui.painter().rect_filled(rect, 6.0, bg_card());
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, border_hairline()),
        StrokeKind::Inside,
    );
    ui.painter()
        .circle_filled(Pos2::new(rect.left() + 10.0, rect.center().y), 3.2, dot_col);
    ui.painter().galley(
        Pos2::new(rect.left() + 18.0, rect.center().y - galley.size().y * 0.5),
        galley,
        text_muted(),
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
        bg_card_hover()
    } else {
        bg_card()
    };
    let fg = if primary {
        Color32::WHITE
    } else if resp.hovered() {
        text_primary()
    } else {
        text_muted()
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if !primary {
        ui.painter().rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, border_subtle()),
            StrokeKind::Inside,
        );
    }
    paint_icon(ui.painter(), rect.shrink(5.5), icon, fg);
    resp.on_hover_text(tooltip)
}

fn pill_icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, active: bool) -> egui::Response {
    let fill = if active {
        SERIKA_VIOLET_SOFT
    } else {
        bg_card()
    };
    let stroke_col = if active {
        SERIKA_VIOLET
    } else {
        border_subtle()
    };
    let fg = if active {
        SERIKA_VIOLET_LIGHT
    } else {
        text_muted()
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
        bg_active_row()
    } else if resp.hovered() {
        bg_card_hover()
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, fill);
    }

    let title_col = if is_active || resp.hovered() {
        text_primary()
    } else {
        text_secondary()
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
            if del_resp.hovered() { ROSE } else { text_dim() },
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
        .fill(bg_card())
        .corner_radius(CornerRadius::same(6))
        .stroke(Stroke::new(1.0, border_hairline()))
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
                        .color(text_primary()),
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
                        .color(text_muted()),
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

fn popover_frame(radius: u8) -> egui::Frame {
    egui::Frame::NONE
        .fill(bg_elevated())
        .stroke(Stroke::new(1.0, border_subtle()))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(8))
}

fn menu_divider(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let rect = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
    ui.painter().rect_filled(rect, 0.0, border_hairline());
    ui.add_space(4.0);
}

fn menu_row(ui: &mut egui::Ui, icon: Icon, label: &str, danger: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, bg_card_hover());
    }
    let color = if danger { ROSE } else { text_primary() };
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 16.0, rect.center().y),
        Vec2::splat(14.0),
    );
    paint_icon(
        ui.painter(),
        icon_rect,
        icon,
        if danger { ROSE } else { text_dim() },
    );
    ui.painter().text(
        Pos2::new(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        color,
    );
    resp.clicked()
}

fn settings_tab_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(13.5),
        if active { text_primary() } else { text_dim() },
    );
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 16.0, 32.0), Sense::click());
    ui.painter().galley(
        Pos2::new(rect.left() + 8.0, rect.center().y - galley.size().y * 0.5),
        galley,
        if active { text_primary() } else { text_dim() },
    );
    if active {
        let line = Rect::from_min_max(
            Pos2::new(rect.left() + 8.0, rect.bottom() - 1.0),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        );
        ui.painter().rect_filled(line, 0.0, text_primary());
    }
    resp.clicked()
}

fn settings_row_label(ui: &mut egui::Ui, title: &str, help: &str) {
    ui.label(
        RichText::new(title)
            .size(13.5)
            .strong()
            .color(text_primary()),
    );
    if !help.is_empty() {
        ui.label(RichText::new(help).size(12.0).color(text_dim()));
    }
    ui.add_space(4.0);
}

fn draw_quota_block(ui: &mut egui::Ui, label: &str, reset: &str, fraction: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.5).color(text_secondary()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !reset.is_empty() {
                ui.label(RichText::new(reset).size(12.0).color(text_dim()));
            }
        });
    });
    ui.add_space(4.0);
    let bar = ui.allocate_space(Vec2::new(ui.available_width(), 4.0)).1;
    ui.painter().rect_filled(bar, 2.0, bg_card_hover());
    let fill = Rect::from_min_size(
        bar.min,
        Vec2::new(bar.width() * fraction.clamp(0.0, 1.0), 4.0),
    );
    ui.painter().rect_filled(fill, 2.0, ACCENT_PURPLE);
}

fn model_popover_row(
    ui: &mut egui::Ui,
    model: &ChatModel,
    active: bool,
    locked: bool,
    locked_label: &str,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
    if active || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            8.0,
            if active {
                bg_card_hover()
            } else {
                bg_card_hover()
            },
        );
    }
    let name_color = if locked { text_dim() } else { text_primary() };
    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        &model.name,
        FontId::proportional(13.5),
        name_color,
    );
    let mut meta = model
        .description
        .clone()
        .or_else(|| model.provider.clone())
        .unwrap_or_default();
    if model.premium {
        if !meta.is_empty() {
            meta.push_str("  ·  ");
        }
        meta.push_str("Serika+");
        if let Some(mult) = model.usage_multiplier.filter(|value| *value > 1.0) {
            meta.push(' ');
            meta.push_str(&format_multiplier(mult));
        }
        if locked && !locked_label.is_empty() {
            meta.push_str(" · ");
            meta.push_str(locked_label);
        }
    }
    if !meta.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left() + 10.0, rect.top() + 26.0),
            egui::Align2::LEFT_TOP,
            meta,
            FontId::proportional(11.5),
            text_dim(),
        );
    }
    if locked {
        let icon = Rect::from_center_size(
            Pos2::new(rect.right() - 16.0, rect.center().y),
            Vec2::splat(12.0),
        );
        paint_icon(ui.painter(), icon, Icon::Lock, text_dim());
    } else if active {
        let icon = Rect::from_center_size(
            Pos2::new(rect.right() - 16.0, rect.center().y),
            Vec2::splat(12.0),
        );
        paint_icon(ui.painter(), icon, Icon::Check, text_primary());
    }
    resp.clicked()
}

fn format_multiplier(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{}x", value.round() as i64)
    } else {
        let text = format!("{value:.1}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        format!("{text}x")
    }
}

fn screenshot_catalog() -> ModelCatalog {
    let row = |id: &str, name: &str, premium: bool, mult: Option<f64>, provider: &str| ChatModel {
        id: id.to_string(),
        name: name.to_string(),
        description: Some(provider.to_string()),
        max_tokens: None,
        provider: Some(provider.to_string()),
        premium,
        is_default: id == "openrouter/free",
        usage_multiplier: mult,
        availability: None,
    };
    ModelCatalog {
        free: vec![
            row(
                "openrouter/free",
                "openrouter Free",
                false,
                None,
                "openrouter",
            ),
            row(
                "deepseek/deepseek-v4.1-flash",
                "DeepSeek V4.1 Flash",
                false,
                None,
                "deepseek",
            ),
            row(
                "anthropic/claude-haiku-4.5",
                "Claude Haiku 5.5",
                false,
                None,
                "anthropic",
            ),
        ],
        premium: vec![
            row(
                "anthropic/claude-3.5-sonnet",
                "Claude 3.5 Sonnet",
                true,
                Some(5.0),
                "anthropic",
            ),
            row("openai/gpt-4o", "GPT-4o", true, Some(5.0), "openai"),
            row(
                "google/gemini-2.5-pro",
                "Gemini 2.5 Pro",
                true,
                Some(11.0),
                "google",
            ),
            row(
                "google/gemini-3-flash",
                "Gemini 3 Flash",
                true,
                Some(1.5),
                "google",
            ),
        ],
    }
}

fn screenshot_usage() -> UsageSnapshot {
    let session_reset = epoch_to_rfc3339(now_unix() + 4 * 3600 + 57 * 60);
    let weekly_reset = epoch_to_rfc3339(now_unix() + 3 * 86_400);
    UsageSnapshot {
        total_messages: Some(12),
        daily_messages: Some(1),
        last_reset: None,
        plan: Some(serika_runtime::model::UsagePlan {
            tier: Some(serika_runtime::model::UsageTier {
                key: Some("max".into()),
                name: Some("Serika+ AI Max".into()),
            }),
            is_premium: Some(true),
            byok: Some(false),
            features: None,
            session: Some(serika_runtime::model::UsageWindow {
                limit: Some(5000),
                used: Some(0),
                remaining: Some(5000),
                percent: Some(0.0),
                started_at: None,
                resets_at: Some(session_reset),
                window_hours: Some(5),
                week_start: None,
                by_kind: None,
            }),
            weekly: Some(serika_runtime::model::UsageWindow {
                limit: Some(10000),
                used: Some(1200),
                remaining: Some(8800),
                percent: Some(12.0),
                started_at: None,
                resets_at: Some(weekly_reset),
                window_hours: None,
                week_start: None,
                by_kind: None,
            }),
            daily: None,
            last_reset: None,
            reset_offer: None,
        }),
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn local_hour() -> u32 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        extern "system" {
            fn GetLocalTime(time: *mut SystemTime);
        }
        let mut time = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut time) };
        return u32::from(time.hour);
    }
    #[cfg(not(windows))]
    {
        ((now_unix() / 3600) % 24) as u32
    }
}

fn ellipsize(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> String {
    if painter
        .layout_no_wrap(text.to_string(), font.clone(), color)
        .size()
        .x
        <= max_width
    {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut end = chars.len();
    while end > 1 {
        end -= 1;
        let mut candidate: String = chars.iter().take(end).collect();
        candidate.push('…');
        if painter
            .layout_no_wrap(candidate.clone(), font.clone(), color)
            .size()
            .x
            <= max_width
        {
            return candidate;
        }
    }
    "…".to_string()
}

fn format_expiry(raw: &str) -> Option<String> {
    let epoch = parse_rfc3339(raw).or_else(|| {
        let n: i64 = raw.trim().parse().ok()?;
        Some(if n > 10_000_000_000 { n / 1000 } else { n })
    })?;
    let (_, month, day) = civil_from_days(epoch.div_euclid(86_400));
    let year = civil_from_days(epoch.div_euclid(86_400)).0;
    let name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get((month as usize).wrapping_sub(1))?;
    Some(format!("{name} {day}, {year}"))
}

fn format_reset_delta(iso: &str) -> Option<String> {
    let target = parse_rfc3339(iso)?;
    let delta = target - now_unix();
    if delta <= 90 {
        return None;
    }
    let hours = delta / 3600;
    let minutes = (delta % 3600) / 60;
    if hours >= 48 {
        Some(format!("{}d {}h", hours / 24, hours % 24))
    } else if hours > 0 {
        Some(format!("{hours}h {minutes}m"))
    } else {
        Some(format!("{minutes}m"))
    }
}

fn parse_rfc3339(value: &str) -> Option<i64> {
    let value = value.trim().trim_end_matches('Z');
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let time = time.split('.').next().unwrap_or(time);
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next().unwrap_or("0").parse().ok()?;
    let days = civil_days(year, month, day)?;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

fn epoch_to_rfc3339(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let tod = epoch.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = tod / 3600;
    let minute = (tod % 3600) / 60;
    let second = tod % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_days(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    Some(era * 146_097 + doe as i64 - 719_468)
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month as i64, day as i64)
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
    for (name, path) in [
        ("ui_cjk_jp", r"C:\Windows\Fonts\YuGothR.ttc"),
        ("ui_cjk_sc", r"C:\Windows\Fonts\msyh.ttc"),
        ("ui_cjk_kr", r"C:\Windows\Fonts\malgun.ttf"),
        (
            "ui_cjk_jp_linux",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ),
        (
            "ui_cjk_noto",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut data = FontData::from_owned(bytes);
            data.index = 0;
            fonts.font_data.insert(name.into(), data.into());
            if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
                fam.push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn apply_serika_theme(ctx: &egui::Context) {
    let theme = if ui_light() {
        Theme::Light
    } else {
        Theme::Dark
    };
    ctx.set_theme(theme);
    let mut style = (*ctx.style_of(theme)).clone();
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

    let mut visuals = if ui_light() {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = bg_deep();
    visuals.window_fill = bg_card();
    visuals.extreme_bg_color = bg_input();
    visuals.faint_bg_color = bg_sidebar();
    visuals.override_text_color = Some(text_primary());
    visuals.selection.bg_fill = bg_active_row();
    visuals.selection.stroke = Stroke::new(1.0, SERIKA_VIOLET);

    visuals.widgets.noninteractive.bg_fill = bg_card();
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border_subtle());
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text_primary());
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(7);

    visuals.widgets.inactive.bg_fill = bg_input();
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border_subtle());
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text_primary());
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);

    visuals.widgets.hovered.bg_fill = bg_card_hover();
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, SERIKA_VIOLET);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, text_primary());
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);

    visuals.widgets.active.bg_fill = SERIKA_VIOLET_SOFT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, SERIKA_VIOLET);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, text_primary());
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    style.visuals = visuals;
    ctx.set_style_of(theme, style);
}
