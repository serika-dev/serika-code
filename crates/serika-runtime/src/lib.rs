//! Shared agent runtime for the Serika Code CLI and desktop application.
//!
//! Model calls go through the Serika chat API (`POST /api/chat/message`) so the
//! server's account usage meter stays in charge. Local tools are described in
//! `protocol` because that API streams assistant text and does not return tool
//! calls to the client.

pub mod agent;
pub mod agents;
pub mod app;
pub mod attachments;
pub mod browser;
pub mod client;
pub mod config;
pub mod context;
pub mod credentials;
pub mod diffutil;
pub mod doctor;
pub mod error;
pub mod events;
pub mod gitinfo;
pub mod handoff;
pub mod subagents;
pub mod i18n;
pub mod logsetup;
pub mod model;
pub mod patch;
pub mod paths;
pub mod permissions;
pub mod protocol;
pub mod providers;
pub mod sessions;
pub mod setup;
pub mod shell;
pub mod tools;
pub mod updater;
pub mod workspace;

#[cfg(test)]
pub mod testsupport;

pub use agent::{TaskOutcome, TaskRequest};
pub use app::{Runtime, RuntimeOptions};
pub use client::LoginOutcome;
pub use config::{Config, ProjectSettings, DEFAULT_ACCOUNTS_BASE, DEFAULT_API_BASE};
pub use credentials::{MemoryStore, SecretStore, StoredSession};
pub use doctor::{Check, CheckLevel, DoctorReport};
pub use error::{Error, Result};
pub use events::{AgentEvent, ApprovalHub, EventReceiver, EventSender, FileDiff};
pub use model::{AccountProfile, ChatModel, ModelCatalog, UsageSnapshot};
pub use paths::AppPaths;
pub use permissions::{PermissionMode, ActionKind, Gate};
pub use sessions::{SessionStore, SessionSummary, StoredMessage, ToolRun};
