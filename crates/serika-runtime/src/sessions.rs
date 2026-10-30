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
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

pub struct RewindOutcome {
    pub prompt: String,
    pub restored_files: usize,
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
             CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id);
             CREATE INDEX IF NOT EXISTS snapshots_session ON snapshots(session_id, message_id);
             CREATE INDEX IF NOT EXISTS tool_runs_session ON tool_runs(session_id);
             CREATE INDEX IF NOT EXISTS changes_session ON changes(session_id);
             CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at);",
        )?;
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
            "INSERT INTO projects (path, last_opened) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened",
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
            "INSERT INTO projects (path, last_opened) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened",
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
        Ok(())
    }

    pub fn list(&self, project: Option<&Path>) -> Result<Vec<SessionSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, model_id, permission, updated_at
             FROM sessions ORDER BY updated_at DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                project_path: row.get(1)?,
                title: row.get(2)?,
                model_id: row.get(3)?,
                permission: row.get(4)?,
                updated_at: row.get(5)?,
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

    pub fn get(&self, id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, model_id, permission, updated_at
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
            "INSERT INTO projects (path, last_opened) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened",
            params![project.to_string_lossy().to_string(), now_secs()],
        )?;
        Ok(())
    }

    pub fn recent_projects(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path FROM projects ORDER BY last_opened DESC LIMIT 20",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }

    pub fn append_message(&self, session_id: &str, role: &str, content: &str) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, role, content, now_secs()],
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
            "SELECT id, role, content, created_at FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
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

    /// Drop this user turn and everything after it, and put edited files back.
    pub fn rewind(&self, session_id: &str, message_id: i64) -> Result<RewindOutcome> {
        let conn = self.lock();
        let (role, prompt, created_at): (String, String, i64) = conn
            .query_row(
                "SELECT role, content, created_at FROM messages WHERE session_id = ?1 AND id = ?2",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
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
        Ok(RewindOutcome {
            prompt,
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
}
