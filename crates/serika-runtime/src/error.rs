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
    #[error("{}", describe_http(.0))]
    Http(#[from] reqwest::Error),
}

/// A network failure in words, with the real cause. reqwest's own text is only
/// "error sending request for url (...)", which hides whether it was a timeout, a refused
/// connection, or a connection the server had already closed.
pub fn describe_http(err: &reqwest::Error) -> String {
    let mut causes: Vec<String> = Vec::new();
    let mut current = std::error::Error::source(err);
    while let Some(cause) = current {
        let text = cause.to_string();
        if !text.is_empty() && !causes.contains(&text) {
            causes.push(text);
        }
        current = cause.source();
    }
    let headline = if err.is_timeout() {
        "Serika did not answer in time."
    } else if err.is_connect() {
        "Could not connect to Serika. Check your internet connection."
    } else if err.is_request() {
        "The connection to Serika was interrupted. Try again."
    } else {
        "The request to Serika failed."
    };
    if causes.is_empty() {
        headline.to_string()
    } else {
        format!("{headline} ({})", causes.join(": "))
    }
}

/// Connection-level failures where nothing reached the server or the reply never started.
/// A pooled connection the server already closed shows up this way.
pub fn is_transient(err: &reqwest::Error) -> bool {
    !err.is_timeout() && (err.is_connect() || err.is_request())
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
