//! The interactive input: a rounded box with a status line under it, history,
//! multi-line messages, and completion lists for `/commands` and `@files`.
//!
//! It draws inline at the bottom of the scrollback (no alternate screen), so the
//! conversation above stays where the terminal put it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::terminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::{self, Term, BOLD, DIM, RESET, VIOLET, VIOLET_LIGHT};

/// Raw keyboard mode for as long as the guard lives.
pub struct RawGuard {
    active: bool,
}

impl RawGuard {
    pub fn new() -> Self {
        let active = terminal::enable_raw_mode().is_ok();
        if active {
            tui::set_raw(true);
            let _ = crossterm::execute!(std::io::stdout(), EnableBracketedPaste);
        }
        Self { active }
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
            let _ = terminal::disable_raw_mode();
            tui::set_raw(false);
        }
    }
}

/// Key presses only; Windows also reports releases.
pub fn is_press(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat
}

/// Wait for y / n. Enter, Esc, and Ctrl+C mean no. Expects raw mode.
pub fn read_yes_no() -> bool {
    loop {
        match event::read() {
            Ok(Event::Key(key)) if is_press(&key) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => return true,
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter | KeyCode::Esc => return false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
                _ => {}
            },
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

pub enum Input {
    Submit(String),
    Exit,
    ClearScreen,
    Expand,
}

/// What the box shows besides your text.
pub struct Prompt {
    pub footer_left: String,
    pub footer_right: String,
    pub notice: Option<String>,
    pub placeholder: String,
}

#[derive(Clone)]
struct Item {
    label: String,
    detail: String,
    insert: String,
}

pub struct Editor {
    history: Vec<String>,
    history_path: PathBuf,
    project: PathBuf,
    exclusions: Vec<String>,
    files: Option<Vec<String>>,
    buf: Vec<char>,
    cursor: usize,
    history_at: Option<usize>,
    draft: Vec<char>,
    items: Vec<Item>,
    selected: usize,
    dismissed: bool,
    notice: Option<String>,
    last_ctrl_c: Option<Instant>,
    /// Rows drawn last time and the row the cursor was left on, counted from the top.
    drawn_rows: usize,
    cursor_row: usize,
}

const MAX_TEXT_ROWS: usize = 12;
const MAX_ITEMS: usize = 8;

impl Editor {
    pub fn new(data_dir: &Path, project: &Path, exclusions: Vec<String>) -> Self {
        let history_path = data_dir.join("cli-history.jsonl");
        let history = std::fs::read_to_string(&history_path)
            .ok()
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<String>(line).ok())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            history,
            history_path,
            project: project.to_path_buf(),
            exclusions,
            files: None,
            buf: Vec::new(),
            cursor: 0,
            history_at: None,
            draft: Vec::new(),
            items: Vec::new(),
            selected: 0,
            dismissed: false,
            notice: None,
            last_ctrl_c: None,
            drawn_rows: 0,
            cursor_row: 0,
        }
    }

    /// Put text in the box for the next read (edit a queued message, for example).
    pub fn prefill(&mut self, text: &str) {
        self.buf = text.chars().collect();
        self.cursor = self.buf.len();
    }

    fn remember(&mut self, text: &str) {
        if text.trim().is_empty() || self.history.last().map(|l| l == text).unwrap_or(false) {
            return;
        }
        self.history.push(text.to_string());
        if self.history.len() > 1000 {
            let drop = self.history.len() - 1000;
            self.history.drain(..drop);
        }
        let body: String = self
            .history
            .iter()
            .filter_map(|line| serde_json::to_string(line).ok())
            .map(|line| line + "\n")
            .collect();
        let _ = std::fs::write(&self.history_path, body);
    }

    /// Read one message. Blocks until Enter, quit, or a key the session handles.
    pub fn read(&mut self, term: &Term, prompt: &Prompt) -> Input {
        if !term.fancy {
            return self.read_plain();
        }
        let _raw = RawGuard::new();
        self.history_at = None;
        self.notice = None;
        self.drawn_rows = 0;
        self.cursor_row = 0;
        self.refresh_items();
        self.render(term, prompt);
        loop {
            let event = match event::read() {
                Ok(event) => event,
                Err(_) => return Input::Exit,
            };
            let outcome = match event {
                Event::Key(key) if is_press(&key) => self.key(key),
                Event::Paste(text) => {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    self.insert_str(&text);
                    None
                }
                Event::Resize(_, _) => None,
                _ => continue,
            };
            if let Some(input) = outcome {
                self.clear(term);
                if let Input::Submit(text) = &input {
                    self.remember(text);
                }
                return input;
            }
            self.refresh_items();
            self.render(term, prompt);
        }
    }

    fn read_plain(&mut self) -> Input {
        print!("\n> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => Input::Exit,
            Ok(_) => Input::Submit(line.trim_end().to_string()),
        }
    }

    fn text(&self) -> String {
        self.buf.iter().collect()
    }

    fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\t' {
                self.buf.insert(self.cursor, ' ');
                self.cursor += 1;
                self.buf.insert(self.cursor, ' ');
            } else if c.is_control() && c != '\n' {
                continue;
            } else {
                self.buf.insert(self.cursor, c);
            }
            self.cursor += 1;
        }
        self.dismissed = false;
        self.history_at = None;
    }

    fn take(&mut self) -> String {
        let text = self.text();
        self.buf.clear();
        self.cursor = 0;
        self.items.clear();
        text
    }

    /// Handle a key. `Some` ends the read.
    fn key(&mut self, key: KeyEvent) -> Option<Input> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if !matches!(key.code, KeyCode::Char('c')) || !ctrl {
            self.notice = None;
        }
        match key.code {
            KeyCode::Enter if shift || alt => self.insert_str("\n"),
            KeyCode::Char('j') if ctrl => self.insert_str("\n"),
            KeyCode::Enter => {
                if let Some(item) = self.items.get(self.selected).cloned() {
                    let is_command = item.insert.starts_with('/');
                    self.apply(&item);
                    // A command without arguments runs right away.
                    let takes_args = tui::COMMANDS
                        .iter()
                        .any(|(cmd, args, _)| *cmd == item.insert.trim() && !args.is_empty());
                    if is_command && !takes_args {
                        return Some(Input::Submit(self.take().trim().to_string()));
                    }
                    return None;
                }
                // A trailing backslash continues on the next line.
                if self.cursor > 0 && self.buf.get(self.cursor - 1) == Some(&'\\') && self.cursor == self.buf.len() {
                    self.buf.remove(self.cursor - 1);
                    self.cursor -= 1;
                    self.insert_str("\n");
                    return None;
                }
                return Some(Input::Submit(self.take()));
            }
            KeyCode::Tab => {
                if let Some(item) = self.items.get(self.selected).cloned() {
                    self.apply(&item);
                }
            }
            KeyCode::Esc => {
                if !self.items.is_empty() {
                    self.dismissed = true;
                } else if !self.buf.is_empty() {
                    self.buf.clear();
                    self.cursor = 0;
                }
            }
            KeyCode::Char('c') if ctrl => {
                if !self.buf.is_empty() {
                    self.buf.clear();
                    self.cursor = 0;
                } else if self
                    .last_ctrl_c
                    .map(|t| t.elapsed() < Duration::from_secs(3))
                    .unwrap_or(false)
                {
                    return Some(Input::Exit);
                } else {
                    self.last_ctrl_c = Some(Instant::now());
                    self.notice = Some("Press Ctrl+C again to quit".into());
                }
            }
            KeyCode::Char('d') if ctrl => {
                if self.buf.is_empty() {
                    return Some(Input::Exit);
                }
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Char('l') if ctrl => return Some(Input::ClearScreen),
            KeyCode::Char('o') if ctrl => return Some(Input::Expand),
            KeyCode::Char('a') if ctrl => self.cursor = self.line_start(),
            KeyCode::Char('e') if ctrl => self.cursor = self.line_end(),
            KeyCode::Char('u') if ctrl => {
                let start = self.line_start();
                self.buf.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char('w') if ctrl => self.delete_word(),
            KeyCode::Backspace if ctrl || alt => self.delete_word(),
            KeyCode::Char(c) if !ctrl || alt => {
                let mut s = [0u8; 4];
                self.insert_str(c.encode_utf8(&mut s));
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buf.remove(self.cursor);
                    self.dismissed = false;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Left if ctrl || alt => self.cursor = self.word_left(),
            KeyCode::Right if ctrl || alt => self.cursor = self.word_right(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.buf.len()),
            KeyCode::Home => self.cursor = self.line_start(),
            KeyCode::End => self.cursor = self.line_end(),
            KeyCode::Up => {
                if !self.items.is_empty() {
                    self.selected = self.selected.checked_sub(1).unwrap_or(self.items.len() - 1);
                } else if self.line_start() == 0 {
                    self.history_prev();
                } else {
                    self.move_vertical(-1);
                }
            }
            KeyCode::Down => {
                if !self.items.is_empty() {
                    self.selected = (self.selected + 1) % self.items.len();
                } else if self.line_end() == self.buf.len() {
                    self.history_next();
                } else {
                    self.move_vertical(1);
                }
            }
            _ => {}
        }
        None
    }

    fn line_start(&self) -> usize {
        self.buf[..self.cursor].iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self) -> usize {
        self.buf[self.cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.buf.len())
    }

    fn move_vertical(&mut self, dir: i32) {
        let col = self.cursor - self.line_start();
        if dir < 0 {
            let start = self.line_start();
            if start == 0 {
                return;
            }
            let prev_end = start - 1;
            let prev_start = self.buf[..prev_end].iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(0);
            self.cursor = prev_start + col.min(prev_end - prev_start);
        } else {
            let end = self.line_end();
            if end >= self.buf.len() {
                return;
            }
            let next_start = end + 1;
            let next_end = self.buf[next_start..].iter().position(|c| *c == '\n').map(|i| next_start + i).unwrap_or(self.buf.len());
            self.cursor = next_start + col.min(next_end - next_start);
        }
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && self.buf[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !self.buf[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        while i < self.buf.len() && self.buf[i].is_whitespace() {
            i += 1;
        }
        while i < self.buf.len() && !self.buf[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn delete_word(&mut self) {
        let start = self.word_left();
        self.buf.drain(start..self.cursor);
        self.cursor = start;
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_at {
            None => {
                self.draft = self.buf.clone();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_at = Some(next);
        self.buf = self.history[next].chars().collect();
        self.cursor = self.buf.len();
    }

    fn history_next(&mut self) {
        let Some(i) = self.history_at else { return };
        if i + 1 < self.history.len() {
            self.history_at = Some(i + 1);
            self.buf = self.history[i + 1].chars().collect();
        } else {
            self.history_at = None;
            self.buf = std::mem::take(&mut self.draft);
        }
        self.cursor = self.buf.len();
    }

    /// The word around the cursor, with where it starts.
    fn token(&self) -> (usize, String) {
        let start = self.buf[..self.cursor].iter().rposition(|c| c.is_whitespace()).map(|i| i + 1).unwrap_or(0);
        (start, self.buf[start..self.cursor].iter().collect())
    }

    fn apply(&mut self, item: &Item) {
        if item.insert.starts_with('/') {
            self.buf = format!("{} ", item.insert).chars().collect();
            self.cursor = self.buf.len();
        } else {
            let (start, _) = self.token();
            let insert: Vec<char> = format!("{} ", item.insert).chars().collect();
            self.buf.splice(start..self.cursor, insert.iter().copied());
            self.cursor = start + insert.len();
        }
        self.items.clear();
        self.dismissed = true;
    }

    fn files(&mut self) -> &[String] {
        if self.files.is_none() {
            let list = serika_runtime::workspace::list_files(&self.project, &self.exclusions, Some(10))
                .map(|files| files.into_iter().take(20_000).map(|f| f.relative.replace('\\', "/")).collect())
                .unwrap_or_default();
            self.files = Some(list);
        }
        self.files.as_deref().unwrap_or(&[])
    }

    fn refresh_items(&mut self) {
        let previous: Vec<String> = self.items.iter().map(|i| i.insert.clone()).collect();
        self.items.clear();
        if self.dismissed {
            return;
        }
        let text = self.text();
        if text.starts_with('/') && !text.contains(char::is_whitespace) {
            let query = text.to_ascii_lowercase();
            self.items = tui::COMMANDS
                .iter()
                .filter(|(cmd, _, _)| cmd.starts_with(&query) && *cmd != "/exit")
                .map(|(cmd, args, help)| Item {
                    label: if args.is_empty() { cmd.to_string() } else { format!("{cmd} {args}") },
                    detail: help.to_string(),
                    insert: cmd.to_string(),
                })
                .collect();
        } else {
            let (_, token) = self.token();
            if let Some(query) = token.strip_prefix('@') {
                let query = query.to_ascii_lowercase();
                let mut hits: Vec<(usize, String)> = self
                    .files()
                    .iter()
                    .filter_map(|path| {
                        let lower = path.to_ascii_lowercase();
                        let name = lower.rsplit('/').next().unwrap_or(&lower);
                        let rank = if query.is_empty() {
                            2
                        } else if name.starts_with(&query) {
                            0
                        } else if name.contains(&query) {
                            1
                        } else if lower.contains(&query) {
                            2
                        } else {
                            return None;
                        };
                        Some((rank * 1000 + path.len(), path.clone()))
                    })
                    .collect();
                hits.sort();
                self.items = hits
                    .into_iter()
                    .take(MAX_ITEMS)
                    .map(|(_, path)| Item {
                        label: format!("@{path}"),
                        detail: String::new(),
                        insert: format!("@{path}"),
                    })
                    .collect();
            }
        }
        let same = previous == self.items.iter().map(|i| i.insert.clone()).collect::<Vec<_>>();
        if !same {
            self.selected = 0;
        }
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }

    /// Wrap the text into display rows and find the cursor's row and column.
    fn layout(&self, width: usize) -> (Vec<String>, usize, usize) {
        let width = width.max(4);
        let mut rows: Vec<String> = vec![String::new()];
        let mut row_width = 0;
        let (mut cur_row, mut cur_col) = (0, 0);
        for (i, c) in self.buf.iter().enumerate() {
            if i == self.cursor {
                cur_row = rows.len() - 1;
                cur_col = row_width;
            }
            if *c == '\n' {
                rows.push(String::new());
                row_width = 0;
                continue;
            }
            let w = c.width().unwrap_or(1);
            if row_width + w > width {
                rows.push(String::new());
                row_width = 0;
            }
            rows.last_mut().unwrap().push(*c);
            row_width += w;
        }
        if self.cursor >= self.buf.len() {
            if row_width >= width {
                rows.push(String::new());
                row_width = 0;
            }
            cur_row = rows.len() - 1;
            cur_col = row_width;
        }
        (rows, cur_row, cur_col)
    }

    fn clear(&mut self, _term: &Term) {
        let mut out = std::io::stdout();
        if self.cursor_row > 0 {
            let _ = write!(out, "\x1b[{}A", self.cursor_row);
        }
        let _ = write!(out, "\r\x1b[J");
        let _ = out.flush();
        self.drawn_rows = 0;
        self.cursor_row = 0;
    }

    fn render(&mut self, term: &Term, prompt: &Prompt) {
        let width = term.width();
        let inner = width.saturating_sub(6).max(10);
        let (rows, cur_row, cur_col) = self.layout(inner);
        // Keep the cursor's row in view when the message is long.
        let first = cur_row.saturating_sub(MAX_TEXT_ROWS - 1).min(rows.len().saturating_sub(MAX_TEXT_ROWS));
        let visible: Vec<&String> = rows.iter().skip(first).take(MAX_TEXT_ROWS).collect();

        let mut lines: Vec<String> = Vec::new();
        let edge = if self.buf.is_empty() { DIM } else { VIOLET };
        let label = if rows.len() > MAX_TEXT_ROWS {
            format!(" {} lines ", rows.len())
        } else {
            String::new()
        };
        lines.push(format!(
            "{edge}╭{}{RESET}{DIM}{label}{RESET}{edge}╮{RESET}",
            "─".repeat(width.saturating_sub(2 + label.width()))
        ));
        for (i, row) in visible.iter().enumerate() {
            let lead = if i == 0 && first == 0 { format!("{VIOLET}{BOLD}›{RESET}") } else { " ".to_string() };
            let (body, used) = if self.buf.is_empty() {
                let hint = tui::ellipsize_end(&prompt.placeholder, inner);
                (format!("{DIM}{hint}{RESET}"), hint.width())
            } else {
                ((*row).clone(), row.width())
            };
            lines.push(format!(
                "{edge}│{RESET} {lead} {body}{}{edge}│{RESET}",
                " ".repeat(inner.saturating_sub(used) + 1)
            ));
        }
        lines.push(format!("{edge}╰{}╯{RESET}", "─".repeat(width.saturating_sub(2))));

        let left = match self.notice.as_ref().or(prompt.notice.as_ref()) {
            Some(notice) => format!("{}{notice}{RESET}", tui::AMBER),
            None => prompt.footer_left.clone(),
        };
        let right = &prompt.footer_right;
        let room = width.saturating_sub(tui::visible_width(right) + 4);
        let left = if tui::visible_width(&left) > room {
            tui::ellipsize_end(&tui::strip_ansi(&left), room)
        } else {
            left
        };
        let gap = width.saturating_sub(tui::visible_width(&left) + tui::visible_width(right) + 3);
        lines.push(format!("  {left}{}{DIM}{right}{RESET}", " ".repeat(gap)));

        for (i, item) in self.items.iter().enumerate() {
            let selected = i == self.selected;
            let label_w = 28usize;
            let label = tui::ellipsize_end(&item.label, if item.detail.is_empty() { width.saturating_sub(8) } else { label_w });
            let detail = tui::ellipsize_end(&item.detail, width.saturating_sub(label_w + 10));
            if selected {
                lines.push(format!(
                    "  {VIOLET}▸{RESET} {BOLD}{VIOLET_LIGHT}{label:<label_w$}{RESET} {detail}",
                ));
            } else {
                lines.push(format!("    {label:<label_w$} {DIM}{detail}{RESET}"));
            }
        }

        let mut out = std::io::stdout();
        let mut frame = String::new();
        if self.cursor_row > 0 {
            frame.push_str(&format!("\x1b[{}A", self.cursor_row));
        }
        frame.push_str("\r\x1b[J");
        frame.push_str(&lines.join("\r\n"));
        let total = lines.len();
        let target_row = 1 + (cur_row - first);
        let up = total - 1 - target_row;
        if up > 0 {
            frame.push_str(&format!("\x1b[{up}A"));
        }
        // "│ › " is four columns wide.
        frame.push_str(&format!("\r\x1b[{}G", 5 + cur_col));
        let _ = out.write_all(tui::themed(&frame).replace("\r\r\n", "\r\n").as_bytes());
        let _ = out.flush();
        self.drawn_rows = total;
        self.cursor_row = target_row;
    }
}

/// One entry in an arrow-key list.
pub struct PickItem {
    pub section: String,
    pub label: String,
    pub detail: String,
    pub marker: String,
}

/// Choose from a list with ↑↓, typing filters it, Enter picks, Esc cancels.
pub fn pick(term: &Term, title: &str, items: &[PickItem], current: usize) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    if !term.fancy {
        for (i, item) in items.iter().enumerate() {
            println!("{:>3}. {} {}", i + 1, item.label, item.detail);
        }
        print!("number> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        return line.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1)).filter(|n| *n < items.len());
    }
    let _raw = RawGuard::new();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide);
    let mut filter = String::new();
    let mut selected = current.min(items.len() - 1);
    let mut drawn = 0usize;
    let window = 14usize;
    let result = loop {
        let matches: Vec<usize> = (0..items.len())
            .filter(|i| {
                filter.is_empty() || {
                    let hay = format!("{} {} {}", items[*i].section, items[*i].label, items[*i].detail).to_ascii_lowercase();
                    filter.to_ascii_lowercase().split_whitespace().all(|w| hay.contains(w))
                }
            })
            .collect();
        let pos = matches.iter().position(|i| *i == selected).unwrap_or(0);
        if let Some(first) = matches.get(pos) {
            selected = *first;
        }
        let start = pos.saturating_sub(window / 2).min(matches.len().saturating_sub(window));
        let width = term.width();
        let mut lines = vec![format!(
            "{BOLD}{title}{RESET}  {DIM}↑↓ move · enter select · esc cancel · type to filter{RESET}  {VIOLET_LIGHT}{filter}{RESET}"
        )];
        let mut last_section = String::new();
        for &i in matches.iter().skip(start).take(window) {
            let item = &items[i];
            if item.section != last_section {
                lines.push(format!("  {DIM}{}{RESET}", item.section));
                last_section = item.section.clone();
            }
            let label = tui::ellipsize_end(&item.label, 34);
            let detail = tui::ellipsize_end(&item.detail, width.saturating_sub(48));
            if i == selected {
                lines.push(format!("  {VIOLET}▸{RESET} {BOLD}{VIOLET_LIGHT}{label:<34}{RESET} {detail} {}", item.marker));
            } else {
                lines.push(format!("    {label:<34} {DIM}{detail}{RESET} {}", item.marker));
            }
        }
        if matches.is_empty() {
            lines.push(format!("  {DIM}Nothing matches “{filter}”.{RESET}"));
        } else if matches.len() > window {
            lines.push(format!("  {DIM}{} of {}{RESET}", pos + 1, matches.len()));
        }
        let mut frame = String::new();
        if drawn > 1 {
            frame.push_str(&format!("\x1b[{}A", drawn - 1));
        }
        frame.push_str("\r\x1b[J");
        frame.push_str(&lines.join("\r\n"));
        tui::write_out(&frame);
        drawn = lines.len();

        let key = match event::read() {
            Ok(Event::Key(key)) if is_press(&key) => key,
            Ok(_) => continue,
            Err(_) => break None,
        };
        let step = |delta: i64| -> usize {
            if matches.is_empty() {
                return selected;
            }
            let next = (pos as i64 + delta).rem_euclid(matches.len() as i64) as usize;
            matches[next]
        };
        match key.code {
            KeyCode::Esc => break None,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break None,
            KeyCode::Enter => break matches.get(pos).copied(),
            KeyCode::Up => selected = step(-1),
            KeyCode::Down | KeyCode::Tab => selected = step(1),
            KeyCode::PageUp => selected = step(-(window as i64)),
            KeyCode::PageDown => selected = step(window as i64),
            KeyCode::Backspace => {
                filter.pop();
            }
            KeyCode::Char(c) => filter.push(c),
            _ => {}
        }
    };
    let mut frame = String::new();
    if drawn > 1 {
        frame.push_str(&format!("\x1b[{}A", drawn - 1));
    }
    frame.push_str("\r\x1b[J");
    tui::write_out(&frame);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Editor {
        let dir = std::env::temp_dir();
        let mut e = Editor::new(&dir.join("serika-test-none"), &dir, Vec::new());
        e.prefill(text);
        e
    }

    #[test]
    fn wraps_and_tracks_the_cursor() {
        let e = editor("abcdef\nxy");
        let (rows, row, col) = e.layout(4);
        assert_eq!(rows, vec!["abcd", "ef", "xy"]);
        assert_eq!((row, col), (2, 2));
    }

    #[test]
    fn slash_lists_matching_commands() {
        let mut e = editor("/mo");
        e.refresh_items();
        assert_eq!(e.items.first().map(|i| i.insert.as_str()), Some("/model"));
        e.prefill("/model x");
        e.refresh_items();
        assert!(e.items.is_empty());
    }

    #[test]
    fn words_and_lines_move_the_cursor() {
        let mut e = editor("one two\nthree");
        assert_eq!(e.word_left(), 8);
        e.cursor = 3;
        e.move_vertical(1);
        assert_eq!(e.cursor, 11);
        e.delete_word();
        assert_eq!(e.text(), "one two\nee");
    }
}
