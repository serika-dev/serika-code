//! Terminal presentation for the `serika` CLI.
//!
//! The interactive session borrows its layout from modern agent CLIs: a boxed
//! header, a `›` prompt with a footer hint, bulleted assistant turns, compact
//! tool cells with a `└` output tree, coloured diffs, and a live spinner line.
//!
//! Everything is plain ANSI. When stdout or stderr is not a terminal the
//! renderer falls back to undecorated output so pipes and scripts keep working.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Instant;

use serde_json::Value;

// Serika brand colours (truecolor).
const VIOLET: &str = "\x1b[38;2;139;92;246m"; // #8B5CF6
const VIOLET_LIGHT: &str = "\x1b[38;2;167;139;250m"; // #A78BFA
const GREEN: &str = "\x1b[38;2;52;211;153m";
const RED: &str = "\x1b[38;2;248;113;113m";
const AMBER: &str = "\x1b[38;2;251;191;36m";
const DIFF_ADD: &str = "\x1b[38;2;134;239;172m\x1b[48;2;14;38;24m";
const DIFF_DEL: &str = "\x1b[38;2;252;165;165m\x1b[48;2;48;16;20m";
const USER_BG: &str = "\x1b[48;2;30;30;36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const RESET: &str = "\x1b[0m";
const CLEAR_LINE: &str = "\r\x1b[2K";

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Shared terminal capabilities.
#[derive(Clone, Copy)]
pub struct Term {
    pub fancy: bool,
}

impl Term {
    pub fn detect() -> Self {
        let tty = std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
        let disabled = std::env::var_os("NO_COLOR").is_some()
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

/// Session facts shown in the header box.
pub struct Banner<'a> {
    pub version: &'a str,
    pub model: &'a str,
    pub directory: &'a Path,
    pub permission: &'a str,
    pub account: &'a str,
    pub session: Option<&'a str>,
}

pub fn print_banner(term: &Term, info: &Banner<'_>) {
    let mut out = std::io::stdout();
    if !term.fancy {
        let _ = writeln!(out, "Serika Code {}", info.version);
        let _ = writeln!(out, "model: {}", info.model);
        let _ = writeln!(out, "directory: {}", info.directory.display());
        let _ = writeln!(out, "approvals: {}", info.permission);
        let _ = writeln!(out, "account: {}", info.account);
        let _ = writeln!(out, "Type a task, /help for commands, /exit to leave.");
        return;
    }

    let inner = (term.width().saturating_sub(4)).min(72);
    let dir = pretty_path(info.directory);
    let mut rows: Vec<(String, usize)> = Vec::new();
    let title_plain = format!(">_ Serika Code (v{})", info.version);
    let title = format!(
        "{VIOLET}{BOLD}>_{RESET} {BOLD}Serika Code{RESET} {DIM}(v{}){RESET}",
        info.version
    );
    rows.push((title, title_plain.chars().count()));
    rows.push((String::new(), 0));
    let fields: [(&str, String, &str); 4] = [
        ("model", info.model.to_string(), "/model to change"),
        ("directory", dir, ""),
        ("approvals", info.permission.to_string(), "/approvals to change"),
        ("account", info.account.to_string(), ""),
    ];
    for (label, value, hint) in fields.iter() {
        let label_col = format!("{label}:");
        let budget = inner.saturating_sub(13 + hint.chars().count() + 2);
        let value = ellipsize_start(value, budget.max(8));
        let plain = if hint.is_empty() {
            format!("{label_col:<12} {value}")
        } else {
            format!("{label_col:<12} {value}   {hint}")
        };
        let styled = if hint.is_empty() {
            format!("{DIM}{label_col:<12}{RESET} {value}")
        } else {
            format!("{DIM}{label_col:<12}{RESET} {value}   {VIOLET_LIGHT}{hint}{RESET}")
        };
        rows.push((styled, plain.chars().count()));
    }
    if let Some(id) = info.session {
        let plain = format!("{:<12} {id}", "session:");
        let styled = format!("{DIM}{:<12}{RESET} {DIM}{id}{RESET}", "session:");
        rows.push((styled, plain.chars().count()));
    }

    let border = |s: &str| format!("{DIM}{s}{RESET}");
    let _ = writeln!(out, "{}", border(&format!("╭{}╮", "─".repeat(inner + 2))));
    for (styled, plain_len) in rows {
        let pad = inner.saturating_sub(plain_len);
        let _ = writeln!(
            out,
            "{} {styled}{} {}",
            border("│"),
            " ".repeat(pad),
            border("│")
        );
    }
    let _ = writeln!(out, "{}", border(&format!("╰{}╯", "─".repeat(inner + 2))));
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  {DIM}To get started, describe a task or try one of these commands:{RESET}"
    );
    let _ = writeln!(out);
    for (cmd, help) in QUICK_COMMANDS {
        let _ = writeln!(out, "  {VIOLET_LIGHT}{cmd:<12}{RESET} {DIM}{help}{RESET}");
    }
    let _ = writeln!(out);
    let _ = out.flush();
}

const QUICK_COMMANDS: [(&str, &str); 6] = [
    ("/model", "choose what model to use"),
    ("/status", "show the current session configuration"),
    ("/diff", "show the git diff, including untracked files"),
    ("/review", "review the current changes and find issues"),
    ("/new", "start a fresh conversation"),
    ("/help", "list every command"),
];

pub const ALL_COMMANDS: [(&str, &str); 13] = [
    ("/model [id]", "list models, or switch to <id>"),
    ("/approvals [mode]", "read_only, standard, restricted, or full"),
    ("/status", "show the current session configuration"),
    ("/diff", "show the git diff, including untracked files"),
    ("/review [focus]", "ask the agent to review uncommitted changes"),
    ("/new", "start a fresh conversation"),
    ("/sessions", "list saved sessions for this project"),
    ("/resume <id>", "continue a saved session"),
    ("/models", "list every discovered model"),
    ("/usage", "show Serika Cloud usage"),
    ("/clear", "clear the screen"),
    ("/help", "show this list"),
    ("/exit", "leave Serika Code"),
];

pub fn print_help(term: &Term) {
    let mut out = std::io::stdout();
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", term.bold("Commands"));
    for (cmd, help) in ALL_COMMANDS {
        let _ = writeln!(out, "  {}  {}", term.accent(&format!("{cmd:<18}")), term.dim(help));
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", term.bold("Keys"));
    let _ = writeln!(
        out,
        "  {}  {}",
        term.accent(&format!("{:<18}", "Enter")),
        term.dim("send the task")
    );
    let _ = writeln!(
        out,
        "  {}  {}",
        term.accent(&format!("{:<18}", "Ctrl+C")),
        term.dim("interrupt the running task")
    );
    let _ = writeln!(
        out,
        "  {}  {}",
        term.accent(&format!("{:<18}", "Ctrl+D / /exit")),
        term.dim("quit")
    );
    let _ = writeln!(out);
}

/// Draw the prompt with a footer hint underneath and leave the cursor on the
/// prompt line. Returns the number of terminal rows the footer occupies.
pub fn draw_prompt(term: &Term, model: &str, permission: &str) {
    let mut out = std::io::stdout();
    if !term.fancy {
        let _ = write!(out, "\n> ");
        let _ = out.flush();
        return;
    }
    let left = "⏎ send · /help commands · Ctrl+C interrupt";
    let right = format!("{model} · {permission}");
    let width = term.width();
    let gap = width
        .saturating_sub(2 + left.chars().count() + right.chars().count() + 2)
        .max(2);
    let _ = write!(
        out,
        "\n\n  {DIM}{left}{}{right}{RESET}\x1b[1A\r{VIOLET}{BOLD}›{RESET} ",
        " ".repeat(gap)
    );
    let _ = out.flush();
}

/// After the user pressed Enter, replace the raw echo and the footer with a
/// styled user cell.
pub fn echo_user(term: &Term, text: &str) {
    if !term.fancy {
        return;
    }
    let width = term.width();
    let rows = (text.chars().count() + 2).div_ceil(width).max(1);
    let mut out = std::io::stdout();
    // Cursor is on the line after the input (the footer line). Clear it, then
    // climb over the input rows and clear those too.
    let _ = write!(out, "{CLEAR_LINE}");
    for _ in 0..rows {
        let _ = write!(out, "\x1b[1A{CLEAR_LINE}");
    }
    let body = format!("› {text}");
    let pad = width.saturating_sub(body.chars().count() % width.max(1));
    let _ = writeln!(out, "{USER_BG}{BOLD}{body}{RESET}{USER_BG}{}{RESET}", " ".repeat(pad.min(width)));
    let _ = writeln!(out);
    let _ = out.flush();
}

pub fn clear_screen(term: &Term) {
    if term.fancy {
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[2J\x1b[3J\x1b[H");
        let _ = out.flush();
    }
}

pub fn info_line(term: &Term, text: &str) {
    eprintln!("  {}", term.dim(text));
}

pub fn error_line(term: &Term, text: &str) {
    eprintln!("{} {}", term.red("■"), text);
}

/// Streams one agent turn into the terminal.
pub struct TurnRenderer {
    term: Term,
    started: Instant,
    status: String,
    tick: usize,
    spinner_drawn: bool,
    line_buf: String,
    in_fence: bool,
    msg_started: bool,
    printed_any: bool,
    tools: HashMap<String, (String, String)>,
    tool_output: HashMap<String, Vec<String>>,
    usage: Option<String>,
}

impl TurnRenderer {
    pub fn new(term: Term) -> Self {
        if term.fancy {
            let mut err = std::io::stderr();
            let _ = write!(err, "\x1b[?25l");
            let _ = err.flush();
        }
        Self {
            term,
            started: Instant::now(),
            status: "Working".into(),
            tick: 0,
            spinner_drawn: false,
            line_buf: String::new(),
            in_fence: false,
            msg_started: false,
            printed_any: false,
            tools: HashMap::new(),
            tool_output: HashMap::new(),
            usage: None,
        }
    }

    fn clear_spinner(&mut self) {
        if self.spinner_drawn {
            let mut err = std::io::stderr();
            let _ = write!(err, "{CLEAR_LINE}");
            let _ = err.flush();
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
        let label = ellipsize_end(&self.status, width.saturating_sub(40).max(12));
        let mut err = std::io::stderr();
        let _ = write!(
            err,
            "{CLEAR_LINE}{VIOLET}{frame}{RESET} {BOLD}{label}{RESET} {DIM}({elapsed} • Ctrl+C to interrupt){RESET}"
        );
        let _ = err.flush();
        self.spinner_drawn = true;
    }

    pub fn set_status(&mut self, status: &str) {
        let s = status.trim();
        if s.is_empty() {
            return;
        }
        if !self.term.fancy {
            eprintln!("{s}");
            return;
        }
        // Capitalise the first letter so it reads like a sentence.
        let mut chars = s.chars();
        self.status = match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        self.tick();
    }

    pub fn session_started(&mut self, id: &str) {
        if !self.term.fancy {
            eprintln!("session {id}");
        }
    }

    pub fn message_chunk(&mut self, text: &str) {
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
        if self.msg_started {
            println!();
        }
        self.msg_started = false;
        self.in_fence = false;
    }

    fn emit_message_line(&mut self, line: &str) {
        self.clear_spinner();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            self.in_fence = !self.in_fence;
            if !self.msg_started {
                // Keep the bullet alignment even if a reply opens with a fence.
                println!("{VIOLET}•{RESET}");
                self.msg_started = true;
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
            format!("{VIOLET_LIGHT}•{RESET} ")
        };
        let body = if self.in_fence {
            format!("{VIOLET_LIGHT}{line}{RESET}")
        } else {
            render_markdown_line(line)
        };
        println!("{prefix}{body}");
        let _ = std::io::stdout().flush();
        self.printed_any = true;
    }

    pub fn tool_requested(&mut self, id: &str, name: &str, arguments: &str) {
        self.tools
            .insert(id.to_string(), (name.to_string(), arguments.to_string()));
        let (verb, detail) = tool_header(name, arguments);
        self.status = if detail.is_empty() {
            verb_progressive(&verb)
        } else {
            format!("{} {}", verb_progressive(&verb), detail)
        };
        if !self.term.fancy {
            eprintln!("tool {name}");
        }
    }

    pub fn tool_output(&mut self, id: &str, stream: &str) {
        if !self.term.fancy {
            eprintln!("{stream}");
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
            eprintln!("{name} {mark}: {summary}");
            return;
        }
        self.flush_partial_message();
        self.clear_spinner();
        let args = self
            .tools
            .get(id)
            .map(|(_, a)| a.clone())
            .unwrap_or_default();
        let (verb, detail) = tool_header(name, &args);
        // Edits are rendered by `file_changed` with the diff itself.
        let is_edit = matches!(
            name,
            "write_file" | "replace_in_file" | "apply_patch"
        );
        if is_edit && ok {
            self.status = "Working".into();
            return;
        }
        let bullet = if ok {
            format!("{GREEN}•{RESET}")
        } else {
            format!("{RED}•{RESET}")
        };
        let detail_fmt = if name == "run_command" || name.starts_with("git_") {
            format!("{VIOLET_LIGHT}{detail}{RESET}")
        } else {
            detail.clone()
        };
        println!("{bullet} {BOLD}{verb}{RESET} {detail_fmt}");

        let mut lines: Vec<String> = self.tool_output.remove(id).unwrap_or_default();
        if lines.is_empty() && (!ok || matches!(name, "run_command" | "git_status" | "git_log" | "git_commit")) {
            lines = summary.lines().map(|l| l.to_string()).collect();
        }
        lines.retain(|l| !l.trim().is_empty());
        let width = self.term.width().saturating_sub(8);
        let max = if ok { 4 } else { 8 };
        let total = lines.len();
        for (i, l) in lines.iter().take(max).enumerate() {
            let tree = if i == 0 { "└" } else { " " };
            let colour = if ok { DIM } else { RED };
            println!("  {DIM}{tree}{RESET} {colour}{}{RESET}", ellipsize_end(l, width));
        }
        if total > max {
            println!("    {DIM}{ITALIC}… +{} lines{RESET}", total - max);
        }
        println!();
        let _ = std::io::stdout().flush();
        self.printed_any = true;
        self.status = "Working".into();
    }

    pub fn file_changed(&mut self, path: &str, diff: &str) {
        if !self.term.fancy {
            eprintln!("changed {path}");
            println!("{diff}");
            return;
        }
        self.flush_partial_message();
        self.clear_spinner();
        let (adds, dels) = diff_counts(diff);
        println!(
            "{GREEN}•{RESET} {BOLD}Edited{RESET} {path} {DIM}({RESET}{GREEN}+{adds}{RESET} {RED}-{dels}{RESET}{DIM}){RESET}"
        );
        print_diff_body(&self.term, diff, 48);
        println!();
        let _ = std::io::stdout().flush();
        self.printed_any = true;
    }

    pub fn usage(&mut self, summary: String) {
        if !self.term.fancy {
            eprintln!("{summary}");
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
            println!("{RED}■{RESET} {message}");
            println!();
        } else {
            eprintln!("serika: {message}");
        }
    }

    pub fn cancelled(&mut self) {
        self.flush_partial_message();
        self.clear_spinner();
        if self.term.fancy {
            println!("{AMBER}■{RESET} {BOLD}Interrupted{RESET} {DIM}— tell Serika what to do differently.{RESET}");
            println!();
        } else {
            eprintln!("cancelled");
        }
    }

    /// Pause the spinner and show the cursor (for approval prompts).
    pub fn suspend(&mut self) {
        self.flush_partial_message();
        self.clear_spinner();
        if self.term.fancy {
            let mut err = std::io::stderr();
            let _ = write!(err, "\x1b[?25h");
            let _ = err.flush();
        }
    }

    pub fn resume(&mut self) {
        if self.term.fancy {
            let mut err = std::io::stderr();
            let _ = write!(err, "\x1b[?25l");
            let _ = err.flush();
        }
    }

    fn flush_partial_message(&mut self) {
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.emit_message_line(&line);
        }
        if self.msg_started {
            println!();
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
        let mut footer = format!("Worked for {secs:.1}s");
        if let Some(u) = &self.usage {
            footer.push_str(" · ");
            footer.push_str(u);
        }
        let width = self.term.width();
        let label = format!(" {footer} ");
        let side = width.saturating_sub(label.chars().count() + 2) / 2;
        let line = format!(
            "{DIM}{}{label}{}{RESET}",
            "─".repeat(side.max(2)),
            "─".repeat(side.max(2))
        );
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{line}");
        let _ = write!(err, "\x1b[?25h");
        let _ = err.flush();
    }
}

impl Drop for TurnRenderer {
    fn drop(&mut self) {
        if self.term.fancy {
            let mut err = std::io::stderr();
            let _ = write!(err, "\x1b[?25h");
            let _ = err.flush();
        }
    }
}

/// Codex-style approval prompt. Returns true when the user allows the action.
pub fn ask_approval(term: &Term, title: &str, detail: &str, destructive: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        eprintln!("approval required to {title}: {detail}");
        return false;
    }
    let mut err = std::io::stderr();
    if !term.fancy {
        let kind = if destructive { "This can be destructive. " } else { "" };
        let _ = write!(err, "{kind}Allow {title}?\n{detail}\n[y/N] ");
        let _ = err.flush();
    } else {
        let bar_col = if destructive { RED } else { VIOLET };
        let heading = if destructive {
            format!("{RED}{BOLD}Allow {title}?{RESET} {RED}(destructive){RESET}")
        } else {
            format!("{BOLD}Allow {title}?{RESET}")
        };
        let width = term.width().saturating_sub(6);
        let _ = writeln!(err, "{bar_col}▌{RESET} {heading}");
        for line in detail.lines().take(12) {
            let _ = writeln!(err, "{bar_col}▌{RESET} {DIM}{}{RESET}", ellipsize_end(line, width));
        }
        let _ = writeln!(err, "{bar_col}▌{RESET}");
        let _ = write!(
            err,
            "{bar_col}▌{RESET} {VIOLET_LIGHT}y{RESET} yes   {VIOLET_LIGHT}n{RESET} no {DIM}(default){RESET}   {VIOLET}{BOLD}›{RESET} "
        );
        let _ = err.flush();
    }
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    let allow = matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if term.fancy {
        let verdict = if allow {
            format!("{GREEN}✔ Approved{RESET}")
        } else {
            format!("{RED}✗ Denied{RESET}")
        };
        let _ = writeln!(err, "  {verdict} {DIM}{title}{RESET}\n");
    }
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
            println!(
                "{VIOLET_LIGHT}•{RESET} {BOLD}{p}{RESET} {DIM}({RESET}{GREEN}+{a}{RESET} {RED}-{d}{RESET}{DIM}){RESET}"
            );
            print_diff_body(term, chunk, 400);
            println!();
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
        print_diff_body(term, &chunk, 400);
    } else {
        flush(&current, &chunk);
    }
}

fn print_diff_body(term: &Term, diff: &str, max_lines: usize) {
    let width = term.width().saturating_sub(10);
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
            // @@ -a,b +c,d @@
            let mut parts = line.split_whitespace();
            let _ = parts.next();
            if let Some(old) = parts.next() {
                old_no = old.trim_start_matches('-').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            }
            if let Some(new) = parts.next() {
                new_no = new.trim_start_matches('+').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            }
            if shown > 0 && shown < max_lines {
                println!("    {DIM}⋮{RESET}");
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
            println!("  {DIM}{new_no:>4}{RESET} {DIFF_ADD}+{text}{RESET}");
            new_no += 1;
        } else if line.starts_with('-') {
            println!("  {DIM}{old_no:>4}{RESET} {DIFF_DEL}-{text}{RESET}");
            old_no += 1;
        } else {
            println!("  {DIM}{new_no:>4}  {text}{RESET}");
            old_no += 1;
            new_no += 1;
        }
    }
    if hidden > 0 {
        println!("    {DIM}{ITALIC}… +{hidden} lines{RESET}");
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

/// Turn a tool call into a past-tense verb and a short detail string.
fn tool_header(name: &str, arguments: &str) -> (String, String) {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match name {
        "run_command" => ("Ran".into(), s("command")),
        "read_file" => ("Read".into(), s("path")),
        "list_dir" => {
            let p = s("path");
            ("Explored".into(), if p.is_empty() { ".".into() } else { p })
        }
        "search_text" => ("Searched".into(), s("query").if_empty(s("pattern"))),
        "search_files" => ("Searched files".into(), s("query").if_empty(s("pattern"))),
        "write_file" | "replace_in_file" | "apply_patch" => ("Edited".into(), s("path")),
        "delete_file" => ("Deleted".into(), s("path")),
        "move_file" => ("Moved".into(), format!("{} → {}", s("from"), s("to"))),
        "git_status" => ("Ran".into(), "git status".into()),
        "git_diff" => ("Ran".into(), format!("git diff {}", s("path")).trim().to_string()),
        "git_log" => ("Ran".into(), "git log".into()),
        "git_commit" => ("Committed".into(), s("message")),
        other => ("Called".into(), other.to_string()),
    }
}

fn verb_progressive(verb: &str) -> String {
    match verb {
        "Ran" => "Running".into(),
        "Read" => "Reading".into(),
        "Explored" => "Exploring".into(),
        "Searched" | "Searched files" => "Searching".into(),
        "Edited" => "Editing".into(),
        "Deleted" => "Deleting".into(),
        "Moved" => "Moving".into(),
        "Committed" => "Committing".into(),
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

/// Minimal inline markdown: headings, bullets, `code`, and **bold**.
fn render_markdown_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if let Some(rest) = trimmed
        .strip_prefix("### ")
        .or_else(|| trimmed.strip_prefix("## "))
        .or_else(|| trimmed.strip_prefix("# "))
    {
        return format!("{indent}{BOLD}{}{RESET}", inline(rest, BOLD));
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return format!("{indent}{DIM}-{RESET} {}", inline(rest, ""));
    }
    if trimmed.starts_with("> ") {
        return format!("{indent}{DIM}▌ {}{RESET}", &trimmed[2..]);
    }
    format!("{indent}{}", inline(trimmed, ""))
}

fn inline(text: &str, base: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                out.push_str(VIOLET_LIGHT);
                out.push_str(&after[..end]);
                out.push_str(RESET);
                out.push_str(base);
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                out.push_str(BOLD);
                out.push_str(&after[..end]);
                out.push_str(RESET);
                out.push_str(base);
                rest = &after[end + 2..];
                continue;
            }
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
        }
        rest = chars.as_str();
    }
    out
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

fn ellipsize_end(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 2 {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max - 1).collect();
    s.push('…');
    s
}

fn ellipsize_start(text: &str, max: usize) -> String {
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
    std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .nth(1)
                .and_then(|w| w.parse().ok())
        })
        .unwrap_or(100)
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
        assert_eq!(tool_header("git_status", "{}").1, "git status");
    }

    #[test]
    fn counts_diff_lines_without_headers() {
        let diff = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n+c\n ctx\n";
        assert_eq!(diff_counts(diff), (2, 1));
    }

    #[test]
    fn inline_markdown_strips_markers() {
        let s = inline("use `cargo` **now**", "");
        assert!(s.contains("cargo") && s.contains("now"));
        assert!(!s.contains('`') && !s.contains("**"));
    }

    #[test]
    fn ellipsizes_both_ends() {
        assert_eq!(ellipsize_end("abcdef", 4), "abc…");
        assert_eq!(ellipsize_start("abcdef", 4), "…def");
    }
}
