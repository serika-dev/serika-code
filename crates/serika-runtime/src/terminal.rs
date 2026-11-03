//! Terminals for the desktop's Terminal panel.
//!
//! Each terminal is a persistent shell (PowerShell 7 when installed, else Windows PowerShell;
//! the login shell elsewhere) reading commands from a pipe, so `cd`, variables, and history
//! carry over between commands. After every command the shell prints a marker with the exit
//! code and the current folder, which ends the command in the panel. Interrupting kills the
//! whole process tree and starts a fresh shell in the same folder. There is no PTY, so
//! full-screen programs (vim, top) do not work; everything that prints lines does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Text the shell printed. `error` is true for stderr.
    Output { id: String, text: String, error: bool },
    /// A command finished.
    Done { id: String, code: i32, cwd: String },
    /// The shell itself ended.
    Exited { id: String },
}

pub type Sink = Arc<dyn Fn(TerminalEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Shell {
    PowerShell,
    Posix,
}

struct Term {
    id: String,
    shell: Shell,
    program: PathBuf,
    marker: String,
    cwd: StdMutex<PathBuf>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pid: AtomicU32,
    running: AtomicBool,
}

#[derive(Default)]
pub struct TerminalHost {
    terms: Mutex<HashMap<String, Arc<Term>>>,
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn pick_shell() -> (Shell, PathBuf) {
    if cfg!(windows) {
        let program = find_on_path(&["pwsh.exe"])
            .or_else(|| find_on_path(&["powershell.exe"]))
            .unwrap_or_else(|| PathBuf::from("powershell.exe"));
        (Shell::PowerShell, program)
    } else {
        let program = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| find_on_path(&["bash", "sh"]))
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        (Shell::Posix, program)
    }
}

/// Drop ANSI colour and cursor codes; the panel shows plain text.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c == '\r' {
            // Keep CRLF as LF; a bare CR (progress bars) becomes a line break too.
            if chars.peek() != Some(&'\n') {
                out.push('\n');
            }
        } else if c != '\x07' && c != '\x08' {
            out.push(c);
        }
    }
    out
}

/// Split output into text before a done-marker and the marker's exit code and folder.
fn take_marker(buffer: &mut String, marker: &str) -> Option<(String, i32, String)> {
    let at = buffer.find(marker)?;
    let line_end = buffer[at..].find('\n').map(|i| at + i)?;
    let mut before = buffer[..at].to_string();
    if before.ends_with('\n') {
        before.pop();
    }
    let line = buffer[at + marker.len()..line_end].trim().to_string();
    buffer.replace_range(..=line_end, "");
    let (code, cwd) = line.split_once(' ').unwrap_or((line.as_str(), ""));
    Some((before, code.trim().parse().unwrap_or(1), cwd.trim().to_string()))
}

impl TerminalHost {
    /// Start a terminal in `cwd`. Returns its id and the shell's name.
    pub async fn open(&self, cwd: &Path, sink: Sink) -> Result<(String, String)> {
        let (shell, program) = pick_shell();
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let term = Arc::new(Term {
            id: id.clone(),
            shell,
            program: program.clone(),
            marker: format!("__SERIKA_DONE_{}__", &uuid::Uuid::new_v4().simple().to_string()[..10]),
            cwd: StdMutex::new(cwd.to_path_buf()),
            stdin: Mutex::new(None),
            child: Mutex::new(None),
            pid: AtomicU32::new(0),
            running: AtomicBool::new(false),
        });
        spawn_shell(&term, sink).await?;
        self.terms.lock().await.insert(id.clone(), term);
        let name = program.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Ok((id, name))
    }

    async fn get(&self, id: &str) -> Result<Arc<Term>> {
        self.terms.lock().await.get(id).cloned().ok_or_else(|| Error::msg("that terminal is closed"))
    }

    /// Run one command line. Output arrives through the sink; `Done` ends it.
    pub async fn run(&self, id: &str, command: &str) -> Result<()> {
        let term = self.get(id).await?;
        if term.running.load(Ordering::SeqCst) {
            return Err(Error::msg("a command is still running; stop it first"));
        }
        let line = command.replace("\r\n", "\n").replace('\n', "; ");
        let script = match term.shell {
            Shell::PowerShell => format!(
                "{line}\n$__ok = $?; $__code = if ($__ok) {{ 0 }} elseif ($LASTEXITCODE) {{ $LASTEXITCODE }} else {{ 1 }}; [Console]::Out.WriteLine(\"`n{} \" + $__code + \" \" + (Get-Location).Path)\n",
                term.marker
            ),
            Shell::Posix => format!("{line}\nprintf '\\n{} %s %s\\n' \"$?\" \"$PWD\"\n", term.marker),
        };
        term.running.store(true, Ordering::SeqCst);
        let mut stdin = term.stdin.lock().await;
        let pipe = stdin.as_mut().ok_or_else(|| Error::msg("the shell is not running"))?;
        pipe.write_all(script.as_bytes()).await?;
        pipe.flush().await?;
        Ok(())
    }

    /// Stop what is running: end the shell and everything it started, then start a fresh one.
    pub async fn interrupt(&self, id: &str, sink: Sink) -> Result<()> {
        let term = self.get(id).await?;
        kill_tree(&term).await;
        let cwd = term.cwd.lock().unwrap_or_else(|e| e.into_inner()).display().to_string();
        term.running.store(false, Ordering::SeqCst);
        sink(TerminalEvent::Output { id: term.id.clone(), text: "^C\n".into(), error: true });
        sink(TerminalEvent::Done { id: term.id.clone(), code: 130, cwd });
        spawn_shell(&term, sink).await
    }

    pub async fn close(&self, id: &str) {
        if let Some(term) = self.terms.lock().await.remove(id) {
            kill_tree(&term).await;
        }
    }

    pub async fn close_all(&self) {
        let all: Vec<Arc<Term>> = self.terms.lock().await.drain().map(|(_, t)| t).collect();
        for term in all {
            kill_tree(&term).await;
        }
    }
}

async fn kill_tree(term: &Term) {
    *term.stdin.lock().await = None;
    let pid = term.pid.load(Ordering::SeqCst);
    if pid != 0 && cfg!(windows) {
        let mut taskkill = Command::new("taskkill");
        taskkill.args(["/PID", &pid.to_string(), "/T", "/F"]).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        taskkill.creation_flags(0x08000000);
        let _ = taskkill.status().await;
    }
    if let Some(mut child) = term.child.lock().await.take() {
        let _ = child.kill().await;
    }
}

async fn spawn_shell(term: &Arc<Term>, sink: Sink) -> Result<()> {
    let cwd = term.cwd.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut command = Command::new(&term.program);
    match term.shell {
        Shell::PowerShell => {
            command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"]);
        }
        Shell::Posix => {
            command.arg("-s");
        }
    }
    command
        .current_dir(&cwd)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|err| Error::msg(format!("could not start {}: {err}", term.program.display())))?;
    #[cfg(windows)]
    crate::browser::job::adopt(&child);
    term.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
    let mut stdin = child.stdin.take().ok_or_else(|| Error::msg("no shell input"))?;
    if term.shell == Shell::PowerShell {
        // UTF-8 both ways, no progress bars, and none of PowerShell's own colour codes.
        stdin
            .write_all(b"[Console]::OutputEncoding = [Text.Encoding]::UTF8; [Console]::InputEncoding = [Text.Encoding]::UTF8; $OutputEncoding = [Text.Encoding]::UTF8; $ProgressPreference = 'SilentlyContinue'; if ($PSStyle) { $PSStyle.OutputRendering = 'PlainText' }\n")
            .await?;
    }
    let stdout = child.stdout.take().ok_or_else(|| Error::msg("no shell output"))?;
    let stderr = child.stderr.take().ok_or_else(|| Error::msg("no shell output"))?;
    *term.stdin.lock().await = Some(stdin);
    *term.child.lock().await = Some(child);

    // Standard output: text, and the marker that ends each command.
    {
        let term = term.clone();
        let sink = sink.clone();
        let pid = term.pid.load(Ordering::SeqCst);
        tokio::spawn(async move {
            let mut stdout = stdout;
            let mut chunk = vec![0u8; 8192];
            let mut pending: Vec<u8> = Vec::new();
            let mut text = String::new();
            loop {
                let n = match stdout.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                pending.extend_from_slice(&chunk[..n]);
                // Keep an incomplete UTF-8 sequence for the next read.
                let valid = match std::str::from_utf8(&pending) {
                    Ok(_) => pending.len(),
                    Err(e) => e.valid_up_to(),
                };
                text.push_str(&String::from_utf8_lossy(&pending[..valid]));
                pending.drain(..valid);
                while let Some((before, code, cwd)) = take_marker(&mut text, &term.marker) {
                    if !before.is_empty() {
                        sink(TerminalEvent::Output { id: term.id.clone(), text: strip_ansi(&before) + "\n", error: false });
                    }
                    if !cwd.is_empty() {
                        *term.cwd.lock().unwrap_or_else(|e| e.into_inner()) = PathBuf::from(&cwd);
                    }
                    term.running.store(false, Ordering::SeqCst);
                    sink(TerminalEvent::Done { id: term.id.clone(), code, cwd });
                }
                // Flush text that cannot be the start of a marker.
                let keep = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let tail = text[keep..].to_string();
                let hold = term.marker.starts_with(tail.trim_start()) || tail.contains("__SERIKA");
                let flush_to = if hold { keep } else { text.len() };
                if flush_to > 0 {
                    let out: String = text.drain(..flush_to).collect();
                    sink(TerminalEvent::Output { id: term.id.clone(), text: strip_ansi(&out), error: false });
                }
            }
            // Only report the end if this shell was not replaced by an interrupt.
            if term.pid.load(Ordering::SeqCst) == pid {
                term.running.store(false, Ordering::SeqCst);
                sink(TerminalEvent::Exited { id: term.id.clone() });
            }
        });
    }
    // Standard error.
    {
        let id = term.id.clone();
        tokio::spawn(async move {
            let mut stderr = stderr;
            let mut chunk = vec![0u8; 8192];
            loop {
                let n = match stderr.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let text = strip_ansi(&String::from_utf8_lossy(&chunk[..n]));
                sink(TerminalEvent::Output { id: id.clone(), text, error: true });
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colour_codes_and_carriage_returns() {
        assert_eq!(strip_ansi("\x1b[32mok\x1b[0m\r\n50%\r100%"), "ok\n50%\n100%");
        assert_eq!(strip_ansi("\x1b]0;title\x07done"), "done");
    }

    #[test]
    fn finds_the_done_marker() {
        let mut buf = String::from("hello\n__M__ 0 C:\\work\nrest");
        let (before, code, cwd) = take_marker(&mut buf, "__M__").unwrap();
        assert_eq!((before.as_str(), code, cwd.as_str()), ("hello", 0, "C:\\work"));
        assert_eq!(buf, "rest");
        let mut partial = String::from("x\n__M__ 1");
        assert!(take_marker(&mut partial, "__M__").is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runs_commands_and_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("inner");
        std::fs::create_dir_all(&sub).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: Sink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let host = TerminalHost::default();
        let (id, _) = host.open(dir.path(), sink.clone()).await.unwrap();
        let wait_done = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<TerminalEvent>| {
            let mut out = String::new();
            loop {
                match rx.blocking_recv() {
                    Some(TerminalEvent::Output { text, .. }) => out.push_str(&text),
                    Some(TerminalEvent::Done { code, cwd, .. }) => return (out, code, cwd),
                    Some(TerminalEvent::Exited { .. }) | None => return (out, -1, String::new()),
                }
            }
        };
        host.run(&id, "cd inner").await.unwrap();
        let (_, code, cwd) = tokio::task::block_in_place(|| wait_done(&mut rx));
        assert_eq!(code, 0);
        assert!(cwd.to_ascii_lowercase().ends_with("inner"), "{cwd}");
        host.run(&id, "echo serika-term-test").await.unwrap();
        let (out, code, _) = tokio::task::block_in_place(|| wait_done(&mut rx));
        assert_eq!(code, 0);
        assert!(out.contains("serika-term-test"), "{out:?}");
        host.close(&id).await;
    }
}
