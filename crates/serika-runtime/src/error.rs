use crate::model::UsageLimitBody;

/// Recoverable failures from the runtime. Library paths return this instead of panicking.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("not signed in")]
    AuthRequired,
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("{0}")]
    UsageLimit(UsageLimitBody),
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("cancelled")]
    Cancelled,
    #[error("serika API {status}: {message}")]
    Api {
        status: u16,
        message: String,
        code: Option<String>,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UsageLimit(_) => 2,
            Self::AuthRequired | Self::Auth(_) => 3,
            Self::Cancelled => 4,
            Self::RateLimited(_) => 5,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
