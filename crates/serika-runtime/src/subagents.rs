//! Sub-agents: a chat can hand self-contained tasks to child agents.
//!
//! The model calls `delegate_task` (several times in one reply to run them side by side).
//! Each task gets its own chat, linked to the parent and kept out of the chat list, and runs
//! with the same tools in the same project, on the parent's model or another one (a Serika
//! model, Claude Code, or Codex). Progress streams to the parent as `Subagent` events; a
//! child's approval requests appear in the parent; stopping the parent stops its children.

use std::path::Path;

use serde_json::Value;
use uuid::Uuid;

use crate::agent::TaskRequest;
use crate::error::Error;
use crate::events::{emit, AgentEvent};
use crate::permissions::PermissionMode;
use crate::protocol::ToolCall;
use crate::tools::ToolOutcome;
use crate::Runtime;

/// How many sub-agents one reply may start.
pub const MAX_PARALLEL: usize = 6;

pub fn instructions() -> &'static str {
    r#"Sub-agents (you can delegate):
- delegate_task {"task":"what to do, with every detail it needs","name":"short label"}
  A sub-agent works in this project with the same tools and reports back with a summary. It sees only the task text, not this conversation, so make the task self-contained.
  Call delegate_task several times in one reply to run independent pieces in parallel (research, reviews, separate files). Do small or tightly coupled work yourself.
  It uses your model. Only if the user asks for another one, add "model" with a real id, for example claude-code/sonnet to use their Claude Code subscription."#
}

struct Spec {
    task: String,
    name: String,
    model: Option<String>,
}

fn spec(call: &ToolCall) -> Result<Spec, String> {
    let s = |k: &str| call.arguments.get(k).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    let task = s("task");
    if task.is_empty() {
        return Err("delegate_task needs a \"task\"".into());
    }
    let mut name = s("name");
    if name.is_empty() {
        name = task.lines().next().unwrap_or("Sub-agent").chars().take(48).collect();
    }
    // Models sometimes fill in a placeholder; only a provider/model id counts.
    let model = Some(s("model")).filter(|m| {
        let lower = m.to_ascii_lowercase();
        m.contains('/') && !m.contains(' ') && !lower.contains("model-id") && !lower.contains("optional")
    });
    Ok(Spec { task, name, model })
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

impl Runtime {
    /// Run every `delegate_task` call of one reply at the same time, in call order.
    pub(crate) async fn run_subagents(
        &self,
        parent: &str,
        project: &Path,
        calls: &[&ToolCall],
        request: &TaskRequest,
        permission: PermissionMode,
        parent_model: &str,
    ) -> Vec<ToolOutcome> {
        let runs = calls.iter().enumerate().map(|(i, call)| async move {
            if i >= MAX_PARALLEL {
                return ToolOutcome {
                    output: format!("only {MAX_PARALLEL} sub-agents can run at once; do this one after the others finish"),
                    ok: false,
                    diff: None,
                };
            }
            self.run_subagent(parent, project, call, request, permission, parent_model).await
        });
        futures_util::future::join_all(runs).await
    }

    async fn run_subagent(
        &self,
        parent: &str,
        project: &Path,
        call: &ToolCall,
        request: &TaskRequest,
        permission: PermissionMode,
        parent_model: &str,
    ) -> ToolOutcome {
        let spec = match spec(call) {
            Ok(spec) => spec,
            Err(message) => return ToolOutcome { output: message, ok: false, diff: None },
        };
        let model = spec.model.clone().unwrap_or_else(|| parent_model.to_string());
        let id = Uuid::new_v4().simple().to_string();
        let tool_id = Uuid::new_v4().simple().to_string();
        if let Err(err) = self
            .sessions
            .create(&id, project, &spec.name, &model, permission)
            .and_then(|_| self.sessions.set_parent(&id, parent))
        {
            return ToolOutcome { output: format!("could not start the sub-agent: {err}"), ok: false, diff: None };
        }

        emit(&request.events, AgentEvent::ToolRequested {
            id: tool_id.clone(),
            name: "delegate_task".into(),
            arguments: call.arguments.to_string(),
        });
        emit(&request.events, AgentEvent::ToolStarted { id: tool_id.clone(), name: "delegate_task".into() });
        let update = |status: &str, activity: &str, tools: usize, summary: &str| {
            emit(&request.events, AgentEvent::Subagent {
                id: id.clone(),
                name: spec.name.clone(),
                task: spec.task.clone(),
                model: model.clone(),
                status: status.into(),
                activity: activity.into(),
                tools,
                summary: summary.into(),
            });
        };
        update("running", "Starting", 0, "");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let child = TaskRequest {
            project: project.to_path_buf(),
            prompt: format!(
                "{}\n\nYou are a sub-agent working on one task for another agent. Do the task, then reply with a concise report of what you found or changed (files, commands, results).",
                spec.task
            ),
            session_id: Some(id.clone()),
            model: Some(model.clone()),
            permission: Some(permission),
            yes: request.yes,
            events: tx,
            cancel: request.cancel.child_token(),
            attachments: Vec::new(),
        };

        // Forward the child's progress while it runs.
        let forward = async {
            let mut tools = 0usize;
            let mut changed: Vec<String> = Vec::new();
            while let Some(event) = rx.recv().await {
                match event {
                    AgentEvent::ToolRequested { name, arguments, .. } => {
                        tools += 1;
                        let detail = serde_json::from_str::<Value>(&arguments)
                            .ok()
                            .and_then(|v| {
                                ["path", "file_path", "command", "query", "pattern", "url"]
                                    .iter()
                                    .find_map(|k| v.get(*k).and_then(Value::as_str).map(str::to_string))
                            })
                            .unwrap_or_default();
                        let activity = if detail.is_empty() { name } else { format!("{name} {}", clip(&detail, 80)) };
                        update("running", &activity, tools, "");
                    }
                    AgentEvent::ApprovalRequired { id: approval, title, detail, destructive } => {
                        // The parent asks on the child's behalf; the shared approval hub answers it.
                        emit(&request.events, AgentEvent::ApprovalRequired {
                            id: approval,
                            title: format!("{} (sub-agent “{}”)", title, spec.name),
                            detail,
                            destructive,
                        });
                    }
                    AgentEvent::FileChanged(diff) => {
                        if !changed.contains(&diff.path) {
                            changed.push(diff.path.clone());
                        }
                        emit(&request.events, AgentEvent::FileChanged(diff));
                    }
                    AgentEvent::ReasoningChunk { .. } => update("running", "Thinking", tools, ""),
                    AgentEvent::MessageChunk { .. } => update("running", "Writing its report", tools, ""),
                    _ => {}
                }
            }
            (tools, changed)
        };
        let run = Box::pin(self.run_task(child));
        let (mut outcome, (tools, changed)) = tokio::join!(run, forward);

        // Some models finish with tool calls and no words. Ask once for the report.
        if matches!(&outcome, Ok(done) if done.finished && done.summary.trim().is_empty()) {
            update("running", "Writing its report", tools, "");
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let again = TaskRequest {
                project: project.to_path_buf(),
                prompt: "Write your report now, in plain text: the answer to your task and what you did. No tool calls.".into(),
                session_id: Some(id.clone()),
                model: Some(model.clone()),
                permission: Some(PermissionMode::ReadOnly),
                yes: request.yes,
                events: tx,
                cancel: request.cancel.child_token(),
                attachments: Vec::new(),
            };
            let drain = async { while rx.recv().await.is_some() {} };
            let (second, ()) = tokio::join!(Box::pin(self.run_task(again)), drain);
            if let Ok(done) = second {
                if !done.summary.trim().is_empty() {
                    outcome = Ok(done);
                }
            }
        }
        // Still nothing: hand back what its tools found.
        if let Ok(done) = &mut outcome {
            if done.finished && done.summary.trim().is_empty() {
                let runs = self.sessions.tool_runs(&id).unwrap_or_default();
                let found: Vec<String> = runs
                    .iter()
                    .rev()
                    .take(3)
                    .map(|t| format!("{}:\n{}", t.name, clip(t.output.trim(), 1_500)))
                    .collect();
                done.summary = if found.is_empty() {
                    "(The sub-agent finished without a report or any tool results.)".into()
                } else {
                    format!("(No written report. Its last tool results:)\n\n{}", found.join("\n\n"))
                };
            }
        }

        let (status, output, ok) = match outcome {
            Ok(done) if done.finished => {
                let summary = clip(done.summary.trim(), 8_000);
                let files = if changed.is_empty() { String::new() } else { format!("\nFiles changed: {}", changed.join(", ")) };
                (
                    "done",
                    format!("Sub-agent “{}” ({model}) finished after {tools} tool call(s).{files}\n\n{summary}", spec.name),
                    true,
                )
            }
            Ok(done) => ("failed", format!("Sub-agent “{}” stopped: {}", spec.name, done.summary), false),
            Err(Error::Cancelled) => ("cancelled", format!("Sub-agent “{}” was stopped.", spec.name), false),
            Err(err) => ("failed", format!("Sub-agent “{}” failed: {err}", spec.name), false),
        };
        let summary = if ok { output.split("\n\n").nth(1).unwrap_or("").to_string() } else { output.clone() };
        update(status, if ok { "Finished" } else { status }, tools, &clip(&summary, 600));
        emit(&request.events, AgentEvent::ToolFinished {
            id: tool_id,
            name: "delegate_task".into(),
            ok,
            summary: format!("{} · {status}", spec.name),
        });
        ToolOutcome { output, ok, diff: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(args: &str) -> ToolCall {
        ToolCall { name: "delegate_task".into(), arguments: serde_json::from_str(args).unwrap() }
    }

    #[test]
    fn reads_the_task_and_defaults_the_name() {
        let s = spec(&call(r#"{"task":"Find every TODO in src\nand list them"}"#)).unwrap();
        assert_eq!(s.name, "Find every TODO in src");
        assert!(s.model.is_none());
        let s = spec(&call(r#"{"task":"x","name":"Docs","model":"claude-code/sonnet"}"#)).unwrap();
        assert_eq!((s.name.as_str(), s.model.as_deref()), ("Docs", Some("claude-code/sonnet")));
        assert!(spec(&call(r#"{"task":"x","model":"your-model-id"}"#)).unwrap().model.is_none());
        assert!(spec(&call(r#"{"task":"x","model":"optional model id"}"#)).unwrap().model.is_none());
        assert!(spec(&call(r#"{"task":"  "}"#)).is_err());
    }
}
