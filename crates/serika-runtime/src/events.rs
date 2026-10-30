use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::model::UsageSnapshot;

pub type EventSender = mpsc::UnboundedSender<AgentEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<AgentEvent>;

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started { session_id: String },
    Status(String),
    MessageChunk { text: String },
    ReasoningChunk { text: String },
    MessageCompleted { text: String },
    ToolRequested { id: String, name: String, arguments: String },
    ApprovalRequired { id: String, title: String, detail: String, destructive: bool },
    ToolStarted { id: String, name: String },
    ToolOutput { id: String, stream: String },
    ToolFinished { id: String, name: String, ok: bool, summary: String },
    FileChanged(FileDiff),
    UsageUpdated(UsageSnapshot),
    Completed { summary: String },
    Failed { message: String },
    Cancelled,
}

pub fn emit(tx: &EventSender, event: AgentEvent) {
    let _ = tx.send(event);
}

/// Bridges an agent task that is waiting and a UI thread that answers later.
#[derive(Default)]
pub struct ApprovalHub {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl ApprovalHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self, id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(id, tx);
        rx
    }

    pub fn resolve(&self, id: &str, allow: bool) -> bool {
        if let Some(tx) = self
            .pending
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(id)
        {
            let _ = tx.send(allow);
            true
        } else {
            false
        }
    }

    pub fn deny_all(&self) {
        let pending = std::mem::take(
            &mut *self.pending.lock().unwrap_or_else(|err| err.into_inner()),
        );
        for (_, tx) in pending {
            let _ = tx.send(false);
        }
    }
}

#[derive(Clone, Default)]
pub struct CancelFlag {
    pub token: CancellationToken,
}

impl CancelFlag {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}
