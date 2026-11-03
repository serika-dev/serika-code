use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::error::{Error, Result};
use crate::permissions::PermissionMode;

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub project_path: String,
    pub title: String,
    pub model_id: String,
    pub permission: String,
    pub updated_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    /// JSON list of `attachments::Attachment` for a user message.
    pub attachments: Option<String>,
    /// The model the chat was using when this message was written.
    pub model: Option<String>,
    pub created_at: i64,
}

pub struct RewindOutcome {
    pub prompt: String,
    /// JSON list of the attachments the rewound message carried.
    pub attachments: Option<String>,
    pub restored_files: usize,
}

/// A fresh chat id, in the same form the agent uses.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Tool results travel back to the model as user messages; they are not turns the user wrote.
pub fn is_tool_result(role: &str, content: &str) -> bool {
    role == "user" && content.starts_with("Tool results:\n")
}

#[derive(Debug, Clone)]
pub struct ToolRun {
    pub name: String,
    pub arguments: String,
    pub output: String,
    pub ok: bool,
    pub created_at: i64,
}

pub struct SessionStore {
    conn: Mutex<Connection>,
}

impl SessionStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project_path TEXT NOT NULL,
                title TEXT NOT NULL,
                model_id TEXT NOT NULL,
                permission TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                reasoning TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS tool_runs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                name TEXT NOT NULL,
                arguments TEXT NOT NULL,
                output TEXT NOT NULL,
                ok INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS changes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                path TEXT NOT NULL,
                diff TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS projects (
                path TEXT PRIMARY KEY,
                last_opened INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                message_id INTEGER NOT NULL,
                path TEXT NOT NULL,
                existed INTEGER NOT NULL,
                content BLOB,
                UNIQUE(session_id, message_id, path)
             );
             CREATE TABLE IF NOT EXISTS provider_sessions (
                session_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                provider_session TEXT NOT NULL,
                PRIMARY KEY(session_id, provider),
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id);
             CREATE INDEX IF NOT EXISTS snapshots_session ON snapshots(session_id, message_id);
             CREATE INDEX IF NOT EXISTS tool_runs_session ON tool_runs(session_id);
             CREATE INDEX IF NOT EXISTS changes_session ON changes(session_id);
             CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at);",
        )?;
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN reasoning TEXT", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachments TEXT", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN model TEXT", []);
        let _ = conn.execute("ALTER TABLE provider_sessions ADD COLUMN synced_through INTEGER", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN parent_id TEXT", []);
        let _ = conn.execute("ALTER TABLE projects ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0", []);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn integrity_ok(&self) -> Result<bool> {
        let conn = self.lock();
        let status: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(status == "ok")
    }

    pub fn purge_expired(&self, retention_days: u32) -> Result<usize> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = now_secs() - i64::from(retention_days) * 86_400;
        let conn = self.lock();
        let count = conn.execute("DELETE FROM sessions WHERE updated_at < ?1", params![cutoff])?;
        Ok(count)
    }

    pub fn create(
        &self,
        id: &str,
        project: &Path,
        title: &str,
        model_id: &str,
        permission: PermissionMode,
    ) -> Result<()> {
        let now = now_secs();
        let project = project.to_string_lossy().to_string();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO sessions (id, project_path, title, model_id, permission, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, project, title, model_id, permission.as_str(), now],
        )?;
        conn.execute(
            "INSERT INTO projects (path, last_opened, hidden) VALUES (?1, ?2, 0)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened, hidden = 0",
            params![project, now],
        )?;
        Ok(())
    }

    pub fn touch(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        Ok(())
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.lock();
        let count = conn.execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now_secs(), id],
        )?;
        if count == 0 {
            return Err(Error::msg("session not found"));
        }
        Ok(())
    }

    pub fn relocate_project(&self, id: &str, project: &Path) -> Result<()> {
        let now = now_secs();
        let project = project.to_string_lossy().to_string();
        let conn = self.lock();
        let count = conn.execute(
            "UPDATE sessions SET project_path = ?1, updated_at = ?2 WHERE id = ?3",
            params![project, now, id],
        )?;
        if count == 0 {
            return Err(Error::msg("session not found"));
        }
        conn.execute(
            "INSERT INTO projects (path, last_opened, hidden) VALUES (?1, ?2, 0)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened, hidden = 0",
            params![project, now],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        let count = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if count == 0 {
            return Err(Error::msg("session not found"));
        }
        // Sub-agent chats go with their parent.
        conn.execute("DELETE FROM sessions WHERE parent_id = ?1", params![id])?;
        Ok(())
    }

    pub fn list(&self, project: Option<&Path>) -> Result<Vec<SessionSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, model_id, permission, updated_at, created_at
             FROM sessions WHERE parent_id IS NULL ORDER BY updated_at DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                project_path: row.get(1)?,
                title: row.get(2)?,
                model_id: row.get(3)?,
                permission: row.get(4)?,
                updated_at: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut sessions = Vec::new();
        let wanted = project.map(|path| {
            dunce::canonicalize(path)
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .to_string()
        });
        for row in rows {
            let row = row?;
            if let Some(wanted) = &wanted {
                let matches = if cfg!(windows) {
                    row.project_path
                        .replace('/', "\\")
                        .eq_ignore_ascii_case(&wanted.replace('/', "\\"))
                } else {
                    &row.project_path == wanted
                };
                if !matches {
                    continue;
                }
            }
            sessions.push(row);
        }
        Ok(sessions)
    }

    /// Mark a chat as a sub-agent of another. Sub-agent chats stay out of the chat list.
    pub fn set_parent(&self, id: &str, parent: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("UPDATE sessions SET parent_id = ?2 WHERE id = ?1", params![id, parent])?;
        Ok(())
    }

    pub fn parent(&self, id: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let value: Option<Option<String>> = conn
            .query_row("SELECT parent_id FROM sessions WHERE id = ?1", params![id], |row| row.get(0))
            .ok();
        Ok(value.flatten())
    }

    /// Sub-agent chats a chat started, oldest first.
    pub fn children(&self, parent: &str) -> Result<Vec<SessionSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, model_id, permission, updated_at, created_at
             FROM sessions WHERE parent_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![parent], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                project_path: row.get(1)?,
                title: row.get(2)?,
                model_id: row.get(3)?,
                permission: row.get(4)?,
                updated_at: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    pub fn get(&self, id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, model_id, permission, updated_at, created_at
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                project_path: row.get(1)?,
                title: row.get(2)?,
                model_id: row.get(3)?,
                permission: row.get(4)?,
                updated_at: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Tool execution history for one session, oldest first.
    pub fn tool_runs(&self, session_id: &str) -> Result<Vec<ToolRun>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT name, arguments, output, ok, created_at FROM tool_runs
             WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(ToolRun {
                name: row.get(0)?,
                arguments: row.get(1)?,
                output: row.get(2)?,
                ok: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    /// Remember a project in the recent list without creating a session.
    pub fn remember_project(&self, project: &Path) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO projects (path, last_opened, hidden) VALUES (?1, ?2, 0)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened, hidden = 0",
            params![project.to_string_lossy().to_string(), now_secs()],
        )?;
        Ok(())
    }

    /// Id a subscription CLI (Claude Code, Codex) gave this conversation, used to resume it.
    pub fn provider_session(&self, session_id: &str, provider: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT provider_session FROM provider_sessions WHERE session_id = ?1 AND provider = ?2",
        )?;
        let mut rows = stmt.query_map(params![session_id, provider], |row| row.get(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn set_provider_session(&self, session_id: &str, provider: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO provider_sessions (session_id, provider, provider_session) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, provider) DO UPDATE SET provider_session = excluded.provider_session",
            params![session_id, provider, value],
        )?;
        Ok(())
    }

    /// The last message a provider's own session has seen. Later messages came from other models.
    pub fn provider_synced(&self, session_id: &str, provider: &str) -> Result<Option<i64>> {
        let conn = self.lock();
        let value: Option<Option<i64>> = conn
            .query_row(
                "SELECT synced_through FROM provider_sessions WHERE session_id = ?1 AND provider = ?2",
                params![session_id, provider],
                |row| row.get(0),
            )
            .ok();
        Ok(value.flatten())
    }

    pub fn set_provider_synced(&self, session_id: &str, provider: &str, message_id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE provider_sessions SET synced_through = ?3 WHERE session_id = ?1 AND provider = ?2",
            params![session_id, provider, message_id],
        )?;
        Ok(())
    }

    /// The model a chat continues with. New messages are tagged with it.
    pub fn set_model(&self, session_id: &str, model: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE sessions SET model_id = ?2 WHERE id = ?1",
            params![session_id, model],
        )?;
        Ok(())
    }

    /// The newest message id in a chat.
    pub fn last_message_id(&self, session_id: &str) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?)
    }

    /// Remove a project from the project list. Its chats stay in the history.
    /// Hide a project from the list. Its chats stay saved; it returns when the folder is opened again.
    pub fn forget_project(&self, project: &Path) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO projects (path, last_opened, hidden) VALUES (?1, ?2, 1)
             ON CONFLICT(path) DO UPDATE SET hidden = 1",
            params![project.to_string_lossy().to_string(), now_secs()],
        )?;
        Ok(())
    }

    /// Delete every chat (and its sub-agents) stored for this exact folder spelling.
    pub fn delete_project_chats(&self, project: &Path) -> Result<usize> {
        let conn = self.lock();
        let path = project.to_string_lossy().to_string();
        conn.execute(
            "DELETE FROM sessions WHERE parent_id IN (SELECT id FROM sessions WHERE project_path = ?1)",
            params![path],
        )?;
        Ok(conn.execute("DELETE FROM sessions WHERE project_path = ?1", params![path])?)
    }

    /// Every project the app knows, newest first: remembered folders plus folders that have chats.
    pub fn projects(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path, MAX(seen) FROM (
                SELECT path, last_opened AS seen FROM projects WHERE hidden = 0
                UNION ALL
                SELECT project_path AS path, updated_at AS seen FROM sessions
                WHERE parent_id IS NULL AND project_path NOT IN (SELECT path FROM projects WHERE hidden = 1)
             ) GROUP BY path ORDER BY MAX(seen) DESC LIMIT 80",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    pub fn recent_projects(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path FROM projects WHERE hidden = 0 ORDER BY last_opened DESC LIMIT 20",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    pub fn append_message(&self, session_id: &str, role: &str, content: &str) -> Result<i64> {
        self.append_message_with_reasoning(session_id, role, content, None)
    }

    /// A user message with the files and images attached to it.
    pub fn append_user_message(
        &self,
        session_id: &str,
        content: &str,
        attachments: Option<&str>,
    ) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, attachments, created_at, model)
             VALUES (?1, 'user', ?2, ?3, ?4, (SELECT model_id FROM sessions WHERE id = ?1))",
            params![session_id, content, attachments, now_secs()],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), session_id],
        )?;
        Ok(id)
    }

    pub fn append_message_with_reasoning(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        reasoning: Option<&str>,
    ) -> Result<i64> {
        let clean_reasoning = reasoning.map(str::trim).filter(|s| !s.is_empty());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, reasoning, created_at, model)
             VALUES (?1, ?2, ?3, ?4, ?5, (SELECT model_id FROM sessions WHERE id = ?1))",
            params![session_id, role, content, clean_reasoning, now_secs()],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), session_id],
        )?;
        Ok(id)
    }

    pub fn messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, created_at, reasoning, attachments, model FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let role: String = row.get(1)?;
            let raw_content: String = row.get(2)?;
            let db_reasoning: Option<String> = row.get(4)?;
            let (content, reasoning) = if role == "assistant" {
                let (without_think, inline_think) = crate::protocol::extract_think_blocks(&raw_content);
                let combined_reasoning = match (
                    db_reasoning.filter(|s| !s.trim().is_empty()),
                    (!inline_think.is_empty()).then_some(inline_think),
                ) {
                    (Some(a), Some(b)) if !a.contains(&b) => Some(format!("{a}\n\n{b}")),
                    (Some(a), _) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                (without_think, combined_reasoning)
            } else {
                (raw_content, db_reasoning)
            };
            Ok(StoredMessage {
                id: row.get(0)?,
                role,
                content,
                reasoning,
                attachments: row.get(5)?,
                model: row.get(6)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    /// Keep the first copy of a file for this user turn. Later edits in the same turn do not overwrite it.
    pub fn save_snapshot(
        &self,
        session_id: &str,
        message_id: i64,
        path: &str,
        existed: bool,
        content: &[u8],
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO snapshots (session_id, message_id, path, existed, content)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, message_id, path, existed as i32, content],
        )?;
        Ok(())
    }

    /// Ids and times of the messages the user wrote, oldest first. The desktop addresses a
    /// turn by its position in this list, which is the same for a live chat and a reloaded one.
    pub fn user_turns(&self, session_id: &str) -> Result<Vec<(i64, i64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, content, created_at FROM messages WHERE session_id = ?1 AND role = 'user' ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, content, created_at) = row?;
            if !is_tool_result("user", &content) {
                out.push((id, created_at));
            }
        }
        Ok(out)
    }

    /// Message id of the user's `index`-th turn (0-based).
    pub fn user_turn_id(&self, session_id: &str, index: usize) -> Result<i64> {
        self.user_turns(session_id)?
            .get(index)
            .map(|(id, _)| *id)
            .ok_or_else(|| Error::msg("that message is no longer in this chat"))
    }

    /// Remove one exchange: the user's `index`-th message and the reply to it. Files stay as they are.
    pub fn delete_turn(&self, session_id: &str, index: usize) -> Result<()> {
        let turns = self.user_turns(session_id)?;
        let (start, start_at) = *turns
            .get(index)
            .ok_or_else(|| Error::msg("that message is no longer in this chat"))?;
        let next = turns.get(index + 1).copied();
        let conn = self.lock();
        match next {
            Some((end, end_at)) => {
                conn.execute(
                    "DELETE FROM messages WHERE session_id = ?1 AND id >= ?2 AND id < ?3",
                    params![session_id, start, end],
                )?;
                for table in ["tool_runs", "changes"] {
                    conn.execute(
                        &format!("DELETE FROM {table} WHERE session_id = ?1 AND created_at >= ?2 AND created_at < ?3"),
                        params![session_id, start_at, end_at],
                    )?;
                }
                conn.execute(
                    "DELETE FROM snapshots WHERE session_id = ?1 AND message_id >= ?2 AND message_id < ?3",
                    params![session_id, start, end],
                )?;
            }
            None => {
                conn.execute(
                    "DELETE FROM messages WHERE session_id = ?1 AND id >= ?2",
                    params![session_id, start],
                )?;
                for table in ["tool_runs", "changes"] {
                    conn.execute(
                        &format!("DELETE FROM {table} WHERE session_id = ?1 AND created_at >= ?2"),
                        params![session_id, start_at],
                    )?;
                }
                conn.execute(
                    "DELETE FROM snapshots WHERE session_id = ?1 AND message_id >= ?2",
                    params![session_id, start],
                )?;
            }
        }
        // The provider's own copy of the conversation still has the removed turn.
        conn.execute("DELETE FROM provider_sessions WHERE session_id = ?1", params![session_id])?;
        Ok(())
    }

    /// Copy a chat up to and including the reply to the user's `index`-th message into a new chat.
    pub fn fork(&self, session_id: &str, index: usize, new_id: &str) -> Result<()> {
        let turns = self.user_turns(session_id)?;
        if index >= turns.len() {
            return Err(Error::msg("that message is no longer in this chat"));
        }
        let end = turns.get(index + 1).map(|(id, at)| (*id, *at));
        let conn = self.lock();
        let (project, title, model, permission): (String, String, String, String) = conn.query_row(
            "SELECT project_path, title, model_id, permission FROM sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let now = now_secs();
        conn.execute(
            "INSERT INTO sessions (id, project_path, title, model_id, permission, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![new_id, project, format!("{title} (fork)"), model, permission, now],
        )?;
        let (end_id, end_at) = end.unwrap_or((i64::MAX, i64::MAX));
        conn.execute(
            "INSERT INTO messages (session_id, role, content, reasoning, attachments, created_at, model)
             SELECT ?1, role, content, reasoning, attachments, created_at, model FROM messages
             WHERE session_id = ?2 AND id < ?3 ORDER BY id ASC",
            params![new_id, session_id, end_id],
        )?;
        conn.execute(
            "INSERT INTO tool_runs (id, session_id, name, arguments, output, ok, created_at)
             SELECT ?1 || '-' || id, ?1, name, arguments, output, ok, created_at FROM tool_runs
             WHERE session_id = ?2 AND created_at < ?3",
            params![new_id, session_id, end_at],
        )?;
        conn.execute(
            "INSERT INTO changes (session_id, path, diff, created_at)
             SELECT ?1, path, diff, created_at FROM changes WHERE session_id = ?2 AND created_at < ?3",
            params![new_id, session_id, end_at],
        )?;
        Ok(())
    }

    /// Drop this user turn and everything after it, and put edited files back.
    pub fn rewind(&self, session_id: &str, message_id: i64) -> Result<RewindOutcome> {
        let conn = self.lock();
        let (role, prompt, created_at, attachments): (String, String, i64, Option<String>) = conn
            .query_row(
                "SELECT role, content, created_at, attachments FROM messages WHERE session_id = ?1 AND id = ?2",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| Error::msg("that message is not in this session"))?;
        if role != "user" {
            return Err(Error::msg("rewind starts from a message you sent"));
        }
        let project: String = conn.query_row(
            "SELECT project_path FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        let root = Path::new(&project);
        let mut stmt = conn.prepare(
            "SELECT path, existed, content FROM snapshots
             WHERE session_id = ?1 AND message_id >= ?2
               AND message_id = (
                 SELECT MIN(message_id) FROM snapshots AS older
                 WHERE older.session_id = snapshots.session_id
                   AND older.path = snapshots.path
                   AND older.message_id >= ?2
               )",
        )?;
        let rows = stmt.query_map(params![session_id, message_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? != 0,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let files = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut restored = 0usize;
        for (rel, existed, content) in files {
            let full = crate::workspace::resolve_inside(root, &rel)?;
            if existed {
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&full, content)?;
            } else if full.is_file() {
                std::fs::remove_file(&full)?;
            }
            restored += 1;
        }
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND id >= ?2",
            params![session_id, message_id],
        )?;
        conn.execute(
            "DELETE FROM snapshots WHERE session_id = ?1 AND message_id >= ?2",
            params![session_id, message_id],
        )?;
        conn.execute(
            "DELETE FROM tool_runs WHERE session_id = ?1 AND created_at >= ?2",
            params![session_id, created_at],
        )?;
        conn.execute(
            "DELETE FROM changes WHERE session_id = ?1 AND created_at >= ?2",
            params![session_id, created_at],
        )?;
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), session_id],
        )?;
        conn.execute("DELETE FROM provider_sessions WHERE session_id = ?1", params![session_id])?;
        Ok(RewindOutcome {
            prompt,
            attachments,
            restored_files: restored,
        })
    }

    pub fn record_tool(
        &self,
        id: &str,
        session_id: &str,
        name: &str,
        arguments: &str,
        output: &str,
        ok: bool,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO tool_runs (id, session_id, name, arguments, output, ok, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, session_id, name, arguments, output, ok as i32, now_secs()],
        )?;
        Ok(())
    }

    pub fn record_change(&self, session_id: &str, path: &str, diff: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO changes (session_id, path, diff, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, path, diff, now_secs()],
        )?;
        Ok(())
    }

    pub fn changes(&self, session_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path, diff FROM changes WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|err| err.into_inner())
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_and_resumes_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sessions.db");
        let store = SessionStore::open(&db).unwrap();
        store
            .create("s1", dir.path(), "Fix auth", "deepseek/deepseek-v4.1-flash", PermissionMode::Standard)
            .unwrap();
        store.append_message("s1", "user", "fix it").unwrap();
        store.append_message("s1", "assistant", "done").unwrap();
        store.record_tool("t1", "s1", "read_file", "{}", "ok", true).unwrap();
        store.record_change("s1", "src/lib.rs", "--- a\n+++ b\n").unwrap();
        let reopened = SessionStore::open(&db).unwrap();
        let messages = reopened.messages("s1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "done");
        assert_eq!(reopened.changes("s1").unwrap().len(), 1);
        assert!(reopened.integrity_ok().unwrap());
        reopened.rename("s1", "Renamed").unwrap();
        assert_eq!(reopened.get("s1").unwrap().unwrap().title, "Renamed");
        assert_eq!(reopened.tool_runs("s1").unwrap().len(), 1);
        let turn = reopened.append_message("s1", "user", "change it").unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "two").unwrap();
        reopened.save_snapshot("s1", turn, "a.txt", true, b"one").unwrap();
        let outcome = reopened.rewind("s1", turn).unwrap();
        assert_eq!(outcome.prompt, "change it");
        assert_eq!(outcome.restored_files, 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one");
        assert!(reopened.messages("s1").unwrap().iter().all(|message| message.id < turn));
        reopened.delete("s1").unwrap();
        assert!(reopened.messages("s1").unwrap().is_empty());
        assert!(reopened.tool_runs("s1").unwrap().is_empty());
        assert!(reopened.changes("s1").unwrap().is_empty());
        assert!(reopened.get("s1").unwrap().is_none());
    }

    fn three_turns(store: &SessionStore, dir: &Path) {
        store
            .create("s1", dir, "Chat", "claude-code/opus", PermissionMode::Standard)
            .unwrap();
        for (prompt, reply) in [("one", "first"), ("two", "second"), ("three", "third")] {
            store.append_user_message("s1", prompt, Some("[]")).unwrap();
            store.append_message("s1", "user", "Tool results:\nok").unwrap();
            store.append_message("s1", "assistant", reply).unwrap();
        }
        store.set_provider_session("s1", "claude", "abc").unwrap();
    }

    #[test]
    fn removed_projects_stay_hidden_until_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("s.db")).unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        store.create("s1", &project, "Chat", "m", PermissionMode::Standard).unwrap();
        let shown = |store: &SessionStore| store.projects().unwrap().iter().any(|(p, _)| Path::new(p) == project);
        assert!(shown(&store));
        store.forget_project(&project).unwrap();
        assert!(!shown(&store), "a project with chats must stay removed");
        store.remember_project(&project).unwrap();
        assert!(shown(&store));
        assert_eq!(store.delete_project_chats(&project).unwrap(), 1);
        assert!(store.get("s1").unwrap().is_none());
    }

    #[test]
    fn user_turns_skip_tool_results() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("s.db")).unwrap();
        three_turns(&store, dir.path());
        let turns = store.user_turns("s1").unwrap();
        assert_eq!(turns.len(), 3);
        let second = store.user_turn_id("s1", 1).unwrap();
        let outcome = store.rewind("s1", second).unwrap();
        assert_eq!(outcome.prompt, "two");
        assert_eq!(outcome.attachments.as_deref(), Some("[]"));
        assert_eq!(store.user_turns("s1").unwrap().len(), 1);
        // The provider's transcript no longer matches, so it is dropped.
        assert_eq!(store.provider_session("s1", "claude").unwrap(), None);
        assert!(store.user_turn_id("s1", 5).is_err());
    }

    #[test]
    fn deletes_one_exchange_in_the_middle() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("s.db")).unwrap();
        three_turns(&store, dir.path());
        store.delete_turn("s1", 1).unwrap();
        let texts: Vec<String> = store
            .messages("s1")
            .unwrap()
            .into_iter()
            .filter(|m| !is_tool_result(&m.role, &m.content))
            .map(|m| m.content)
            .collect();
        assert_eq!(texts, vec!["one", "first", "three", "third"]);
        store.delete_turn("s1", 1).unwrap();
        assert_eq!(store.user_turns("s1").unwrap().len(), 1);
    }

    #[test]
    fn forks_up_to_a_reply() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("s.db")).unwrap();
        three_turns(&store, dir.path());
        store.record_tool("t1", "s1", "read_file", "{}", "ok", true).unwrap();
        store.fork("s1", 1, "s2").unwrap();
        let fork = store.get("s2").unwrap().unwrap();
        assert_eq!(fork.title, "Chat (fork)");
        assert_eq!(fork.model_id, "claude-code/opus");
        let texts: Vec<String> = store
            .messages("s2")
            .unwrap()
            .into_iter()
            .filter(|m| m.role == "assistant")
            .map(|m| m.content)
            .collect();
        assert_eq!(texts, vec!["first", "second"]);
        // The original is untouched.
        assert_eq!(store.user_turns("s1").unwrap().len(), 3);
        assert!(store.fork("s1", 9, "s3").is_err());
    }
}
