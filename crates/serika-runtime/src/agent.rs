use std::path::PathBuf;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::ChatTurn;
use crate::context::{self, trim_history};
use crate::credentials::StoredSession;
use crate::error::{Error, Result};
use crate::events::{emit, AgentEvent, EventSender};
use crate::permissions::PermissionMode;
use crate::protocol::{self, extract_tool_calls};
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
}

pub struct TaskOutcome {
    pub session_id: String,
    pub summary: String,
    pub finished: bool,
}

impl Runtime {
    pub async fn run_task(&self, request: TaskRequest) -> Result<TaskOutcome> {
        if request.prompt.trim().is_empty() {
            return Err(Error::msg("the task is empty"));
        }
        let project = dunce::canonicalize(&request.project).map_err(|_| {
            Error::msg(format!("project does not exist: {}", request.project.display()))
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
        let session_id = self.open_session(&project, &request, &model, permission)?;
        emit(&request.events, AgentEvent::Started { session_id: session_id.clone() });
        emit(&request.events, AgentEvent::Status(format!("model {model} · {permission}")));
        self.sessions.append_message(&session_id, "user", request.prompt.trim())?;

        let mut changed = Vec::new();
        let files = context::snapshot(&project, &exclusions).unwrap_or_default();
        let max_rounds = self.config.max_tool_rounds;

        for round in 1..=max_rounds {
            if request.cancel.is_cancelled() {
                emit(&request.events, AgentEvent::Cancelled);
                return Err(Error::Cancelled);
            }
            emit(&request.events, AgentEvent::Status(format!("turn {round}")));
            let stored = self.sessions.messages(&session_id)?;
            let brief = clip_chars(&context::brief(&project, &files, &changed), 12_000);
            let instructions = format!(
                "{}\n\nPermission mode: {permission}.\n{brief}",
                protocol::tool_instructions()
            );
            let (history, message) = pack_history(&stored, &instructions, self.config.max_context_chars);
            let reply = self
                .complete(&project, &model, &message, &history, &request.cancel, &request.events)
                .await?;
            self.sessions.append_message(&session_id, "assistant", &reply)?;
            let (visible, calls) = extract_tool_calls(&reply);
            if !visible.is_empty() {
                emit(&request.events, AgentEvent::MessageCompleted { text: visible.clone() });
            }
            if calls.is_empty() {
                self.refresh_usage(&request.events).await;
                emit(&request.events, AgentEvent::Completed { summary: visible.clone() });
                return Ok(TaskOutcome {
                    session_id,
                    summary: visible,
                    finished: true,
                });
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
            };
            let mut packed = String::from("Tool results:\n");
            for call in calls {
                if request.cancel.is_cancelled() {
                    emit(&request.events, AgentEvent::Cancelled);
                    return Err(Error::Cancelled);
                }
                let outcome = tools::execute(&call, &ctx).await;
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
                    self.sessions.record_change(&session_id, &diff.path, &diff.diff)?;
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
        emit(&request.events, AgentEvent::Failed { message: summary.clone() });
        Ok(TaskOutcome {
            session_id,
            summary,
            finished: false,
        })
    }

    fn open_session(
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
            return Ok(id.clone());
        }
        let id = Uuid::new_v4().simple().to_string();
        let title = title_from(&request.prompt);
        self.sessions.create(&id, project, &title, model, permission)?;
        Ok(id)
    }

    async fn resolve_model(&self, requested: Option<&str>) -> Result<String> {
        if let Some(model) = requested.filter(|model| !model.is_empty()) {
            return Ok(model.to_string());
        }
        if let Some(model) = self.config.model.clone().filter(|model| !model.is_empty()) {
            return Ok(model);
        }
        // If no Serika session is signed in and we are on the default API URL, prefer optional BYOK keys or API-free Local Harness!
        let has_serika_session = self.secrets.load().ok().flatten().is_some();
        if !has_serika_session && self.config.api_base_url == crate::config::DEFAULT_API_BASE {
            if self.config.resolve_anthropic_key().is_some() {
                return Ok("claude/claude-sonnet-4-20250514".to_string());
            }
            if self.config.resolve_openai_key().is_some() {
                return Ok("openai/gpt-4.1".to_string());
            }
            if self.config.resolve_gemini_key().is_some() {
                return Ok("gemini/gemini-2.5-pro".to_string());
            }
            return Ok("local/workspace".to_string());
        }
        match self.client.models().await {
            Ok(catalog) => catalog
                .default_coding_model()
                .ok_or_else(|| Error::msg("Serika returned no chat models")),
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
        cancel: &CancellationToken,
        events: &EventSender,
    ) -> Result<String> {
        use crate::providers::{
            complete_anthropic, complete_gemini, complete_local, complete_openai, ProviderKind,
        };

        match ProviderKind::from_model_id(model) {
            ProviderKind::Local => {
                return complete_local(model, project, message, history, cancel, |chunk| {
                    emit(events, AgentEvent::MessageChunk { text: chunk.to_string() })
                })
                .await;
            }
            ProviderKind::OpenAi => {
                return complete_openai(
                    self.client.http(),
                    &self.config,
                    model,
                    message,
                    history,
                    cancel,
                    |chunk| emit(events, AgentEvent::MessageChunk { text: chunk.to_string() }),
                    |chunk| emit(events, AgentEvent::ReasoningChunk { text: chunk.to_string() }),
                )
                .await;
            }
            ProviderKind::Anthropic => {
                return complete_anthropic(
                    self.client.http(),
                    &self.config,
                    model,
                    message,
                    history,
                    cancel,
                    |chunk| emit(events, AgentEvent::MessageChunk { text: chunk.to_string() }),
                    |chunk| emit(events, AgentEvent::ReasoningChunk { text: chunk.to_string() }),
                )
                .await;
            }
            ProviderKind::Gemini => {
                return complete_gemini(
                    self.client.http(),
                    &self.config,
                    model,
                    message,
                    history,
                    cancel,
                    |chunk| emit(events, AgentEvent::MessageChunk { text: chunk.to_string() }),
                )
                .await;
            }
            ProviderKind::Serika => {}
        }

        let Some(mut session) = self.secrets.load()? else {
            // If pointing at default API and not signed in, fall back to API-free local workspace agent
            if self.config.api_base_url == crate::config::DEFAULT_API_BASE {
                return complete_local(
                    "local/workspace",
                    project,
                    message,
                    history,
                    cancel,
                    |chunk| emit(events, AgentEvent::MessageChunk { text: chunk.to_string() }),
                )
                .await;
            }
            return Err(Error::AuthRequired);
        };
        let mut refreshed = false;
        loop {
            let result = self
                .client
                .chat(
                    &session.token,
                    model,
                    message,
                    history,
                    cancel,
                    |chunk| emit(events, AgentEvent::MessageChunk { text: chunk.to_string() }),
                    |chunk| emit(events, AgentEvent::ReasoningChunk { text: chunk.to_string() }),
                )
                .await;
            match result {
                Err(Error::Auth(_)) if !refreshed => {
                    refreshed = true;
                    session = refresh_session(&self.client, session).await?;
                    self.secrets.save(&session)?;
                }
                other => return other,
            }
        }
    }

    async fn refresh_usage(&self, events: &EventSender) {
        let Ok(Some(session)) = self.secrets.load() else { return };
        if let Ok(usage) = self.client.usage(&session.token).await {
            emit(events, AgentEvent::UsageUpdated(usage));
        }
    }
}

async fn refresh_session(client: &crate::client::SerikaClient, mut session: StoredSession) -> Result<StoredSession> {
    match client.refresh(&session.refresh_token).await {
        Ok((token, refresh)) => {
            session.token = token;
            session.refresh_token = refresh;
            Ok(session)
        }
        Err(_) => Err(Error::Auth("the session expired. Sign in again.".into())),
    }
}

fn pack_history(messages: &[crate::sessions::StoredMessage], instructions: &str, max_chars: usize) -> (Vec<ChatTurn>, String) {
    let current = messages.last().map(|message| message.content.clone()).unwrap_or_default();
    let mut turns = vec![ChatTurn {
        role: "system".into(),
        content: instructions.to_string(),
    }];
    let prior = messages.len().saturating_sub(1);
    for message in &messages[..prior] {
        let role = if message.role == "assistant" { "assistant" } else { "user" };
        turns.push(ChatTurn {
            role: role.to_string(),
            content: message.content.clone(),
        });
    }
    (trim_history(turns, max_chars), current)
}

fn format_result(name: &str, outcome: &ToolOutcome) -> String {
    format!(
        "[{name}] {}\n{}\n\n",
        if outcome.ok { "ok" } else { "failed" },
        clip_chars(&outcome.output, 12_000)
    )
}

fn title_from(prompt: &str) -> String {
    let line = prompt.lines().find(|line| !line.trim().is_empty()).unwrap_or("New session");
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
        a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"))
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
