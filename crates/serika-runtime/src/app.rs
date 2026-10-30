use std::sync::Arc;

use crate::client::SerikaClient;
use crate::config::Config;
use crate::credentials::{KeyringStore, SecretStore, StoredSession};
use crate::error::{Error, Result};
use crate::events::ApprovalHub;
use crate::model::{AccountProfile, ModelCatalog, UsageSnapshot};
use crate::paths::AppPaths;
use crate::sessions::SessionStore;

pub struct RuntimeOptions {
    pub paths: AppPaths,
    pub config: Config,
    pub secrets: Arc<dyn SecretStore>,
}

pub struct Runtime {
    pub config: Config,
    pub paths: AppPaths,
    pub sessions: SessionStore,
    pub secrets: Arc<dyn SecretStore>,
    pub client: SerikaClient,
    pub approvals: Arc<ApprovalHub>,
}

impl Runtime {
    pub fn open() -> Result<Self> {
        let paths = AppPaths::resolve()?;
        paths.ensure()?;
        let mut config = Config::load_or_create(&paths)?;
        config.apply_env_overrides();
        config.validate()?;
        crate::logsetup::init(&paths, &config.log_level)?;
        Self::assemble(paths, config, Arc::new(KeyringStore))
    }

    pub fn open_with(options: RuntimeOptions) -> Result<Self> {
        options.paths.ensure()?;
        options.config.validate()?;
        Self::assemble(options.paths, options.config, options.secrets)
    }

    fn assemble(paths: AppPaths, config: Config, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let sessions = SessionStore::open(&paths.database)?;
        let _ = sessions.purge_expired(config.session_retention_days);
        let client = SerikaClient::new(&config.api_base_url, &config.accounts_base_url)?;
        Ok(Self {
            config,
            paths,
            sessions,
            secrets,
            client,
            approvals: Arc::new(ApprovalHub::new()),
        })
    }

    pub fn save_config(&self) -> Result<()> {
        self.config.save(&self.paths.config_file)
    }

    pub async fn login_password(
        &self,
        email: &str,
        password: &str,
        two_factor: Option<&str>,
    ) -> Result<crate::client::LoginOutcome> {
        let outcome = self.client.login_password(email, password, two_factor).await?;
        self.persist_login(outcome)
    }

    pub async fn request_magic_link(&self, email: &str) -> Result<String> {
        self.client.request_magic_link(email).await
    }

    pub async fn consume_magic_link(
        &self,
        token: &str,
        two_factor: Option<&str>,
    ) -> Result<crate::client::LoginOutcome> {
        let outcome = self.client.consume_magic_link(token, two_factor).await?;
        self.persist_login(outcome)
    }

    fn persist_login(&self, outcome: crate::client::LoginOutcome) -> Result<crate::client::LoginOutcome> {
        if let crate::client::LoginOutcome::SignedIn(session) = &outcome {
            self.secrets.save(session)?;
        }
        Ok(outcome)
    }

    pub async fn logout(&self) -> Result<()> {
        if let Some(session) = self.secrets.load()? {
            if let Err(err) = self.client.logout(&session.token).await {
                tracing::warn!("server logout did not complete: {err}");
            }
        }
        self.secrets.clear()?;
        Ok(())
    }

    pub fn signed_in(&self) -> Result<Option<StoredSession>> {
        self.secrets.load()
    }

    pub async fn account(&self) -> Result<AccountProfile> {
        let session = self.token()?.ok_or(Error::AuthRequired)?;
        match self.client.account(&session.token).await {
            Ok(profile) => Ok(profile),
            Err(Error::Auth(_)) => {
                let session = self.refresh_saved(session).await?;
                self.client.account(&session.token).await
            }
            Err(err) => Err(err),
        }
    }

    pub async fn models(&self) -> Result<ModelCatalog> {
        self.client.models().await
    }

    pub async fn usage(&self) -> Result<UsageSnapshot> {
        let session = self.token()?.ok_or(Error::AuthRequired)?;
        match self.client.usage(&session.token).await {
            Ok(usage) => Ok(usage),
            Err(Error::Auth(_)) => {
                let session = self.refresh_saved(session).await?;
                self.client.usage(&session.token).await
            }
            Err(err) => Err(err),
        }
    }

    fn token(&self) -> Result<Option<StoredSession>> {
        self.secrets.load()
    }

    async fn refresh_saved(&self, session: StoredSession) -> Result<StoredSession> {
        let (token, refresh) = self
            .client
            .refresh(&session.refresh_token)
            .await
            .map_err(|_| Error::Auth("the session expired. Sign in again.".into()))?;
        let mut updated = session.clone();
        updated.token = token;
        updated.refresh_token = refresh;
        self.secrets.save(&updated)?;
        Ok(updated)
    }
}
