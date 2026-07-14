use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::progress::ProgressEvent;
use crate::{Language, MessageArgs, MessageKey, Translator};

#[derive(Debug, Clone)]
pub struct ActivityEntry {
    pub status: ActivityStatus,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityStatus {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct ProgressModel {
    pub active: bool,
    pub stage: String,
    pub elapsed: Duration,
    pub downloads: HashMap<String, (u64, u64)>,
    pub log: VecDeque<ActivityEntry>,
    pub failed_agents: Vec<String>,
    pub retry_available: bool,
}

impl ProgressModel {
    pub const MAX_LOG_ENTRIES: usize = 200;

    pub fn apply(&mut self, event: ProgressEvent, elapsed: Duration, language: Language) {
        self.elapsed = elapsed;
        let translator = Translator::new(language);
        let mut args = MessageArgs::default();
        match event {
            ProgressEvent::Locking => {
                self.set_stage(&translator.text(MessageKey::ProgressLocking, &args));
            }
            ProgressEvent::Connecting { source } => {
                args.0.insert("source", source);
                self.set_stage(&translator.text(MessageKey::ProgressConnecting, &args));
            }
            ProgressEvent::FetchingManifest => {
                self.set_stage(&translator.text(MessageKey::ProgressFetchingManifest, &args));
            }
            ProgressEvent::ValidatingManifest => {
                self.set_stage(&translator.text(MessageKey::ProgressValidatingManifest, &args));
            }
            ProgressEvent::Downloading {
                artifact,
                downloaded,
                total,
            } => {
                self.active = true;
                args.0.insert("artifact", artifact.clone());
                self.stage = translator.text(MessageKey::ProgressDownloading, &args);
                self.downloads.insert(artifact, (downloaded, total));
            }
            ProgressEvent::Verifying { artifact } => {
                args.0.insert("artifact", artifact);
                self.set_stage(&translator.text(MessageKey::ProgressVerifying, &args));
            }
            ProgressEvent::PreparingLocalBackup => {
                self.set_stage(&translator.text(MessageKey::ProgressPreparingBackup, &args));
            }
            ProgressEvent::RestoringSkills => {
                self.set_stage(&translator.text(MessageKey::ProgressRestoringSkills, &args));
            }
            ProgressEvent::ImportingDatabase => {
                self.set_stage(&translator.text(MessageKey::ProgressImportingDatabase, &args));
            }
            ProgressEvent::ApplyingProvider { agent } => {
                self.set_stage(&format!(
                    "{} · {agent}",
                    translator.text(MessageKey::ProgressApplyingProvider, &args)
                ));
            }
            ProgressEvent::ApplyingMcp { agent } => {
                self.set_stage(&format!(
                    "{} · {agent}",
                    translator.text(MessageKey::ProgressApplyingMcp, &args)
                ));
            }
            ProgressEvent::ApplyingSkills {
                agent,
                completed,
                total,
            } => self.set_stage(&format!(
                "{} · {agent} {completed}/{total}",
                translator.text(MessageKey::ProgressApplyingSkills, &args)
            )),
            ProgressEvent::Retrying {
                operation,
                attempt,
                max_attempts,
            } => self.push(
                ActivityStatus::Warning,
                format!(
                    "{} {attempt}/{max_attempts} · {operation}",
                    translator.text(MessageKey::ProgressRetrying, &args)
                ),
            ),
            ProgressEvent::Warning {
                stage,
                agent,
                detail,
                ..
            } => {
                if let Some(agent) = &agent {
                    if !self.failed_agents.contains(agent) {
                        self.failed_agents.push(agent.clone());
                    }
                }
                self.retry_available = true;
                self.push(
                    ActivityStatus::Warning,
                    format!(
                        "{stage}{} · {detail}",
                        agent.map(|a| format!("/{a}")).unwrap_or_default()
                    ),
                );
            }
            ProgressEvent::Completed {
                duration_ms,
                snapshot_id,
            } => {
                self.active = false;
                self.stage = translator.text(MessageKey::ProgressCompleted, &args);
                self.elapsed = Duration::from_millis(duration_ms as u64);
                args.0
                    .insert("snapshot", short_id(&snapshot_id).to_string());
                self.push(
                    ActivityStatus::Success,
                    translator.text(MessageKey::ActivitySnapshot, &args),
                );
            }
            ProgressEvent::Failed { stage, detail, .. } => {
                self.active = false;
                self.retry_available = true;
                self.stage = format!(
                    "{} · {stage}",
                    translator.text(MessageKey::ErrorPrefix, &args)
                );
                self.push(ActivityStatus::Error, format!("{stage} · {detail}"));
            }
        }
    }

    pub(crate) fn apply_skill(
        &mut self,
        agent: &str,
        skill: &str,
        completed: usize,
        total: usize,
        elapsed: Duration,
        language: Language,
    ) {
        self.elapsed = elapsed;
        let label = Translator::new(language)
            .text(MessageKey::ProgressApplyingSkills, &MessageArgs::default());
        self.set_stage(&format!("{label} · {agent} · {skill} {completed}/{total}"));
    }

    pub fn push(&mut self, status: ActivityStatus, text: String) {
        self.log.push_back(ActivityEntry { status, text });
        while self.log.len() > Self::MAX_LOG_ENTRIES {
            self.log.pop_front();
        }
    }

    fn set_stage(&mut self, stage: &str) {
        self.active = true;
        self.stage = stage.to_string();
        self.push(ActivityStatus::Info, stage.to_string());
    }
}

fn short_id(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}
