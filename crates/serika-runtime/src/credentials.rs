use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::error::{Error, Result};

const SERVICE: &str = "serika-code";
const USER: &str = "session";

/// Session material kept in the OS credential store, never in config or logs.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub is_premium: bool,
}

/// Debug output never includes the tokens.
impl std::fmt::Debug for StoredSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredSession")
            .field("token", &"[redacted]")
            .field("refresh_token", &"[redacted]")
            .field("user_id", &self.user_id)
            .field("username", &self.username)
            .field("email", &self.email)
            .field("is_premium", &self.is_premium)
            .finish()
    }
}

impl Drop for StoredSession {
    fn drop(&mut self) {
        self.token.zeroize();
        self.refresh_token.zeroize();
    }
}

pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredSession>>;
    fn save(&self, session: &StoredSession) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredSession>> {
        let entry = keyring::Entry::new(SERVICE, USER).map_err(|err| Error::msg(err.to_string()))?;
        match entry.get_password() {
            Ok(raw) => {
                let session = serde_json::from_str(&raw).map_err(|err| {
                    Error::msg(format!("stored credentials are unreadable ({err}). Sign in again."))
                })?;
                Ok(Some(session))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(Error::msg(format!("credential store: {err}"))),
        }
    }

    fn save(&self, session: &StoredSession) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, USER).map_err(|err| Error::msg(err.to_string()))?;
        let raw = serde_json::to_string(session)?;
        entry
            .set_password(&raw)
            .map_err(|err| Error::msg(format!("credential store: {err}")))?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, USER).map_err(|err| Error::msg(err.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(Error::msg(format!("credential store: {err}"))),
        }
    }
}

/// In-memory store for tests. Production builds use [`KeyringStore`].
pub struct MemoryStore {
    inner: Mutex<Option<StoredSession>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<StoredSession>> {
        Ok(self.inner.lock().unwrap_or_else(|err| err.into_inner()).clone())
    }

    fn save(&self, session: &StoredSession) -> Result<()> {
        *self.inner.lock().unwrap_or_else(|err| err.into_inner()) = Some(session.clone());
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.inner.lock().unwrap_or_else(|err| err.into_inner()) = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip_and_clear() {
        let store = MemoryStore::new();
        assert!(store.load().unwrap().is_none());
        store
            .save(&StoredSession {
                token: "t".into(),
                refresh_token: "r".into(),
                user_id: "u".into(),
                username: "noa".into(),
                email: "noa@example.com".into(),
                is_premium: false,
            })
            .unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.username, "noa");
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
