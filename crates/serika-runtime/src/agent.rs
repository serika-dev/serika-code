use std::path::PathBuf;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::ChatTurn;
use crate::context::{self, trim_history};
use crate::credentials::StoredSession;
use crate::error::{Error, Result};
use crate::events::{emit, AgentEvent, EventSender};
use crate::permissions::PermissionMode;
use crate::protocol::{self, extract_reasoning_and_tool_calls};
use crate::tools::{self, ToolContext, ToolOutcome};
use crate::Runtime;

pub struct TaskRequest {
    pub project: PathBuf,
    pub prompt: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub permission: Option<PermissionMode>,
    pub yes: bool,
    pub events: EventSender,
    pub cancel: CancellationToken,
    /// Files and images staged by `attachments::stage`.
    pub attachments: Vec<crate::attachments::Attachment>,
}

pub struct TaskOutcome {
    pub session_id: String,
    pub summary: String,
    pub finished: bool,
}

impl Runtime {
    pub async fn run_task(&self, request: TaskRequest) -> Result<TaskOutcome> {
        if request.prompt.trim().is_empty() && request.attachments.is_empty() {
            return Err(Error::msg("the task is empty"));
        }
        let project = dunce::canonicalize(&request.project).map_err(|_| {
            Error::msg(format!(
                "project does not exist: {}",
                request.project.display()
            ))
        })?;
        if !project.is_dir() {
            return Err(Error::msg("the project path is not a directory"));
        }
        let project_settings = crate::config::ProjectSettings::load(&project)?;
        let permission = request
            .permission
            .or(project_settings.permission)
            .unwrap_or(self.config.permission);
        let mut exclusions = self.config.context_exclusions.clone();
        exclusions.extend(project_settings.extra_exclusions);
        let model = self.resolve_model(request.model.as_deref()).await?;
        // Claude Code and Codex run their own tool loop on the user's subscription login.
        if let Some(agent) = crate::agents::CliAgent::from_model(&model) {
            return self
                .run_cli_agent(agent, &request, &project, permission, &model)
                .await;
        }
        let session_id = self.open_session(&project, &request, &model, permission)?;
        emit(
            &request.events,
            AgentEvent::Started {
                session_id: session_id.clone(),
            },
        );
        emit(
            &request.events,
            AgentEvent::Status(format!("model {model} · {permission}")),
        );
        let mut attachments = request.attachments.clone();
        crate::attachments::load_text(&mut attachments);
        self.prepare_attachments(&mut attachments, &model, &request.events)
            .await?;
        let turn_id = self.sessions.append_user_message(
            &session_id,
            request.prompt.trim(),
            crate::attachments::to_json(&attachments).as_deref(),
        )?;
        if let Ok(stored) = self.sessions.messages(&session_id) {
            if let Some(from) = crate::handoff::previous_model(&stored, turn_id).filter(|m| *m != model) {
                let messages = stored.iter().filter(|m| m.id < turn_id).count();
                emit(&request.events, AgentEvent::Handoff { from, to: model.clone(), messages });
            }
        }

        // Top-level chats may delegate to sub-agents; sub-agents may not delegate further.
        let is_child = self.sessions.parent(&session_id).ok().flatten().is_some();
        // Skills and MCP tools available in this project.
        let skills = crate::skills::discover(&project, &self.paths.data_dir);
        let servers = crate::mcp_hub::load_servers(&self.paths.data_dir, &project);
        let mcp_tools = if servers.iter().any(|s| s.enabled) {
            emit(&request.events, AgentEvent::Status("Connecting MCP servers".into()));
            self.mcp.ensure(&servers, &project).await
        } else {
            Vec::new()
        };
        let extras = [
            crate::project::instructions_prompt(&project),
            crate::skills::instructions(&skills),
            crate::mcp_hub::instructions(&mcp_tools),
        ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let mut changed = Vec::new();
        let files = context::snapshot(&project, &exclusions).unwrap_or_default();
        let max_rounds = self.config.max_tool_rounds;
        let mut accumulated_reasoning = String::new();
        let mut retried_empty = false;

        for round in 1..=max_rounds {
            if request.cancel.is_cancelled() {
                emit(&request.events, AgentEvent::Cancelled);
                return Err(Error::Cancelled);
            }
            emit(&request.events, AgentEvent::Status(format!("turn {round}")));
            let stored = self.sessions.messages(&session_id)?;
            let brief = clip_chars(&context::brief(&project, &files, &changed), 12_000);
            let instructions = format!(
                "{}{}{}\n\nPermission mode: {permission}.\n{brief}",
                protocol::tool_instructions(),
                if is_child { String::new() } else { format!("\n\n{}", crate::subagents::instructions()) },
                if extras.is_empty() { String::new() } else { format!("\n\n{extras}") }
            );
            let (history, message, message_images) =
                pack_history(&stored, &instructions, self.config.max_context_chars);
            let (reply, streamed_reasoning) = self
                .complete(
                    &project,
                    &model,
                    &message,
                    &history,
                    &message_images,
                    &request.cancel,
                    &request.events,
                )
                .await?;
            let (visible, inline_reasoning, calls) = extract_reasoning_and_tool_calls(&reply);
            // A model that answers with nothing at all gets one more try before the turn ends.
            if reply.trim().is_empty() && calls.is_empty() && !retried_empty {
                retried_empty = true;
                emit(&request.events, AgentEvent::Status("Empty reply, asking again".into()));
                continue;
            }

            let mut round_reasoning = streamed_reasoning.trim().to_string();
            if !inline_reasoning.is_empty() && !round_reasoning.contains(&inline_reasoning) {
                if !round_reasoning.is_empty() {
                    round_reasoning.push_str("\n\n");
                }
                round_reasoning.push_str(&inline_reasoning);
            }

            if round_reasoning.is_empty() {
                if !calls.is_empty() {
                    let tool_summary = calls
                        .iter()
                        .map(|c| {
                            if let Some(p) = c.arguments.get("path").and_then(|v| v.as_str()) {
                                format!("{}({p})", c.name)
                            } else if let Some(q) =
                                c.arguments.get("query").and_then(|v| v.as_str())
                            {
                                format!("{}(\"{q}\")", c.name)
                            } else if let Some(pat) =
                                c.arguments.get("pattern").and_then(|v| v.as_str())
                            {
                                format!("{}(\"{pat}\")", c.name)
                            } else if let Some(cmd) =
                                c.arguments.get("command").and_then(|v| v.as_str())
                            {
                                format!("{}(\"{cmd}\")", c.name)
                            } else {
                                c.name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    round_reasoning =
                        format!("Step {round}: Planning workspace inspection via {tool_summary}.");
                    emit(
                        &request.events,
                        AgentEvent::ReasoningChunk {
                            text: format!("{round_reasoning}\n"),
                        },
                    );
                } else if accumulated_reasoning.is_empty() {
                    let proj_name = project
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("workspace");
                    round_reasoning = format!(
                        "Analyzed workspace `{proj_name}` ({} indexed files) · Formulated response with `{model}`.",
                        files.len()
                    );
                    emit(
                        &request.events,
                        AgentEvent::ReasoningChunk {
                            text: round_reasoning.clone(),
                        },
                    );
                }
            }

            if !round_reasoning.is_empty() {
                if !accumulated_reasoning.is_empty() {
                    accumulated_reasoning.push_str("\n\n");
                }
                accumulated_reasoning.push_str(&round_reasoning);
            }

            if calls.is_empty() {
                let stored_text = if visible.is_empty() { &reply } else { &visible };
                self.sessions.append_message_with_reasoning(
                    &session_id,
                    "assistant",
                    stored_text,
                    Some(&accumulated_reasoning),
                )?;
                if !visible.is_empty() {
                    emit(
                        &request.events,
                        AgentEvent::MessageCompleted {
                            text: visible.clone(),
                        },
                    );
                }
                self.refresh_usage(&request.events).await;
                emit(
                    &request.events,
                    AgentEvent::Completed {
                        summary: visible.clone(),
                    },
                );
                return Ok(TaskOutcome {
                    session_id,
                    summary: visible,
                    finished: true,
                });
            }

            self.sessions.append_message_with_reasoning(
                &session_id,
                "assistant",
                &reply,
                Some(&round_reasoning),
            )?;
            if !visible.is_empty() {
                emit(
                    &request.events,
                    AgentEvent::MessageCompleted {
                        text: visible.clone(),
                    },
                );
            }
            let ctx = ToolContext {
                root: &project,
                exclusions: &exclusions,
                max_read_bytes: self.config.max_read_bytes,
                timeout: std::time::Duration::from_secs(self.config.command_timeout_secs),
                mode: permission,
                yes: request.yes,
                events: &request.events,
                cancel: &request.cancel,
                approvals: &self.approvals,
                browser: &self.browser,
                browser_enabled: self.config.browser_enabled,
                skills: &skills,
                mcp: &self.mcp,
            };
            let mut packed = String::from("Tool results:\n");
            // Sub-agents of this reply start together, then their reports join the results in order.
            let delegated: Vec<protocol::ToolCall> = calls.iter().filter(|c| c.name == "delegate_task").cloned().collect();
            let mut reports = if delegated.is_empty() || is_child {
                Vec::new()
            } else {
                let refs: Vec<&protocol::ToolCall> = delegated.iter().collect();
                self.run_subagents(&session_id, &project, &refs, &request, permission, &model).await
            };
            reports.reverse();
            for call in calls {
                if request.cancel.is_cancelled() {
                    emit(&request.events, AgentEvent::Cancelled);
                    return Err(Error::Cancelled);
                }
                for path in checkpoint_paths(&call) {
                    let Ok(full) = crate::workspace::resolve_inside(&project, &path) else {
                        continue;
                    };
                    let (existed, bytes) = if full.is_file() {
                        (true, std::fs::read(&full).unwrap_or_default())
                    } else {
                        (false, Vec::new())
                    };
                    let _ =
                        self.sessions
                            .save_snapshot(&session_id, turn_id, &path, existed, &bytes);
                }
                let outcome = if call.name == "delegate_task" {
                    if is_child {
                        ToolOutcome {
                            output: "sub-agents cannot start more sub-agents; do this part yourself".into(),
                            ok: false,
                            diff: None,
                        }
                    } else {
                        reports.pop().unwrap_or(ToolOutcome { output: "the sub-agent did not run".into(), ok: false, diff: None })
                    }
                } else {
                    tools::execute(&call, &ctx).await
                };
                if outcome.output == "cancelled" {
                    emit(&request.events, AgentEvent::Cancelled);
                    return Err(Error::Cancelled);
                }
                let args = call.arguments.to_string();
                self.sessions.record_tool(
                    &Uuid::new_v4().simple().to_string(),
                    &session_id,
                    &call.name,
                    &args,
                    &clip_chars(&outcome.output, 20_000),
                    outcome.ok,
                )?;
                if let Some(diff) = &outcome.diff {
                    self.sessions
                        .record_change(&session_id, &diff.path, &diff.diff)?;
                    if !changed.contains(&diff.path) {
                        changed.push(diff.path.clone());
                    }
                }
                packed.push_str(&format_result(&call.name, &outcome));
            }
            self.sessions.append_message(&session_id, "user", &packed)?;
        }

        let summary = format!(
            "Stopped after {max_rounds} tool rounds with the task unfinished. The last tool results are saved in the session."
        );
        emit(
            &request.events,
            AgentEvent::Failed {
                message: summary.clone(),
            },
        );
        Ok(TaskOutcome {
            session_id,
            summary,
            finished: false,
        })
    }

    pub(crate) fn open_session(
        &self,
        project: &std::path::Path,
        request: &TaskRequest,
        model: &str,
        permission: PermissionMode,
    ) -> Result<String> {
        if let Some(id) = &request.session_id {
            let existing = self
                .sessions
                .get(id)?
                .ok_or_else(|| Error::msg("session not found"))?;
            let current = project.to_string_lossy();
            if !same_path(&existing.project_path, &current) {
                return Err(Error::msg(
                    "that session belongs to a different project. Open that project or start a new session.",
                ));
            }
            self.sessions.touch(id)?;
            if existing.model_id != model {
                self.sessions.set_model(id, model)?;
            }
            return Ok(id.clone());
        }
        let id = Uuid::new_v4().simple().to_string();
        let title = title_from(&request.prompt);
        self.sessions
            .create(&id, project, &title, model, permission)?;
        Ok(id)
    }

    async fn resolve_model(&self, requested: Option<&str>) -> Result<String> {
        if let Some(model) = requested.filter(|model| !model.is_empty()) {
            return Ok(model.to_string());
        }
        if let Some(model) = self.config.model.clone().filter(|model| !model.is_empty()) {
            return Ok(model);
        }
        // If no Serika session is signed in and we are on the default API URL, query live provider endpoints or use Local Workspace!
        let has_serika_session = self.secrets.load().ok().flatten().is_some();
        if !has_serika_session && self.config.api_base_url == crate::config::DEFAULT_API_BASE {
            let report =
                crate::providers::discover_all_models(self.client.http(), &self.config).await;
            if let Some(first_api) = report
                .models
                .iter()
                .find(|m| m.provider.as_deref() != Some("local"))
            {
                return Ok(first_api.id.clone());
            }
            if let Some(first_local_llm) = report
                .models
                .iter()
                .find(|m| m.id.starts_with("local/ollama:") || m.id.starts_with("local/server:"))
            {
                return Ok(first_local_llm.id.clone());
            }
            return Ok("local/workspace".to_string());
        }
        match self.client.models().await {
            Ok(catalog) => {
                if catalog.find("openrouter/free").is_some() {
                    return Ok("openrouter/free".to_string());
                }
                catalog
                    .default_coding_model()
                    .ok_or_else(|| Error::msg("Serika returned no chat models"))
            }
            Err(_) if !has_serika_session => Ok("local/workspace".to_string()),
            Err(err) => Err(err),
        }
    }

    async fn complete(
        &self,
        project: &std::path::Path,
        model: &str,
        message: &str,
        history: &[ChatTurn],
        message_images: &[String],
        cancel: &CancellationToken,
        events: &EventSender,
    ) -> Result<(String, String)> {
        use crate::providers::{
            complete_anthropic, complete_gemini, complete_local, complete_openai, ProviderKind,
        };

        let mut reasoning_buf = String::new();

        // A signed-in session sends every non-local model through Serika Cloud,
        // including catalog ids such as `anthropic/…` and `openai/…`. Direct
        // provider SDKs stay available when nobody is signed in.
        let kind = ProviderKind::from_model_id(model);
        let prefer_serika_cloud =
            !matches!(kind, ProviderKind::Local) && self.secrets.load().ok().flatten().is_some();

        if prefer_serika_cloud {
            // Fall through to `SerikaClient::chat` below.
        } else {
            match kind {
                ProviderKind::Local => {
                    let reply = complete_local(
                        self.client.http(),
                        &self.config,
                        model,
                        project,
                        message,
                        history,
                        cancel,
                        |chunk| {
                            emit(
                                events,
                                AgentEvent::MessageChunk {
                                    text: chunk.to_string(),
                                },
                            )
                        },
                        |chunk| {
                            reasoning_buf.push_str(chunk);
                            emit(
                                events,
                                AgentEvent::ReasoningChunk {
                                    text: chunk.to_string(),
                                },
                            );
                        },
                    )
                    .await?;
                    return Ok((reply, reasoning_buf));
                }
                ProviderKind::OpenAi => {
                    let reply = complete_openai(
                        self.client.http(),
                        &self.config,
                        model,
                        message,
                        history,
                        cancel,
                        |chunk| {
                            emit(
                                events,
                                AgentEvent::MessageChunk {
                                    text: chunk.to_string(),
                                },
                            )
                        },
                        |chunk| {
                            reasoning_buf.push_str(chunk);
                            emit(
                                events,
                                AgentEvent::ReasoningChunk {
                                    text: chunk.to_string(),
                                },
                            );
                        },
                    )
                    .await?;
                    return Ok((reply, reasoning_buf));
                }
                ProviderKind::Anthropic => {
                    let reply = complete_anthropic(
                        self.client.http(),
                        &self.config,
                        model,
                        message,
                        history,
                        cancel,
                        |chunk| {
                            emit(
                                events,
                                AgentEvent::MessageChunk {
                                    text: chunk.to_string(),
                                },
                            )
                        },
                        |chunk| {
                            reasoning_buf.push_str(chunk);
                            emit(
                                events,
                                AgentEvent::ReasoningChunk {
                                    text: chunk.to_string(),
                                },
                            );
                        },
                    )
                    .await?;
                    return Ok((reply, reasoning_buf));
                }
                ProviderKind::Gemini => {
                    let reply = complete_gemini(
                        self.client.http(),
                        &self.config,
                        model,
                        message,
                        history,
                        cancel,
                        |chunk| {
                            emit(
                                events,
                                AgentEvent::MessageChunk {
                                    text: chunk.to_string(),
                                },
                            )
                        },
                        |chunk| {
                            reasoning_buf.push_str(chunk);
                            emit(
                                events,
                                AgentEvent::ReasoningChunk {
                                    text: chunk.to_string(),
                                },
                            );
                        },
                    )
                    .await?;
                    return Ok((reply, reasoning_buf));
                }
                ProviderKind::Serika => {}
            }
        }

        let Some(mut session) = self.secrets.load()? else {
            // If pointing at default API and not signed in, fall back to API-free local workspace agent
            if self.config.api_base_url == crate::config::DEFAULT_API_BASE {
                let reply = complete_local(
                    self.client.http(),
                    &self.config,
                    "local/workspace",
                    project,
                    message,
                    history,
                    cancel,
                    |chunk| {
                        emit(
                            events,
                            AgentEvent::MessageChunk {
                                text: chunk.to_string(),
                            },
                        )
                    },
                    |chunk| {
                        reasoning_buf.push_str(chunk);
                        emit(
                            events,
                            AgentEvent::ReasoningChunk {
                                text: chunk.to_string(),
                            },
                        );
                    },
                )
                .await?;
                return Ok((reply, reasoning_buf));
            }
            return Err(Error::AuthRequired);
        };
        let mut refreshed = false;
        loop {
            reasoning_buf.clear();
            let result = self
                .client
                .chat(
                    &session.token,
                    model,
                    message,
                    history,
                    message_images,
                    cancel,
                    |chunk| {
                        emit(
                            events,
                            AgentEvent::MessageChunk {
                                text: chunk.to_string(),
                            },
                        )
                    },
                    |chunk| {
                        reasoning_buf.push_str(chunk);
                        emit(
                            events,
                            AgentEvent::ReasoningChunk {
                                text: chunk.to_string(),
                            },
                        );
                    },
                )
                .await;
            match result {
                Err(Error::Auth(_)) if !refreshed => {
                    refreshed = true;
                    session = refresh_session(&self.client, session).await?;
                    self.secrets.save(&session)?;
                }
                Ok(reply) => return Ok((reply, reasoning_buf)),
                Err(err) => return Err(err),
            }
        }
    }

    /// Check what a Serika model can take and upload the images. Text files travel inline.
    pub(crate) async fn prepare_attachments(
        &self,
        attachments: &mut [crate::attachments::Attachment],
        model: &str,
        events: &EventSender,
    ) -> Result<()> {
        use crate::attachments::AttachmentKind;
        if attachments.is_empty() {
            return Ok(());
        }
        if let Some(other) = attachments.iter().find(|a| a.kind == AttachmentKind::File) {
            return Err(Error::msg(format!(
                "{} cannot be sent to Serika models. Images and text or code files work; pick Claude Code or Codex to use other files.",
                other.name
            )));
        }
        let images: Vec<usize> = attachments
            .iter()
            .enumerate()
            .filter(|(_, a)| a.kind == AttachmentKind::Image && a.url.is_none())
            .map(|(i, _)| i)
            .collect();
        if images.is_empty() {
            return Ok(());
        }
        let Some(mut session) = self.secrets.load().ok().flatten() else {
            return Err(Error::msg("Sign in to send images to Serika models."));
        };
        let _ = model;
        for (n, index) in images.iter().enumerate() {
            emit(
                events,
                AgentEvent::Status(format!("uploading image {} of {}", n + 1, images.len())),
            );
            let url = match self.client.upload_image(&session.token, &attachments[*index]).await {
                Err(Error::Auth(_)) => {
                    session = refresh_session(&self.client, session).await?;
                    self.secrets.save(&session)?;
                    self.client.upload_image(&session.token, &attachments[*index]).await?
                }
                other => other?,
            };
            attachments[*index].url = Some(url);
        }
        Ok(())
    }

    async fn refresh_usage(&self, events: &EventSender) {
        let Ok(Some(session)) = self.secrets.load() else {
            return;
        };
        if let Ok(usage) = self.client.usage(&session.token).await {
            emit(events, AgentEvent::UsageUpdated(usage));
        }
    }
}

async fn refresh_session(
    client: &crate::client::SerikaClient,
    mut session: StoredSession,
) -> Result<StoredSession> {
    match client.refresh(&session.refresh_token).await {
        Ok((token, refresh)) => {
            session.token = token;
            session.refresh_token = refresh;
            Ok(session)
        }
        Err(_) => Err(Error::Auth("the session expired. Sign in again.".into())),
    }
}

/// The text a model sees for one stored message: text files are inlined, images travel as URLs.
fn message_parts(message: &crate::sessions::StoredMessage) -> (String, Vec<String>) {
    let attachments = crate::attachments::parse_json(message.attachments.as_deref());
    if attachments.is_empty() {
        return (message.content.clone(), Vec::new());
    }
    let images = attachments
        .iter()
        .filter(|a| a.kind == crate::attachments::AttachmentKind::Image)
        .filter_map(|a| a.url.clone())
        .collect();
    (crate::attachments::with_files(&message.content, &attachments), images)
}

fn pack_history(
    messages: &[crate::sessions::StoredMessage],
    instructions: &str,
    max_chars: usize,
) -> (Vec<ChatTurn>, String, Vec<String>) {
    let (current, current_images) = messages
        .last()
        .map(message_parts)
        .unwrap_or_default();
    let mut turns = vec![ChatTurn {
        role: "system".into(),
        content: instructions.to_string(),
        attachments: Vec::new(),
    }];
    let prior = messages.len().saturating_sub(1);
    for message in &messages[..prior] {
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        let (content, images) = message_parts(message);
        turns.push(ChatTurn {
            role: role.to_string(),
            content,
            attachments: images,
        });
    }
    (trim_history(turns, max_chars), current, current_images)
}

fn format_result(name: &str, outcome: &ToolOutcome) -> String {
    format!(
        "[{name}] {}\n{}\n\n",
        if outcome.ok { "ok" } else { "failed" },
        clip_chars(&outcome.output, 12_000)
    )
}

fn checkpoint_paths(call: &crate::protocol::ToolCall) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["path", "from", "to"] {
        if let Some(path) = call.arguments.get(key).and_then(|value| value.as_str()) {
            if !path.is_empty() {
                paths.push(path.to_string());
            }
        }
    }
    if call.name == "apply_patch" {
        if let Some(patch) = call.arguments.get("patch").and_then(|value| value.as_str()) {
            for line in patch.lines() {
                if let Some(rest) = line.strip_prefix("+++ b/") {
                    let path = rest.trim();
                    if !path.is_empty() && path != "/dev/null" {
                        paths.push(path.to_string());
                    }
                }
            }
        }
    }
    paths
}

fn title_from(prompt: &str) -> String {
    let line = prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("New session");
    let mut title = line.trim().chars().take(80).collect::<String>();
    if line.trim().chars().count() > 80 {
        title.push('…');
    }
    title
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.replace('/', "\\")
            .eq_ignore_ascii_case(&b.replace('/', "\\"))
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{MemoryStore, SecretStore, StoredSession};
    use crate::paths::AppPaths;
    use crate::RuntimeOptions;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn edits_a_file_reviews_the_diff_and_resumes() {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let (base, _task) = crate::testsupport::spawn(move |request| {
            if request.path.ends_with("/api/chat/usage") {
                return crate::testsupport::Response {
                    status: 200,
                    content_type: "application/json",
                    body: r#"{"totalMessages":1,"dailyMessages":1,"plan":{"tier":{"key":"free","name":"Free"},"weekly":{"limit":10000,"used":1,"remaining":9999}}}"#.into(),
                };
            }
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let content = match n {
                0 => "```serika-tool\n{\"name\":\"read_file\",\"arguments\":{\"path\":\"hello.txt\"}}\n```",
                1 => "```serika-tool\n{\"name\":\"replace_in_file\",\"arguments\":{\"path\":\"hello.txt\",\"search\":\"hello\",\"replace\":\"hello world\"}}\n```",
                _ => "Updated hello.txt. The file now contains hello world.",
            };
            crate::testsupport::Response {
                status: 200,
                content_type: "text/event-stream",
                body: format!("data: {{\"content\":{}}}\n\ndata: [DONE]\n\n", serde_json::to_string(content).unwrap()),
            }
        })
        .await;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello\n").unwrap();
        let paths = AppPaths::under(dir.path().join("home")).unwrap();
        let mut config = crate::config::Config::default();
        config.api_base_url = base.clone();
        config.accounts_base_url = base;
        config.model = Some("deepseek/deepseek-v4.1-flash".into());
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .save(&StoredSession {
                token: "test-token".into(),
                refresh_token: "test-refresh".into(),
                user_id: "u".into(),
                username: "noa".into(),
                email: "noa@example.com".into(),
                is_premium: false,
            })
            .unwrap();
        let runtime = Runtime::open_with(RuntimeOptions {
            paths,
            config,
            secrets,
        })
        .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = runtime
            .run_task(TaskRequest {
                project: dir.path().to_path_buf(),
                prompt: "Change hello.txt to say hello world".into(),
                session_id: None,
                model: None,
                permission: Some(PermissionMode::Full),
                yes: true,
                events: tx,
                cancel: CancellationToken::new(),
                attachments: Vec::new(),
            })
            .await
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        assert_eq!(body, "hello world\n");
        assert!(outcome.finished);
        assert!(outcome.summary.contains("hello world"));
        let changes = runtime.sessions.changes(&outcome.session_id).unwrap();
        assert!(!changes.is_empty());
        assert!(changes[0].1.contains("+hello world") || changes[0].1.contains("hello world"));
        let messages = runtime.sessions.messages(&outcome.session_id).unwrap();
        assert!(messages.len() >= 4);

        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        let _ = rx.recv().await;
        let again = runtime
            .run_task(TaskRequest {
                project: dunce::canonicalize(dir.path()).unwrap(),
                prompt: "What does the file say now?".into(),
                session_id: Some(outcome.session_id.clone()),
                model: Some("deepseek/deepseek-v4.1-flash".into()),
                permission: Some(PermissionMode::ReadOnly),
                yes: true,
                events: tx2,
                cancel: CancellationToken::new(),
                attachments: Vec::new(),
            })
            .await
            .unwrap();
        assert_eq!(again.session_id, outcome.session_id);
        assert!(hits.load(Ordering::SeqCst) >= 4);
    }

    #[tokio::test]
    async fn refreshes_token_on_401_and_surfaces_usage_limit_429() {
        let step = Arc::new(AtomicUsize::new(0));
        let counter = step.clone();
        let (base, _task) = crate::testsupport::spawn(move |request| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if request.path.ends_with("/api/auth/refresh") {
                return crate::testsupport::Response {
                    status: 200,
                    content_type: "application/json",
                    body: r#"{"token":"fresh-token","refreshToken":"fresh-refresh"}"#.into(),
                };
            }
            if n == 0 {
                return crate::testsupport::Response {
                    status: 401,
                    content_type: "application/json",
                    body: r#"{"error":"expired"}"#.into(),
                };
            }
            crate::testsupport::Response {
                status: 429,
                content_type: "application/json",
                body: r#"{"error":"You've used this week's free allowance.","code":"WEEKLY_LIMIT_REACHED","limitReached":true,"limit":10000,"used":10000,"resetsAt":"2026-11-06T00:00:00.000Z"}"#.into(),
            }
        })
        .await;

        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path().join("home")).unwrap();
        let mut config = crate::config::Config::default();
        config.api_base_url = base.clone();
        config.accounts_base_url = base;
        config.model = Some("deepseek/deepseek-v4.1-flash".into());
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .save(&StoredSession {
                token: "stale-token".into(),
                refresh_token: "old-refresh".into(),
                user_id: "u".into(),
                username: "noa".into(),
                email: "noa@example.com".into(),
                is_premium: false,
            })
            .unwrap();
        let runtime = Runtime::open_with(RuntimeOptions {
            paths,
            config,
            secrets: secrets.clone(),
        })
        .unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let err = runtime
            .run_task(TaskRequest {
                project: dir.path().to_path_buf(),
                prompt: "Run a task".into(),
                session_id: None,
                model: None,
                permission: Some(PermissionMode::ReadOnly),
                yes: true,
                events: tx,
                cancel: CancellationToken::new(),
                attachments: Vec::new(),
            })
            .await
            .err()
            .unwrap();
        assert!(matches!(err, Error::UsageLimit(_)));
        assert_eq!(err.exit_code(), 2);
        let saved = secrets.load().unwrap().unwrap();
        assert_eq!(saved.token, "fresh-token");
        assert_eq!(saved.refresh_token, "fresh-refresh");
    }
}
