use std::sync::mpsc;

use crate::MessageKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    Locking,
    Connecting {
        source: String,
    },
    FetchingManifest,
    ValidatingManifest,
    Downloading {
        artifact: String,
        downloaded: u64,
        total: u64,
    },
    Verifying {
        artifact: String,
    },
    PreparingLocalBackup,
    RestoringSkills,
    ImportingDatabase,
    ApplyingProvider {
        agent: String,
    },
    ApplyingMcp {
        agent: String,
    },
    ApplyingSkills {
        agent: String,
        completed: usize,
        total: usize,
    },
    Retrying {
        operation: String,
        attempt: u8,
        max_attempts: u8,
    },
    Warning {
        stage: String,
        agent: Option<String>,
        message_key: MessageKey,
        detail: String,
    },
    Completed {
        duration_ms: u128,
        snapshot_id: String,
    },
    Failed {
        stage: String,
        message_key: MessageKey,
        detail: String,
        retryable: bool,
    },
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);

    fn emit_skill(&self, agent: String, _skill: String, completed: usize, total: usize) {
        self.emit(ProgressEvent::ApplyingSkills {
            agent,
            completed,
            total,
        });
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProgress;

impl ProgressSink for NoopProgress {
    fn emit(&self, _event: ProgressEvent) {}
}

#[derive(Clone)]
pub struct ChannelProgress {
    sender: mpsc::Sender<ProgressEvent>,
}

impl ChannelProgress {
    pub fn new(sender: mpsc::Sender<ProgressEvent>) -> Self {
        Self { sender }
    }
}

impl ProgressSink for ChannelProgress {
    fn emit(&self, event: ProgressEvent) {
        let _ = self.sender.send(event);
    }
}
