use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::diffutil::unified_diff;
use crate::error::Error;
use crate::events::{emit, AgentEvent, ApprovalHub, EventSender, FileDiff};
use crate::gitinfo;
use crate::patch::{apply_unified_diff, replace_exact};
use crate::permissions::{gate, ActionKind, Gate, PermissionMode};
use crate::protocol::ToolCall;
use crate::shell::{classify_command, run_command, CommandClass};
use crate::workspace::{is_binary_sample, is_sensitive, list_files, resolve_inside};

pub struct ToolContext<'a> {
    pub root: &'a Path,
    pub exclusions: &'a [String],
    pub max_read_bytes: usize,
    pub timeout: Duration,
    pub mode: PermissionMode,
    pub yes: bool,
    pub events: &'a EventSender,
    pub cancel: &'a CancellationToken,
    pub approvals: &'a ApprovalHub,
}

pub struct ToolOutcome {
    pub output: String,
    pub ok: bool,
    pub diff: Option<FileDiff>,
}

pub async fn execute(call: &ToolCall, ctx: &ToolContext<'_>) -> ToolOutcome {
    if call.name.is_empty() {
        let detail = call.arguments.as_str().unwrap_or("the tool block was not valid JSON");
        return fail(detail);
    }
    let id = Uuid::new_v4().simple().to_string();
    emit(ctx.events, AgentEvent::ToolRequested {
        id: id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.to_string(),
    });
    let result = dispatch(&call.name, &call.arguments, ctx, &id).await;
    let (output, ok, diff) = match result {
        Ok(value) => value,
        Err(err) => {
            if matches!(err, Error::Cancelled) {
                return ToolOutcome {
                    output: "cancelled".into(),
                    ok: false,
                    diff: None,
                };
            }
            (err.to_string(), false, None)
        }
    };
    emit(ctx.events, AgentEvent::ToolFinished {
        id,
        name: call.name.clone(),
        ok,
        summary: clip(&output, 240),
    });
    if let Some(diff) = &diff {
        emit(ctx.events, AgentEvent::FileChanged(diff.clone()));
    }
    ToolOutcome { output, ok, diff }
}

type Done = (String, bool, Option<FileDiff>);

async fn dispatch(name: &str, args: &Value, ctx: &ToolContext<'_>, id: &str) -> Result<Done, Error> {
    emit(ctx.events, AgentEvent::ToolStarted { id: id.to_string(), name: name.to_string() });
    match name {
        "list_dir" => Ok((list_dir(args, ctx)?, true, None)),
        "read_file" => Ok((read_file(args, ctx).await?, true, None)),
        "search_text" => Ok((search_text(args, ctx)?, true, None)),
        "search_files" => Ok((search_files(args, ctx)?, true, None)),
        "write_file" => write_file(args, ctx).await,
        "replace_in_file" => replace_in_file(args, ctx).await,
        "apply_patch" => apply_patch(args, ctx).await,
        "delete_file" => delete_file(args, ctx).await,
        "move_file" => move_file(args, ctx).await,
        "run_command" => run_shell(args, ctx, id).await,
        "git_status" => Ok((gitinfo::status(ctx.root).await?, true, None)),
        "git_diff" => {
            let path = args.get("path").and_then(|v| v.as_str());
            Ok((gitinfo::diff(ctx.root, path).await?, true, None))
        }
        "git_log" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
            Ok((gitinfo::log(ctx.root, limit).await?, true, None))
        }
        "git_commit" => git_commit(args, ctx).await,
        other => Ok((format!("unknown tool \"{other}\""), false, None)),
    }
}

fn list_dir(args: &Value, ctx: &ToolContext<'_>) -> Result<String, Error> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
    let dir = resolve_inside(ctx.root, path)?;
    let files = list_files(&dir, ctx.exclusions, Some(depth.clamp(1, 6)))?;
    if files.is_empty() {
        return Ok(format!("{path} is empty or fully ignored"));
    }
    let mut out = String::new();
    for file in files.iter().take(400) {
        out.push_str(&file.relative);
        out.push('\n');
    }
    Ok(out)
}

async fn read_file(args: &Value, ctx: &ToolContext<'_>) -> Result<String, Error> {
    #[derive(Deserialize)]
    struct Args {
        path: String,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        limit: Option<usize>,
    }
    let args: Args = parse(args)?;
    let path = resolve_inside(ctx.root, &args.path)?;
    ensure(ctx, ActionKind::Read, &args.path, "read", false).await?;
    if is_sensitive(&path) {
        ensure(ctx, ActionKind::ReadSensitive, &args.path, "read a sensitive file", true).await?;
    }
    let bytes = std::fs::read(&path)?;
    if is_binary_sample(&bytes[..bytes.len().min(8000)]) {
        return Err(Error::msg("refusing to read a binary file"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let clipped = if text.len() > ctx.max_read_bytes {
        let mut cut = ctx.max_read_bytes;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n… truncated at {} bytes", &text[..cut], cut)
    } else {
        text.to_string()
    };
    let offset = args.offset.unwrap_or(1).max(1);
    let limit = args.limit.unwrap_or(400).clamp(1, 2000);
    let mut out = format!(
        "{} ({} bytes). Line numbers are not part of the file.\n",
        args.path,
        bytes.len()
    );
    for (index, line) in clipped.lines().enumerate() {
        let number = index + 1;
        if number < offset {
            continue;
        }
        if number >= offset + limit {
            out.push_str("… more lines\n");
            break;
        }
        out.push_str(&format!("{number}|{line}\n"));
    }
    Ok(out)
}

fn search_text(args: &Value, ctx: &ToolContext<'_>) -> Result<String, Error> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if query.is_empty() {
        return Err(Error::msg("query is required"));
    }
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let regex = args.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
    let max = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(30) as usize;
    let root = resolve_inside(ctx.root, path)?;
    let matcher = if regex {
        Some(regex::Regex::new(&query).map_err(|err| Error::msg(format!("invalid regex: {err}")))?)
    } else {
        None
    };
    let files = list_files(&root, ctx.exclusions, None)?;
    let mut hits = 0usize;
    let mut out = String::new();
    for file in files {
        if hits >= max {
            break;
        }
        if file.bytes > 1_000_000 {
            continue;
        }
        let full = root.join(&file.relative);
        let Ok(bytes) = std::fs::read(&full) else { continue };
        if is_binary_sample(&bytes[..bytes.len().min(8000)]) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            let matched = if let Some(regex) = &matcher {
                regex.is_match(line)
            } else {
                line.contains(&query)
            };
            if matched {
                out.push_str(&format!("{}:{}:{line}\n", file.relative, index + 1));
                hits += 1;
                if hits >= max {
                    break;
                }
            }
        }
    }
    if hits == 0 {
        Ok("no matches".into())
    } else {
        Ok(out)
    }
}

fn search_files(args: &Value, ctx: &ToolContext<'_>) -> Result<String, Error> {
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    if pattern.is_empty() {
        return Err(Error::msg("pattern is required"));
    }
    let files = list_files(ctx.root, ctx.exclusions, None)?;
    let mut out = String::new();
    let mut count = 0;
    for file in files {
        if file_matches(pattern, &file.relative) {
            out.push_str(&file.relative);
            out.push('\n');
            count += 1;
            if count >= 200 {
                break;
            }
        }
    }
    if count == 0 {
        Ok("no files matched".into())
    } else {
        Ok(out)
    }
}

async fn write_file(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let path = arg_str(args, "path")?;
    let content = arg_str(args, "content")?;
    let full = resolve_inside(ctx.root, path)?;
    if content.len() > 1_000_000 {
        return Err(Error::msg("refusing to write more than 1000000 bytes in one call"));
    }
    ensure(ctx, ActionKind::Write, path, "write a file", false).await?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let previous = std::fs::read_to_string(&full).unwrap_or_default();
    std::fs::write(&full, content.as_bytes())?;
    let diff = unified_diff(path, &previous, content);
    Ok((format!("wrote {path}"), true, Some(FileDiff { path: path.to_string(), diff })))
}

async fn replace_in_file(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let path = arg_str(args, "path")?;
    let search = arg_str(args, "search")?;
    let replace = arg_str(args, "replace")?;
    let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
    let full = resolve_inside(ctx.root, path)?;
    ensure(ctx, ActionKind::Write, path, "edit a file", false).await?;
    let previous = std::fs::read_to_string(&full)?;
    let updated = replace_exact(&previous, search, replace, all)?;
    std::fs::write(&full, updated.as_bytes())?;
    let diff = unified_diff(path, &previous, &updated);
    Ok((format!("updated {path}\n{diff}"), true, Some(FileDiff { path: path.to_string(), diff })))
}

async fn apply_patch(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let patch = arg_str(args, "patch")?;
    let explicit = args.get("path").and_then(|v| v.as_str());
    let paths = if let Some(path) = explicit {
        vec![path.to_string()]
    } else {
        patch_paths(patch)
    };
    if paths.len() != 1 {
        return Err(Error::msg(
            "apply_patch edits one file at a time. Include one +++ b/<path> header, or pass path.",
        ));
    }
    let path = &paths[0];
    let full = resolve_inside(ctx.root, path)?;
    ensure(ctx, ActionKind::Write, path, "apply a patch", false).await?;
    let previous = std::fs::read_to_string(&full).unwrap_or_default();
    let updated = apply_unified_diff(&previous, patch)?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&full, updated.as_bytes())?;
    let diff = unified_diff(path, &previous, &updated);
    Ok((format!("patched {path}\n{diff}"), true, Some(FileDiff { path: path.clone(), diff })))
}

async fn delete_file(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let path = arg_str(args, "path")?;
    let full = resolve_inside(ctx.root, path)?;
    if full.is_dir() {
        return Err(Error::msg("refusing to delete a directory"));
    }
    ensure(ctx, ActionKind::Delete, path, "delete a file", true).await?;
    let previous = std::fs::read_to_string(&full).unwrap_or_default();
    std::fs::remove_file(&full)?;
    let diff = unified_diff(path, &previous, "");
    Ok((format!("deleted {path}"), true, Some(FileDiff { path: path.to_string(), diff })))
}

async fn move_file(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let from = arg_str(args, "from")?;
    let to = arg_str(args, "to")?;
    let source = resolve_inside(ctx.root, from)?;
    let dest = resolve_inside(ctx.root, to)?;
    ensure(ctx, ActionKind::Write, to, "move a file", false).await?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&source, &dest)?;
    Ok((format!("moved {from} to {to}"), true, None))
}

async fn run_shell(args: &Value, ctx: &ToolContext<'_>, id: &str) -> Result<Done, Error> {
    let command = arg_str(args, "command")?;
    let timeout = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs)
        .unwrap_or(ctx.timeout);
    let timeout = timeout.clamp(Duration::from_secs(1), Duration::from_secs(3600));
    let class = classify_command(command);
    let action = match class {
        CommandClass::ReadOnly => ActionKind::CommandReadOnly,
        CommandClass::Development => ActionKind::CommandDev,
        CommandClass::Destructive => ActionKind::CommandDestructive,
        CommandClass::Other => ActionKind::Command,
    };
    ensure(ctx, action, command, "run a command", class == CommandClass::Destructive).await?;
    let result = run_command(command, ctx.root, timeout, ctx.cancel, ctx.events, id).await?;
    let ok = result.exit_code == 0;
    let output = format!("exit {}\n{}", result.exit_code, result.output);
    Ok((output, ok, None))
}

async fn git_commit(args: &Value, ctx: &ToolContext<'_>) -> Result<Done, Error> {
    let message = arg_str(args, "message")?;
    let paths: Vec<String> = args
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    for path in &paths {
        let _ = resolve_inside(ctx.root, path)?;
    }
    ensure(ctx, ActionKind::GitCommit, message, "create a git commit", true).await?;
    let output = gitinfo::commit(ctx.root, message, &paths).await?;
    let ok = !output.starts_with("git exited");
    Ok((output, ok, None))
}

async fn ensure(
    ctx: &ToolContext<'_>,
    action: ActionKind,
    target: &str,
    title: &str,
    destructive_hint: bool,
) -> Result<(), Error> {
    match gate(ctx.mode, action) {
        Gate::Allow => Ok(()),
        Gate::Deny => Err(Error::msg(format!(
            "blocked by the {} permission mode",
            ctx.mode
        ))),
        Gate::Prompt { destructive } => {
            let destructive = destructive || destructive_hint;
            if ctx.yes && !destructive {
                return Ok(());
            }
            let id = Uuid::new_v4().simple().to_string();
            emit(ctx.events, AgentEvent::ApprovalRequired {
                id: id.clone(),
                title: title.to_string(),
                detail: target.to_string(),
                destructive,
            });
            let rx = ctx.approvals.request(id);
            let allowed = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
                answer = rx => answer.unwrap_or(false),
            };
            if allowed {
                Ok(())
            } else {
                Err(Error::msg("the user denied this action"))
            }
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, Error> {
    serde_json::from_value(args.clone()).map_err(|err| Error::msg(format!("invalid arguments: {err}")))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::msg(format!("missing string argument \"{key}\"")))
}

fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            let path = rest.trim();
            if !path.is_empty() && path != "/dev/null" && !paths.iter().any(|have: &String| have == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

fn file_matches(pattern: &str, relative: &str) -> bool {
    let target = if pattern.contains('/') {
        relative
    } else {
        relative.rsplit('/').next().unwrap_or(relative)
    };
    if !pattern.contains('*') && !pattern.contains('?') {
        return relative.to_ascii_lowercase().contains(&pattern.to_ascii_lowercase());
    }
    let mut regex = String::from("(?i)^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    regex::Regex::new(&regex).map(|re| re.is_match(target)).unwrap_or(false)
}

fn fail(message: &str) -> ToolOutcome {
    ToolOutcome {
        output: message.to_string(),
        ok: false,
        diff: None,
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}
