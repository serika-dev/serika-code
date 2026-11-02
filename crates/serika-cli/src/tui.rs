//! Terminal presentation for the `serika` CLI.
//!
//! The interactive session borrows its look from modern agent CLIs: a gradient
//! logo and a boxed header, a boxed input with a status line under it, bulleted
//! assistant turns with highlighted code, tool calls in bordered cells, coloured
//! diffs, and a live spinner line.
//!
//! Everything is plain ANSI. When stdout or stderr is not a terminal, or with
//! `--no-color` / `NO_COLOR`, the renderer falls back to undecorated output so
//! pipes and scripts keep working.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::highlight;

// Default brand colours (truecolor). Themes swap these two at write time.
pub const VIOLET: &str = "\x1b[38;2;139;92;246m"; // #8B5CF6
pub const VIOLET_LIGHT: &str = "\x1b[38;2;167;139;250m"; // #A78BFA
pub const GREEN: &str = "\x1b[38;2;52;211;153m";
pub const RED: &str = "\x1b[38;2;248;113;113m";
pub const AMBER: &str = "\x1b[38;2;251;191;36m";
pub const ORANGE: &str = "\x1b[38;2;251;146;60m";
const DIFF_ADD: &str = "\x1b[38;2;134;239;172m\x1b[48;2;14;38;24m";
const DIFF_DEL: &str = "\x1b[38;2;252;165;165m\x1b[48;2;48;16;20m";
pub const USER_BG: &str = "\x1b[48;2;34;32;42m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";
pub const RESET: &str = "\x1b[0m";
pub const CLEAR_LINE: &str = "\r\x1b[2K";

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Lines in raw mode need an explicit carriage return.
static RAW: AtomicBool = AtomicBool::new(false);
static THEME: AtomicUsize = AtomicUsize::new(0);

pub fn set_raw(on: bool) {
    RAW.store(on, Ordering::SeqCst);
}


/// A colour theme: logo gradient stops plus the two accent shades.
pub struct Theme {
    pub name: &'static str,
    pub stops: [(u8, u8, u8); 3],
    pub accent: (u8, u8, u8),
    pub accent_light: (u8, u8, u8),
}

pub const THEMES: [Theme; 5] = [
    Theme { name: "violet", stops: [(139, 92, 246), (192, 132, 252), (244, 114, 182)], accent: (139, 92, 246), accent_light: (167, 139, 250) },
    Theme { name: "ocean", stops: [(6, 182, 212), (59, 130, 246), (139, 92, 246)], accent: (56, 189, 248), accent_light: (125, 211, 252) },
    Theme { name: "sunset", stops: [(249, 115, 22), (244, 63, 94), (168, 85, 247)], accent: (251, 146, 60), accent_light: (253, 186, 116) },
    Theme { name: "forest", stops: [(16, 185, 129), (132, 204, 22), (34, 211, 238)], accent: (52, 211, 153), accent_light: (110, 231, 183) },
    Theme { name: "mono", stops: [(229, 231, 235), (161, 161, 170), (113, 113, 122)], accent: (212, 212, 216), accent_light: (228, 228, 231) },
];

pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().position(|t| t.name.eq_ignore_ascii_case(name.trim())) {
        Some(index) => {
            THEME.store(index, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

pub fn theme() -> &'static Theme {
    &THEMES[THEME.load(Ordering::SeqCst).min(THEMES.len() - 1)]
}

fn fg((r, g, b): (u8, u8, u8)) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Swap the default accent colours for the theme's, and fix line endings in raw mode.
pub fn themed(text: &str) -> String {
    let index = THEME.load(Ordering::SeqCst);
    let mut out = if index == 0 {
        text.to_string()
    } else {
        let t = theme();
        text.replace(VIOLET, &fg(t.accent)).replace(VIOLET_LIGHT, &fg(t.accent_light))
    };
    if RAW.load(Ordering::SeqCst) {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    out
}

/// `println!` that respects the theme and raw mode.
#[macro_export]
macro_rules! outln {
    () => {{ $crate::tui::write_out(&format!("{}", "\n")); }};
    ($($arg:tt)*) => {{ $crate::tui::write_out(&format!("{}\n", format_args!($($arg)*))); }};
}

/// `print!` that respects the theme and raw mode.
#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => {{ $crate::tui::write_out(&format!($($arg)*)); }};
}

/// `eprintln!` that respects the theme and raw mode.
#[macro_export]
macro_rules! errln {
    ($($arg:tt)*) => {{ $crate::tui::write_err(&format!("{}\n", format_args!($($arg)*))); }};
}

pub fn write_out(text: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(themed(text).as_bytes());
    let _ = out.flush();
}

pub fn write_err(text: &str) {
    let mut err = std::io::stderr();
    let _ = err.write_all(themed(text).as_bytes());
    let _ = err.flush();
}

/// Shared terminal capabilities.
#[derive(Clone, Copy)]
pub struct Term {
    pub fancy: bool,
}

impl Term {
    pub fn detect(no_color: bool) -> Self {
        let tty = std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
        let disabled = no_color
            || std::env::var_os("NO_COLOR").is_some()
            || std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);
        let fancy = tty && !disabled && enable_vt();
        Self { fancy }
    }

    pub fn paint(&self, style: &str, text: &str) -> String {
        if self.fancy {
            format!("{style}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    pub fn dim(&self, text: &str) -> String {
        self.paint(DIM, text)
    }

    pub fn bold(&self, text: &str) -> String {
        self.paint(BOLD, text)
    }

    pub fn accent(&self, text: &str) -> String {
        self.paint(VIOLET_LIGHT, text)
    }

    pub fn green(&self, text: &str) -> String {
        self.paint(GREEN, text)
    }

    pub fn red(&self, text: &str) -> String {
        self.paint(RED, text)
    }

    pub fn amber(&self, text: &str) -> String {
        self.paint(AMBER, text)
    }

    pub fn width(&self) -> usize {
        terminal_width().clamp(40, 240)
    }
}

/// Display width of text that may contain ANSI escapes.
pub fn visible_width(text: &str) -> usize {
    strip_ansi(text).width()
}

pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Session facts shown in the header box.
pub struct Banner<'a> {
    pub version: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
    pub directory: &'a Path,
    pub branch: Option<&'a str>,
    pub permission: &'a str,
    pub account: &'a str,
    pub session: Option<&'a str>,
}

const LOGO: [&str; 6] = [
    "███████╗███████╗██████╗ ██╗██╗  ██╗ █████╗ ",
    "██╔════╝██╔════╝██╔══██╗██║██║ ██╔╝██╔══██╗",
    "███████╗█████╗  ██████╔╝██║█████╔╝ ███████║",
    "╚════██║██╔══╝  ██╔══██╗██║██╔═██╗ ██╔══██║",
    "███████║███████╗██║  ██║██║██║  ██╗██║  ██║",
    "╚══════╝╚══════╝╚═╝  ╚═╝╚═╝╚═╝  ╚═╝╚═╝  ╚═╝",
];

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

/// Colour at position `t` (0..1) along the theme gradient.
pub fn gradient_at(t: f32) -> (u8, u8, u8) {
    let [a, b, c] = theme().stops;
    if t < 0.5 {
        mix(a, b, t * 2.0)
    } else {
        mix(b, c, (t - 0.5) * 2.0)
    }
}

/// Paint text with the theme gradient, one colour step per character.
pub fn gradient_text(text: &str) -> String {
    let count = text.chars().count().max(2) - 1;
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if c == ' ' {
            out.push(c);
            continue;
        }
        out.push_str(&fg(gradient_at(i as f32 / count as f32)));
        out.push(c);
    }
    out.push_str(RESET);
    out
}

pub fn print_banner(term: &Term, info: &Banner<'_>) {
    if !term.fancy {
        outln!("Serika Code {}", info.version);
        outln!("model: {} ({})", info.model, info.provider);
        outln!("directory: {}", info.directory.display());
        outln!("permissions: {}", info.permission);
        outln!("account: {}", info.account);
        outln!("Type a task, /help for commands, /exit to leave.");
        return;
    }
    let width = term.width();
    outln!();
    if width < 64 {
        // Narrow terminal: one line of brand, then the facts without a box.
        outln!("  {}  {DIM}v{}{RESET}", gradient_text("▌ Serika Code"), info.version);
        outln!();
        let dir = match info.branch {
            Some(branch) => format!("{}  {VIOLET_LIGHT}⎇ {branch}{RESET}", pretty_path(info.directory)),
            None => pretty_path(info.directory),
        };
        for (label, value) in [
            ("model", format!("{BOLD}{}{RESET}", info.model)),
            ("via", info.provider.to_string()),
            ("folder", dir),
            ("mode", info.permission.to_string()),
        ] {
            let room = width.saturating_sub(12);
            let value = if visible_width(&value) > room { ellipsize_start(&strip_ansi(&value), room) } else { value };
            outln!("  {DIM}{label:<8}{RESET}{value}");
        }
        outln!();
        outln!("  {DIM}/ commands · @ files · Shift+Tab mode · Ctrl+P model{RESET}");
        outln!();
        return;
    }
    if width >= 50 {
        // The shadow strokes (╗ ║ ═ …) stay dim so the letters read first.
        for row in LOGO {
            let count = row.chars().count().max(2) - 1;
            let mut line = String::from("  ");
            for (i, c) in row.chars().enumerate() {
                if c == ' ' {
                    line.push(' ');
                } else if c == '█' {
                    line.push_str(&fg(gradient_at(i as f32 / count as f32)));
                    line.push(c);
                } else {
                    let (r, g, b) = gradient_at(i as f32 / count as f32);
                    line.push_str(&fg(((r as u16 * 2 / 5) as u8, (g as u16 * 2 / 5) as u8, (b as u16 * 2 / 5) as u8)));
                    line.push(c);
                }
            }
            line.push_str(RESET);
            write_out(&format!("{line}\n"));
        }
        outln!("  {DIM}code{RESET} {DIM}·{RESET} {}", gradient_text(&format!("v{}", info.version)));
    } else {
        outln!("  {}  {DIM}v{}{RESET}", gradient_text(">_ Serika Code"), info.version);
    }
    outln!();

    let inner = width.saturating_sub(6).min(76);
    let dir = match info.branch {
        Some(branch) => format!("{}  {DIM}on{RESET} {VIOLET_LIGHT}⎇ {branch}{RESET}", pretty_path(info.directory)),
        None => pretty_path(info.directory),
    };
    let rows: Vec<(String, String, &str)> = vec![
        ("model".into(), format!("{BOLD}{}{RESET} {DIM}· {}{RESET}", info.model, info.provider), "/model"),
        ("directory".into(), dir, ""),
        ("permissions".into(), info.permission.to_string(), "/permissions"),
        ("account".into(), info.account.to_string(), "/usage"),
    ];
    let mut lines: Vec<String> = rows
        .into_iter()
        .map(|(label, value, hint)| {
            let label = format!("{DIM}{label:<12}{RESET}");
            let hint_w = if hint.is_empty() { 0 } else { hint.len() + 2 };
            let budget = inner.saturating_sub(13 + hint_w);
            let value = if visible_width(&value) > budget {
                ellipsize_start(&strip_ansi(&value), budget.max(8))
            } else {
                value
            };
            let pad = inner.saturating_sub(13 + visible_width(&value) + hint_w);
            if hint.is_empty() {
                format!("{label} {value}{}", " ".repeat(pad + hint_w))
            } else {
                format!("{label} {value}{}  {VIOLET_LIGHT}{hint}{RESET}", " ".repeat(pad))
            }
        })
        .collect();
    if let Some(id) = info.session {
        let text = format!("{DIM}{:<12}{RESET} {DIM}{id}{RESET}", "session");
        let pad = inner.saturating_sub(visible_width(&text));
        lines.push(format!("{text}{}", " ".repeat(pad)));
    }
    let b = |s: &str| format!("{DIM}{s}{RESET}");
    outln!("  {}", b(&format!("╭{}╮", "─".repeat(inner + 2))));
    for line in lines {
        outln!("  {} {line} {}", b("│"), b("│"));
    }
    outln!("  {}", b(&format!("╰{}╯", "─".repeat(inner + 2))));
    outln!();
    outln!("  {BOLD}Tips{RESET}");
    for (key, tip) in [
        ("/", "commands  ·  @ mention files  ·  !run a shell command"),
        ("Ctrl+P", "switch model mid-chat  ·  Shift+Tab permissions"),
        ("Shift+Enter", "new line  ·  ↑↓ history  ·  Esc interrupts a reply"),
        ("Enter while busy", "queues your next message"),
    ] {
        outln!("  {VIOLET_LIGHT}{key:<17}{RESET}{DIM}{tip}{RESET}");
    }
    outln!();
}

pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("/model", "[id]", "switch model, even mid-chat: Serika, Claude Code, Codex, local"),
    ("/provider", "", "Claude Code and Codex: sign-in, models, limits"),
    ("/permissions", "[mode]", "what Serika may do without asking"),
    ("/effort", "[level]", "reasoning effort for Claude Code and Codex"),
    ("/usage", "", "your Serika and subscription limits"),
    ("/new", "", "start a fresh conversation"),
    ("/clear", "", "clear the screen and start fresh"),
    ("/sessions", "", "pick a saved conversation to continue"),
    ("/resume", "[id]", "continue a saved conversation"),
    ("/compact", "", "summarise the chat and continue from the summary"),
    ("/undo", "", "take back the last message and its file changes"),
    ("/retry", "", "ask again for a new reply to your last message"),
    ("/copy", "", "copy the last reply to the clipboard"),
    ("/export", "[file]", "save this conversation as Markdown"),
    ("/rename", "<title>", "rename this conversation"),
    ("/stats", "", "messages, tools, files changed, and models used"),
    ("/init", "", "have Serika write SERIKA.md with notes about this project"),
    ("/diff", "", "show uncommitted changes"),
    ("/review", "[focus]", "ask Serika to review your changes"),
    ("/browser", "<url|on|off|close>", "open a page in Serika's browser"),
    ("/attach", "<path>", "attach a file or image to the next message"),
    ("/queue", "[clear]", "messages waiting to be sent"),
    ("/expand", "", "show the full output of the last tools (Ctrl+O)"),
    ("/theme", "[name]", "violet, ocean, sunset, forest, mono"),
    ("/status", "", "show the session settings"),
    ("/doctor", "", "check the install and connection"),
    ("/login", "", "sign in to Serika"),
    ("/logout", "", "sign out of Serika"),
    ("/help", "", "commands and keys"),
    ("/quit", "", "leave Serika Code"),
    ("/exit", "", "leave Serika Code"),
];

pub fn print_help(term: &Term) {
    outln!();
    outln!("  {}", term.bold("Commands"));
    for &(cmd, args, help) in COMMANDS {
        let left = if args.is_empty() { cmd.to_string() } else { format!("{cmd} {args}") };
        outln!("  {}  {}", term.accent(&format!("{left:<26}")), term.dim(help));
    }
    outln!();
    outln!("  {}", term.bold("Keys"));
    for (key, help) in [
        ("Enter", "send (queues it while a reply runs)"),
        ("!command", "run a shell command; its output goes with your next message"),
        ("Shift+Tab", "cycle permissions: supervised, auto-accept, full, read-only"),
        ("Ctrl+P", "switch model without losing the conversation"),
        ("Shift+Enter, Alt+Enter, Ctrl+J", "new line (or end a line with \\)"),
        ("↑ ↓", "history, or move in a list"),
        ("Tab", "complete a command or @file"),
        ("Esc", "interrupt the reply, close a list, or clear the input"),
        ("Ctrl+C", "clear the input; twice to quit"),
        ("Ctrl+O", "expand the last tool output"),
        ("Ctrl+L", "clear the screen"),
        ("Ctrl+D", "quit on an empty input"),
    ] {
        outln!("  {}  {}", term.accent(&format!("{key:<32}")), term.dim(help));
    }
    outln!();
}

/// The message you sent, as a shaded cell above the reply.
pub fn user_cell(term: &Term, text: &str, attachments: &[String]) {
    if !term.fancy {
        outln!("> {text}");
        return;
    }
    let width = term.width();
    let mut first = true;
    for line in text.lines() {
        for piece in wrap_ansi(line, width.saturating_sub(4).max(10)) {
            let prefix = if first { format!("{VIOLET}{BOLD}›{RESET}{USER_BG} ") } else { "  ".to_string() };
            first = false;
            let body = format!("{prefix}{USER_BG}{piece}");
            let pad = width.saturating_sub(visible_width(&body) + 1);
            outln!("{USER_BG}{body}{USER_BG}{}{RESET}", " ".repeat(pad));
        }
    }
    for name in attachments {
        outln!("  {DIM}📎 {name}{RESET}");
    }
    outln!();
}

pub fn clear_screen(term: &Term) {
    if term.fancy {
        write_out("\x1b[2J\x1b[3J\x1b[H");
    }
}

pub fn info_line(term: &Term, text: &str) {
    errln!("  {}", term.dim(text));
}

pub fn note_line(term: &Term, text: &str) {
    if term.fancy {
        errln!("  {VIOLET_LIGHT}•{RESET} {text}");
    } else {
        errln!("{text}");
    }
}

pub fn error_line(term: &Term, text: &str) {
    errln!("{} {}", term.red("■"), text);
}

/// `[██████░░░░]` for a fraction from 0 to 1, red when full.
pub fn bar(fraction: f64, width: usize) -> String {
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * width as f64).round() as usize;
    let filled = if fraction > 0.0 { filled.max(1) } else { 0 };
    let colour = if fraction >= 1.0 {
        RED
    } else if fraction >= 0.8 {
        ORANGE
    } else {
        VIOLET_LIGHT
    };
    format!("{colour}{}{RESET}{DIM}{}{RESET}", "█".repeat(filled), "░".repeat(width - filled.min(width)))
}

/// Tool output kept for Ctrl+O, newest last.
static EXPANDABLE: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

pub fn print_expanded(term: &Term) {
    let list = EXPANDABLE.lock().unwrap_or_else(|e| e.into_inner());
    if list.is_empty() {
        info_line(term, "No tool output to expand yet.");
        return;
    }
    for (header, lines) in list.iter() {
        outln!("{VIOLET_LIGHT}╭{RESET} {header}");
        for line in lines {
            outln!("{VIOLET_LIGHT}│{RESET} {DIM}{line}{RESET}");
        }
        outln!("{VIOLET_LIGHT}╰{RESET}");
    }
    outln!();
}

/// Streams one agent turn into the terminal.
pub struct TurnRenderer {
    term: Term,
    started: Instant,
    status: String,
    tick: usize,
    spinner_drawn: bool,
    line_buf: String,
    fence: Option<String>,
    highlight: highlight::State,
    msg_started: bool,
    tools: HashMap<String, (String, String, Instant)>,
    tool_output: HashMap<String, Vec<String>>,
    usage: Option<String>,
    queued: usize,
    draft: String,
    interrupt_hint: &'static str,
    pub summary: String,
    tools_run: usize,
    tools_failed: usize,
    files_edited: Vec<String>,
}

impl TurnRenderer {
    pub fn new(term: Term) -> Self {
        if term.fancy {
            write_err("\x1b[?25l");
        }
        EXPANDABLE.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Self {
            term,
            started: Instant::now(),
            status: "Working".into(),
            tick: 0,
            spinner_drawn: false,
            line_buf: String::new(),
            fence: None,
            highlight: highlight::State::default(),
            msg_started: false,
            tools: HashMap::new(),
            tool_output: HashMap::new(),
            usage: None,
            queued: 0,
            draft: String::new(),
            interrupt_hint: "Ctrl+C to interrupt",
            summary: String::new(),
            tools_run: 0,
            tools_failed: 0,
            files_edited: Vec::new(),
        }
    }

    /// The interactive session reads keys itself, so Esc works as well.
    pub fn interactive(mut self) -> Self {
        self.interrupt_hint = "esc to interrupt";
        self
    }

    /// Show what is queued and what you are typing next to the spinner.
    pub fn set_queue(&mut self, queued: usize, draft: &str) {
        self.queued = queued;
        self.draft = draft.to_string();
        self.tick = self.tick.wrapping_sub(1);
        self.tick();
    }

    fn clear_spinner(&mut self) {
        if self.spinner_drawn {
            write_err(CLEAR_LINE);
            self.spinner_drawn = false;
        }
    }

    /// Redraw the live status line. Call this on a timer.
    pub fn tick(&mut self) {
        if !self.term.fancy {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        let frame = SPINNER[self.tick % SPINNER.len()];
        let secs = self.started.elapsed().as_secs();
        let elapsed = if secs >= 60 {
            format!("{}m {:02}s", secs / 60, secs % 60)
        } else {
            format!("{secs}s")
        };
        let width = self.term.width();
        let shimmer = gradient_at(((self.tick % 20) as f32 / 19.0 * 2.0 - 1.0).abs());
        let mut line = format!(
            "{}{frame}{RESET} {BOLD}{}{RESET} {DIM}({elapsed} · {}){RESET}",
            fg(shimmer),
            ellipsize_end(&self.status, width.saturating_sub(50).max(12)),
            self.interrupt_hint
        );
        if self.queued > 0 {
            line.push_str(&format!(" {AMBER}· {} queued{RESET}", self.queued));
        }
        if !self.draft.is_empty() {
            let room = width.saturating_sub(visible_width(&line) + 6).max(8);
            line.push_str(&format!("  {VIOLET}›{RESET} {}{DIM}▏{RESET}", ellipsize_start(&self.draft.replace('\n', " ⏎ "), room)));
        }
        write_err(&format!("{CLEAR_LINE}{line}"));
        self.spinner_drawn = true;
    }

    pub fn set_status(&mut self, status: &str) {
        let s = status.trim();
        if s.is_empty() {
            return;
        }
        if !self.term.fancy {
            errln!("{s}");
            return;
        }
        let mut chars = s.chars();
        self.status = match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        self.tick = self.tick.wrapping_sub(1);
        self.tick();
    }

    pub fn session_started(&mut self, id: &str) {
        if !self.term.fancy {
            errln!("session {id}");
        }
    }

    pub fn message_chunk(&mut self, text: &str) {
        self.summary.push_str(text);
        if !self.term.fancy {
            print!("{text}");
            let _ = std::io::stdout().flush();
            return;
        }
        self.line_buf.push_str(text);
        while let Some(idx) = self.line_buf.find('\n') {
            let line: String = self.line_buf[..idx].trim_end_matches('\r').to_string();
            self.line_buf.replace_range(..idx + 1, "");
            self.emit_message_line(&line);
        }
    }

    pub fn message_completed(&mut self) {
        if !self.term.fancy {
            println!();
            return;
        }
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.emit_message_line(line.trim_end_matches('\r'));
        }
        self.close_fence();
        if self.msg_started {
            outln!();
        }
        self.msg_started = false;
    }

    fn close_fence(&mut self) {
        if self.fence.take().is_some() {
            outln!("  {DIM}╰{}{RESET}", "─".repeat(8));
        }
    }

    fn emit_message_line(&mut self, line: &str) {
        self.clear_spinner();
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if !self.msg_started {
                outln!("{VIOLET_LIGHT}●{RESET}");
                self.msg_started = true;
            }
            if self.fence.is_some() {
                self.close_fence();
            } else {
                let lang = rest.trim().to_string();
                let label = if lang.is_empty() { String::new() } else { format!(" {VIOLET_LIGHT}{lang}{RESET} ") };
                outln!("  {DIM}╭─{RESET}{label}{DIM}{}{RESET}", "─".repeat(8));
                self.highlight = highlight::State::default();
                self.fence = Some(lang);
            }
            return;
        }
        if !self.msg_started && trimmed.is_empty() {
            return;
        }
        let prefix = if self.msg_started {
            "  ".to_string()
        } else {
            self.msg_started = true;
            format!("{VIOLET_LIGHT}●{RESET} ")
        };
        match &self.fence {
            Some(lang) => {
                let width = self.term.width().saturating_sub(8);
                let code = if line.chars().count() > width { ellipsize_end(line, width) } else { line.to_string() };
                outln!("{prefix}{DIM}│{RESET} {}", highlight::line(&code, lang, &mut self.highlight));
            }
            None => {
                // Wrap prose at the terminal edge, continuing under the text (not under the bullet).
                let body = render_markdown_line(line, self.term.width());
                let hang = hanging_indent(line);
                let width = self.term.width().saturating_sub(3).max(20);
                for (i, piece) in wrap_ansi(&body, width).into_iter().enumerate() {
                    if i == 0 {
                        outln!("{prefix}{piece}");
                    } else {
                        outln!("  {}{piece}", " ".repeat(hang));
                    }
                }
            }
        }
    }

    pub fn tool_requested(&mut self, id: &str, name: &str, arguments: &str) {
        self.tools.insert(id.to_string(), (name.to_string(), arguments.to_string(), Instant::now()));
        let (verb, detail) = tool_header(name, arguments);
        self.status = if detail.is_empty() {
            verb_progressive(&verb)
        } else {
            format!("{} {}", verb_progressive(&verb), detail)
        };
        if !self.term.fancy {
            errln!("tool {name}");
        }
    }

    pub fn tool_output(&mut self, id: &str, stream: &str) {
        if !self.term.fancy {
            errln!("{stream}");
            return;
        }
        let lines = self.tool_output.entry(id.to_string()).or_default();
        for l in stream.lines() {
            lines.push(l.to_string());
        }
    }

    pub fn tool_finished(&mut self, id: &str, name: &str, ok: bool, summary: &str) {
        if !self.term.fancy {
            let mark = if ok { "ok" } else { "failed" };
            errln!("{name} {mark}: {summary}");
            return;
        }
        self.flush_partial_message();
        self.clear_spinner();
        self.tools_run += 1;
        if !ok {
            self.tools_failed += 1;
        }
        let (args, started) = self
            .tools
            .get(id)
            .map(|(_, a, t)| (a.clone(), Some(*t)))
            .unwrap_or_default();
        let (verb, detail) = tool_header(name, &args);
        // Edits are rendered by `file_changed` with the diff itself.
        if ok && matches!(name, "write_file" | "replace_in_file" | "apply_patch" | "Edit" | "Write" | "MultiEdit") {
            self.status = "Working".into();
            return;
        }
        let (edge, mark) = if ok { (GREEN, "✓") } else { (RED, "✗") };
        let icon = tool_icon(name);
        let detail_fmt = if name == "run_command" || name == "Bash" || name.starts_with("git_") {
            format!("{VIOLET_LIGHT}{detail}{RESET}")
        } else {
            detail.clone()
        };
        let took = started
            .map(|t| t.elapsed().as_secs_f32())
            .filter(|s| *s >= 0.5)
            .map(|s| format!("  {DIM}{s:.1}s{RESET}"))
            .unwrap_or_default();
        let width = self.term.width();
        let header = format!("{edge}{mark}{RESET} {icon} {BOLD}{verb}{RESET} {detail_fmt}");
        let header = if visible_width(&header) + 8 > width {
            format!("{edge}{mark}{RESET} {icon} {BOLD}{verb}{RESET} {}", ellipsize_end(&detail, width.saturating_sub(verb.len() + 16)))
        } else {
            header
        };

        let mut lines: Vec<String> = self.tool_output.remove(id).unwrap_or_default();
        if lines.is_empty() && (!ok || matches!(name, "run_command" | "Bash" | "git_status" | "git_log" | "git_commit")) {
            lines = summary.lines().map(|l| l.to_string()).collect();
        }
        lines.retain(|l| !l.trim().is_empty());
        if lines.is_empty() {
            let note = summary.lines().next().unwrap_or("").trim();
            if note.is_empty() {
                outln!("{edge}╶{RESET}{header}{took}");
            } else {
                outln!("{edge}╶{RESET}{header}  {DIM}{}{RESET}{took}", ellipsize_end(note, width.saturating_sub(visible_width(&header) + 12).max(10)));
            }
            self.status = "Working".into();
            return;
        }
        outln!("{edge}╭{RESET} {header}{took}");
        let inner = width.saturating_sub(6);
        let max = if ok { 4 } else { 10 };
        let total = lines.len();
        for l in lines.iter().take(max) {
            let colour = if ok { DIM } else { RED };
            outln!("{edge}│{RESET} {colour}{}{RESET}", ellipsize_end(l, inner));
        }
        if total > max {
            let hint = if self.interrupt_hint.starts_with("esc") { " · ctrl+o to expand" } else { "" };
            outln!("{edge}╰{RESET} {DIM}{ITALIC}+{} more lines{hint}{RESET}", total - max);
        } else {
            outln!("{edge}╰{RESET}");
        }
        EXPANDABLE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((strip_ansi(&header), lines));
        self.status = "Working".into();
    }

    pub fn file_changed(&mut self, path: &str, diff: &str) {
        if !self.term.fancy {
            errln!("changed {path}");
            println!("{diff}");
            return;
        }
        self.flush_partial_message();
        self.clear_spinner();
        if !self.files_edited.iter().any(|p| p == path) {
            self.files_edited.push(path.to_string());
        }
        let (adds, dels) = diff_counts(diff);
        outln!("{GREEN}╭{RESET} {GREEN}✓{RESET} ✎ {BOLD}Edited{RESET} {path} {GREEN}+{adds}{RESET} {RED}-{dels}{RESET}");
        print_diff_body(&self.term, diff, 40, &format!("{GREEN}│{RESET}"));
        outln!("{GREEN}╰{RESET}");
    }

    /// A divider where the chat moved to another model.
    pub fn handoff(&mut self, from: &str, to: &str, messages: usize) {
        if !self.term.fancy {
            errln!("switched from {from} to {to}; {messages} earlier messages handed over");
            return;
        }
        self.flush_partial_message();
        self.clear_spinner();
        let from = if from.is_empty() { "the previous model".to_string() } else { from.to_string() };
        let label = format!(
            " ⇄ {from} → {BOLD}{to}{RESET}{DIM} · {messages} earlier message{} handed over ",
            if messages == 1 { "" } else { "s" }
        );
        let side = self.term.width().saturating_sub(visible_width(&label) + 4) / 2;
        outln!("{DIM}{}{RESET}{VIOLET_LIGHT}{label}{RESET}{DIM}{}{RESET}", "╌".repeat(side.max(2)), "╌".repeat(side.max(2)));
    }

    pub fn usage(&mut self, summary: String) {
        if !self.term.fancy {
            errln!("{summary}");
            return;
        }
        if !summary.trim().is_empty() {
            self.usage = Some(summary);
        }
    }

    pub fn failed(&mut self, message: &str) {
        self.flush_partial_message();
        self.clear_spinner();
        if self.term.fancy {
            outln!("{RED}■{RESET} {message}");
            outln!();
        } else {
            errln!("serika: {message}");
        }
    }

    pub fn cancelled(&mut self) {
        self.flush_partial_message();
        self.clear_spinner();
        if self.term.fancy {
            outln!("{AMBER}■{RESET} {BOLD}Interrupted{RESET} {DIM}· tell Serika what to do differently{RESET}");
            outln!();
        } else {
            errln!("cancelled");
        }
    }

    /// Pause the spinner and show the cursor (for approval prompts).
    pub fn suspend(&mut self) {
        self.flush_partial_message();
        self.clear_spinner();
        if self.term.fancy {
            write_err("\x1b[?25h");
        }
    }

    pub fn resume(&mut self) {
        if self.term.fancy {
            write_err("\x1b[?25l");
        }
    }

    fn flush_partial_message(&mut self) {
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.emit_message_line(&line);
        }
        self.close_fence();
        if self.msg_started {
            outln!();
            self.msg_started = false;
        }
    }

    /// Finish the turn: clear the spinner, restore the cursor, print a footer.
    pub fn finish(&mut self) {
        self.flush_partial_message();
        self.clear_spinner();
        if !self.term.fancy {
            return;
        }
        let secs = self.started.elapsed().as_secs_f32();
        let took = if secs >= 60.0 {
            format!("{}m {:02}s", secs as u64 / 60, secs as u64 % 60)
        } else {
            format!("{secs:.1}s")
        };
        let mut parts = vec![format!("{BOLD}Done{RESET}{DIM} in {took}")];
        if self.tools_run > 0 {
            let failed = if self.tools_failed > 0 { format!(" ({} failed)", self.tools_failed) } else { String::new() };
            parts.push(format!("{} tool{}{failed}", self.tools_run, if self.tools_run == 1 { "" } else { "s" }));
        }
        if !self.files_edited.is_empty() {
            let n = self.files_edited.len();
            parts.push(format!("{n} file{} edited", if n == 1 { "" } else { "s" }));
        }
        if let Some(u) = &self.usage {
            parts.push(u.clone());
        }
        let label = format!(" {GREEN}✓{RESET} {}{RESET} ", parts.join(" · "));
        let width = self.term.width();
        let room = width.saturating_sub(visible_width(&label) + 4);
        errln!("{DIM}──{RESET}{label}{DIM}{}{RESET}", "─".repeat(room.min(width)));
        write_err("\x1b[?25h");
    }
}

impl Drop for TurnRenderer {
    fn drop(&mut self) {
        if self.term.fancy {
            write_err("\x1b[?25h");
        }
    }
}

/// Approval prompt. Returns true when the user allows the action.
pub fn ask_approval(term: &Term, title: &str, detail: &str, destructive: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        errln!("approval required to {title}: {detail}");
        return false;
    }
    if !term.fancy {
        let kind = if destructive { "This can be destructive. " } else { "" };
        eprint!("{kind}Allow {title}?\n{detail}\n[y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        return matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    }
    let bar_col = if destructive { RED } else { VIOLET };
    let heading = if destructive {
        format!("{RED}{BOLD}Allow {title}?{RESET} {RED}(destructive){RESET}")
    } else {
        format!("{BOLD}Allow {title}?{RESET}")
    };
    let width = term.width().saturating_sub(6);
    errln!("{bar_col}╭{RESET} {heading}");
    for line in detail.lines().take(12) {
        errln!("{bar_col}│{RESET} {DIM}{}{RESET}", ellipsize_end(line, width));
    }
    write_err(&format!(
        "{bar_col}╰{RESET} {VIOLET_LIGHT}y{RESET} allow   {VIOLET_LIGHT}n{RESET} deny   {DIM}(Enter or Esc denies){RESET} "
    ));
    let allow = if RAW.load(Ordering::SeqCst) {
        crate::editor::read_yes_no()
    } else {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    };
    let verdict = if allow {
        format!("{GREEN}✓ Allowed{RESET}")
    } else {
        format!("{RED}✗ Denied{RESET}")
    };
    errln!("{CLEAR_LINE}  {verdict} {DIM}{title}{RESET}");
    allow
}

/// Print a unified diff with coloured gutters (used by `/diff` and `review`).
pub fn print_diff(term: &Term, diff: &str) {
    if !term.fancy {
        println!("{diff}");
        return;
    }
    let mut current: Option<String> = None;
    let mut chunk = String::new();
    let flush = |path: &Option<String>, chunk: &str| {
        if let Some(p) = path {
            let (a, d) = diff_counts(chunk);
            outln!("{VIOLET_LIGHT}╭{RESET} {BOLD}{p}{RESET} {GREEN}+{a}{RESET} {RED}-{d}{RESET}");
            print_diff_body(term, chunk, 400, &format!("{VIOLET_LIGHT}│{RESET}"));
            outln!("{VIOLET_LIGHT}╰{RESET}");
        }
    };
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&current, &chunk);
            chunk.clear();
            let path = rest
                .split_whitespace()
                .last()
                .map(|p| p.trim_start_matches("b/").to_string())
                .unwrap_or_else(|| rest.to_string());
            current = Some(path);
            continue;
        }
        chunk.push_str(line);
        chunk.push('\n');
    }
    if current.is_none() {
        print_diff_body(term, &chunk, 400, " ");
    } else {
        flush(&current, &chunk);
    }
}

fn print_diff_body(term: &Term, diff: &str, max_lines: usize, gutter: &str) {
    let width = term.width().saturating_sub(12);
    let mut old_no: i64 = 0;
    let mut new_no: i64 = 0;
    let mut shown = 0usize;
    let mut hidden = 0usize;
    for line in diff.lines() {
        if line.starts_with("+++")
            || line.starts_with("---")
            || line.starts_with("index ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("similarity")
            || line.starts_with("rename ")
        {
            continue;
        }
        if line.starts_with("@@") {
            let mut parts = line.split_whitespace();
            let _ = parts.next();
            if let Some(old) = parts.next() {
                old_no = old.trim_start_matches('-').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            }
            if let Some(new) = parts.next() {
                new_no = new.trim_start_matches('+').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            }
            if shown > 0 && shown < max_lines {
                outln!("{gutter}   {DIM}⋮{RESET}");
            }
            continue;
        }
        if shown >= max_lines {
            hidden += 1;
            continue;
        }
        shown += 1;
        let text = ellipsize_end(line.get(1..).unwrap_or(""), width);
        if line.starts_with('+') {
            outln!("{gutter} {DIM}{new_no:>4}{RESET} {DIFF_ADD}+{text}{RESET}");
            new_no += 1;
        } else if line.starts_with('-') {
            outln!("{gutter} {DIM}{old_no:>4}{RESET} {DIFF_DEL}-{text}{RESET}");
            old_no += 1;
        } else {
            outln!("{gutter} {DIM}{new_no:>4}  {text}{RESET}");
            old_no += 1;
            new_no += 1;
        }
    }
    if hidden > 0 {
        outln!("{gutter} {DIM}{ITALIC}… +{hidden} lines{RESET}");
    }
}

fn diff_counts(diff: &str) -> (usize, usize) {
    let mut adds = 0;
    let mut dels = 0;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            adds += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            dels += 1;
        }
    }
    (adds, dels)
}

fn tool_icon(name: &str) -> &'static str {
    match name {
        "run_command" | "Bash" => "⚡",
        "read_file" | "Read" => "📄",
        "list_dir" | "LS" | "Glob" => "📁",
        "search_text" | "search_files" | "Grep" => "🔍",
        "delete_file" => "🗑",
        "move_file" => "↪",
        n if n.starts_with("git_") => "⎇",
        n if n.starts_with("browser_") || n == "WebFetch" || n == "WebSearch" => "🌐",
        _ => "⚙",
    }
}

/// Turn a tool call into a past-tense verb and a short detail string.
fn tool_header(name: &str, arguments: &str) -> (String, String) {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match name {
        "run_command" | "Bash" => ("Ran".into(), s("command")),
        "read_file" | "Read" => ("Read".into(), s("path").if_empty(s("file_path"))),
        "list_dir" | "LS" => {
            let p = s("path");
            ("Explored".into(), if p.is_empty() { ".".into() } else { p })
        }
        "search_text" | "Grep" => ("Searched".into(), s("query").if_empty(s("pattern"))),
        "search_files" | "Glob" => ("Searched files".into(), s("query").if_empty(s("pattern"))),
        "write_file" | "replace_in_file" | "apply_patch" | "Edit" | "Write" | "MultiEdit" => {
            ("Edited".into(), s("path").if_empty(s("file_path")))
        }
        "delete_file" => ("Deleted".into(), s("path")),
        "move_file" => ("Moved".into(), format!("{} → {}", s("from"), s("to"))),
        "git_status" => ("Ran".into(), "git status".into()),
        "git_diff" => ("Ran".into(), format!("git diff {}", s("path")).trim().to_string()),
        "git_log" => ("Ran".into(), "git log".into()),
        "git_commit" => ("Committed".into(), s("message")),
        "browser_open" | "browser_new_tab" => ("Opened".into(), s("url")),
        "browser_snapshot" => ("Read page".into(), String::new()),
        "browser_click" => ("Clicked".into(), args.get("id").map(|v| format!("element {v}")).unwrap_or_default()),
        "browser_type" => ("Typed".into(), s("text")),
        "browser_press" => ("Pressed".into(), s("key")),
        "browser_scroll" => ("Scrolled".into(), s("direction")),
        "browser_back" => ("Went back".into(), String::new()),
        "browser_switch_tab" => ("Switched tab".into(), args.get("tab").map(|v| v.to_string()).unwrap_or_default()),
        "WebFetch" => ("Fetched".into(), s("url")),
        "WebSearch" => ("Searched the web".into(), s("query")),
        other => ("Called".into(), other.to_string()),
    }
}

fn verb_progressive(verb: &str) -> String {
    match verb {
        "Ran" => "Running".into(),
        "Read" | "Read page" => "Reading".into(),
        "Explored" => "Exploring".into(),
        "Searched" | "Searched files" | "Searched the web" => "Searching".into(),
        "Edited" => "Editing".into(),
        "Deleted" => "Deleting".into(),
        "Moved" => "Moving".into(),
        "Committed" => "Committing".into(),
        "Opened" | "Fetched" => "Opening".into(),
        "Clicked" | "Typed" | "Pressed" | "Scrolled" | "Went back" | "Switched tab" => "Browsing".into(),
        _ => "Calling".into(),
    }
}

trait IfEmpty {
    fn if_empty(self, other: String) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, other: String) -> String {
        if self.is_empty() {
            other
        } else {
            self
        }
    }
}

/// Markdown for one line: headings, lists, quotes, rules, tables, and inline styles.
fn render_markdown_line(line: &str, width: usize) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if let Some(rest) = trimmed.strip_prefix("# ") {
        return format!("{indent}{}", gradient_text(&strip_ansi(&inline(rest, ""))).replace(RESET, &format!("{RESET}{BOLD}")) + RESET);
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        return format!("{indent}{BOLD}{VIOLET_LIGHT}{}{RESET}", inline(rest, &format!("{BOLD}{VIOLET_LIGHT}")));
    }
    if let Some(rest) = trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("#### ")) {
        return format!("{indent}{BOLD}{}{RESET}", inline(rest, BOLD));
    }
    if trimmed.chars().all(|c| c == '-' || c == '*' || c == '_') && trimmed.len() >= 3 {
        return format!("{DIM}{}{RESET}", "─".repeat(width.saturating_sub(6).min(60)));
    }
    if let Some(rest) = trimmed.strip_prefix("- [ ] ").or_else(|| trimmed.strip_prefix("* [ ] ")) {
        return format!("{indent}{DIM}☐{RESET} {}", inline(rest, ""));
    }
    if let Some(rest) = trimmed.strip_prefix("- [x] ").or_else(|| trimmed.strip_prefix("- [X] ")) {
        return format!("{indent}{GREEN}☑{RESET} {DIM}{}{RESET}", inline(rest, DIM));
    }
    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
        let bullet = if indent.is_empty() { "•" } else { "◦" };
        return format!("{indent}{VIOLET_LIGHT}{bullet}{RESET} {}", inline(rest, ""));
    }
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        if let Some(rest) = trimmed[digits.len()..].strip_prefix(". ").or_else(|| trimmed[digits.len()..].strip_prefix(") ")) {
            return format!("{indent}{VIOLET_LIGHT}{digits}.{RESET} {}", inline(rest, ""));
        }
    }
    if let Some(rest) = trimmed.strip_prefix("> ").or_else(|| trimmed.strip_prefix(">")) {
        return format!("{indent}{VIOLET}▌{RESET} {ITALIC}{}{RESET}", inline(rest, ITALIC));
    }
    if trimmed.starts_with('|') && trimmed.ends_with('|') {
        if trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            return format!("{indent}{DIM}{}{RESET}", trimmed.replace('-', "─").replace('|', "┼"));
        }
        let cells: Vec<String> = trimmed.trim_matches('|').split('|').map(|c| inline(c.trim(), "")).collect();
        return format!("{indent}{DIM}│{RESET} {} {DIM}│{RESET}", cells.join(&format!(" {DIM}│{RESET} ")));
    }
    format!("{indent}{}", inline(trimmed, ""))
}

/// Inline markdown: `code`, **bold**, *italic*, ~~strike~~, and [links](url).
fn inline(text: &str, base: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    let mut prev = ' ';
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                out.push_str(&format!("{VIOLET_LIGHT}{}{RESET}{base}", &after[..end]));
                rest = &after[end + 1..];
                prev = '`';
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            let marker = &rest[..2];
            if let Some(end) = after.find(marker) {
                out.push_str(&format!("{BOLD}{}{RESET}{base}", &after[..end]));
                rest = &after[end + 2..];
                prev = '*';
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("~~") {
            if let Some(end) = after.find("~~") {
                out.push_str(&format!("\x1b[9m{}{RESET}{base}", &after[..end]));
                rest = &after[end + 2..];
                continue;
            }
        }
        if (rest.starts_with('*') || rest.starts_with('_')) && !prev.is_alphanumeric() {
            let marker = &rest[..1];
            let after = &rest[1..];
            if let Some(end) = after.find(marker) {
                let inner = &after[..end];
                if !inner.is_empty() && !inner.starts_with(' ') && !inner.ends_with(' ') {
                    out.push_str(&format!("{ITALIC}{inner}{RESET}{base}"));
                    rest = &after[end + 1..];
                    prev = '*';
                    continue;
                }
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(close) = after.find("](") {
                if let Some(end) = after[close + 2..].find(')') {
                    let label = &after[..close];
                    let url = &after[close + 2..close + 2 + end];
                    if label == url || url.is_empty() {
                        out.push_str(&format!("{UNDERLINE}{VIOLET_LIGHT}{label}{RESET}{base}"));
                    } else {
                        out.push_str(&format!("{UNDERLINE}{label}{RESET}{base} {DIM}({url}){RESET}{base}"));
                    }
                    rest = &after[close + 2 + end + 1..];
                    prev = ')';
                    continue;
                }
            }
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
            prev = c;
        }
        rest = chars.as_str();
    }
    out
}

/// "weekly 0.4% used" from Serika's usage answer; the plan name when no window is known.
pub fn usage_brief(usage: &serika_runtime::UsageSnapshot) -> String {
    let Some(plan) = &usage.plan else { return String::new() };
    let pct = |w: &Option<serika_runtime::model::UsageWindow>| {
        w.as_ref().and_then(|w| match (w.used, w.limit) {
            (Some(used), Some(limit)) if limit > 0 => Some(used as f64 * 100.0 / limit as f64),
            _ => None,
        })
    };
    let show = |p: f64| if p > 0.0 && p < 10.0 { format!("{p:.1}%") } else { format!("{p:.0}%") };
    match (pct(&plan.session), pct(&plan.weekly)) {
        (Some(s), Some(w)) => format!("session {} · week {} used", show(s), show(w)),
        (None, Some(w)) => format!("week {} used", show(w)),
        (Some(s), None) => format!("session {} used", show(s)),
        (None, None) => plan.tier.as_ref().and_then(|t| t.name.clone()).unwrap_or_default(),
    }
}

/// A readable name for a model id when no catalog name is at hand.
pub fn pretty_model(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("claude-code/") {
        return format!("Claude Code {rest}");
    }
    if let Some(rest) = id.strip_prefix("codex/") {
        return format!("Codex {rest}");
    }
    id.to_string()
}

/// Columns a wrapped markdown line continues at: its indent plus a list marker.
fn hanging_indent(line: &str) -> usize {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    let marker = if trimmed.starts_with("- [") || trimmed.starts_with("* [") {
        2
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") || trimmed.starts_with("> ") {
        2
    } else if digits > 0 && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") ")) {
        digits + 2
    } else {
        0
    };
    indent + marker
}

/// Break styled text into lines of at most `width` columns at spaces. Escape codes are kept
/// and cost no width; a word longer than a line is split.
pub fn wrap_ansi(text: &str, width: usize) -> Vec<String> {
    if visible_width(text) <= width {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut line_w = 0usize;
    let mut word = String::new();
    let mut word_w = 0usize;
    let mut chars = text.chars().peekable();
    let flush_word = |lines: &mut Vec<String>, line: &mut String, line_w: &mut usize, word: &mut String, word_w: &mut usize| {
        if *line_w > 0 && *line_w + *word_w > width {
            lines.push(std::mem::take(line).trim_end().to_string());
            *line_w = 0;
            let trimmed = word.trim_start().to_string();
            *word_w -= word.len() - trimmed.len();
            *word = trimmed;
        }
        line.push_str(word);
        *line_w += *word_w;
        word.clear();
        *word_w = 0;
    };
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            word.push(c);
            for c in chars.by_ref() {
                word.push(c);
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if c == ' ' && word_w > 0 && !word.ends_with(' ') {
            flush_word(&mut lines, &mut line, &mut line_w, &mut word, &mut word_w);
        }
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if word_w + w > width && line_w == 0 {
            // A single word wider than the line.
            lines.push(std::mem::take(&mut word));
            word_w = 0;
        }
        word.push(c);
        word_w += w;
    }
    flush_word(&mut lines, &mut line, &mut line_w, &mut word, &mut word_w);
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

pub fn pretty_path(path: &Path) -> String {
    let full = path.display().to_string();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = home.to_string_lossy().to_string();
        if !home.is_empty() && full.to_ascii_lowercase().starts_with(&home.to_ascii_lowercase()) {
            return format!("~{}", &full[home.len()..]);
        }
    }
    full
}

pub fn ellipsize_end(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 2 {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max - 1).collect();
    s.push('…');
    s
}

pub fn ellipsize_start(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 2 {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    #[derive(Default)]
    pub struct Coord {
        pub x: i16,
        pub y: i16,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct SmallRect {
        pub left: i16,
        pub top: i16,
        pub right: i16,
        pub bottom: i16,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct ScreenBufferInfo {
        pub size: Coord,
        pub cursor: Coord,
        pub attributes: u16,
        pub window: SmallRect,
        pub max_size: Coord,
    }
    extern "system" {
        pub fn GetStdHandle(n: u32) -> isize;
        pub fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
        pub fn SetConsoleMode(h: isize, mode: u32) -> i32;
        pub fn GetConsoleScreenBufferInfo(h: isize, info: *mut ScreenBufferInfo) -> i32;
    }
    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
}

#[cfg(windows)]
fn enable_vt() -> bool {
    let mut ok = true;
    for which in [win::STD_OUTPUT_HANDLE, win::STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 console calls on the process's own std handles.
        unsafe {
            let h = win::GetStdHandle(which);
            if h == 0 || h == -1 {
                ok = false;
                continue;
            }
            let mut mode = 0u32;
            if win::GetConsoleMode(h, &mut mode) == 0 {
                ok = false;
                continue;
            }
            if mode & win::ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0
                && win::SetConsoleMode(h, mode | win::ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0
            {
                ok = false;
            }
        }
    }
    ok
}

#[cfg(not(windows))]
fn enable_vt() -> bool {
    true
}

#[cfg(windows)]
fn terminal_width() -> usize {
    // SAFETY: reads console geometry for our own stdout handle.
    unsafe {
        let h = win::GetStdHandle(win::STD_OUTPUT_HANDLE);
        let mut info = win::ScreenBufferInfo::default();
        if h != 0 && h != -1 && win::GetConsoleScreenBufferInfo(h, &mut info) != 0 {
            let w = (info.window.right - info.window.left + 1) as usize;
            if w > 0 {
                return w;
            }
        }
    }
    env_columns().unwrap_or(100)
}

#[cfg(not(windows))]
fn terminal_width() -> usize {
    if let Some(c) = env_columns() {
        return c;
    }
    crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(100)
}

fn env_columns() -> Option<usize> {
    std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_read_like_actions() {
        assert_eq!(
            tool_header("run_command", r#"{"command":"cargo test"}"#),
            ("Ran".to_string(), "cargo test".to_string())
        );
        assert_eq!(tool_header("read_file", r#"{"path":"src/a.rs"}"#).0, "Read");
        assert_eq!(tool_header("Read", r#"{"file_path":"src/a.rs"}"#).1, "src/a.rs");
        assert_eq!(tool_header("git_status", "{}").1, "git status");
    }

    #[test]
    fn counts_diff_lines_without_headers() {
        let diff = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n+c\n ctx\n";
        assert_eq!(diff_counts(diff), (2, 1));
    }

    #[test]
    fn inline_markdown_strips_markers() {
        let s = strip_ansi(&inline("use `cargo` **now**, *really*, see [docs](https://x.dev)", ""));
        assert_eq!(s, "use cargo now, really, see docs (https://x.dev)");
        // Snake_case and arithmetic are not emphasis.
        assert_eq!(strip_ansi(&inline("a_b_c and 2 * 3 * 4", "")), "a_b_c and 2 * 3 * 4");
    }

    #[test]
    fn markdown_lists_and_tables() {
        assert_eq!(strip_ansi(&render_markdown_line("1. first", 80)), "1. first");
        assert_eq!(strip_ansi(&render_markdown_line("- item", 80)), "• item");
        assert_eq!(strip_ansi(&render_markdown_line("| a | b |", 80)), "│ a │ b │");
    }

    #[test]
    fn wraps_styled_text_at_spaces() {
        let text = format!("one {BOLD}two{RESET} three four five");
        let lines = wrap_ansi(&text, 10);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain, vec!["one two", "three four", "five"]);
        assert!(lines[0].contains(BOLD));
        assert_eq!(hanging_indent("  - item"), 4);
        assert_eq!(hanging_indent("12. step"), 4);
    }

    #[test]
    fn ellipsizes_both_ends() {
        assert_eq!(ellipsize_end("abcdef", 4), "abc…");
        assert_eq!(ellipsize_start("abcdef", 4), "…def");
    }

    #[test]
    fn themes_swap_the_accent() {
        assert!(set_theme("ocean"));
        let out = themed(&format!("{VIOLET}x"));
        assert!(!out.contains(VIOLET));
        assert!(set_theme("violet"));
        assert!(!set_theme("nope"));
        assert_eq!(visible_width(&format!("{BOLD}ab{RESET}")), 2);
    }
}
