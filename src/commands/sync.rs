use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    AgentPaths, AgentRepository, DeviceSettings, McpProjector, ProjectionReport, ProviderProjector,
    SkillProjector,
};
use crate::config::{ConfigStore, SourceCatalog};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::remote::RemoteClient;
use crate::restore::{RestoreService, SyncLockGuard};
use crate::{AppError, AppPaths, Language, MessageArgs, MessageKey, Translator};

pub struct SyncRequest {
    pub source_name: Option<String>,
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub source_name: String,
    pub snapshot_id: String,
    pub backup_dir: PathBuf,
    pub projection: ProjectionReport,
    pub duration: Duration,
}

pub struct SyncService {
    pub paths: AppPaths,
    pub catalog: SourceCatalog,
    pub progress: Arc<dyn ProgressSink>,
    pub cancellation: CancellationToken,
}

impl SyncService {
    pub async fn run(&mut self, request: SyncRequest) -> Result<SyncOutcome, AppError> {
        let started = Instant::now();
        let result = self.run_inner(request, started).await;
        if let Err(error) = &result {
            self.progress.emit(ProgressEvent::Failed {
                stage: "sync".to_string(),
                message_key: MessageKey::UnexpectedError,
                detail: error.to_string(),
                retryable: matches!(
                    error,
                    AppError::SyncLocked
                        | AppError::WebDavTransport { .. }
                        | AppError::S3Transport { .. }
                ),
            });
        }
        result
    }

    async fn run_inner(
        &mut self,
        request: SyncRequest,
        started: Instant,
    ) -> Result<SyncOutcome, AppError> {
        let source = self
            .catalog
            .resolve(request.source_name.as_deref())?
            .clone();
        let source_name = source.name.clone();
        self.progress.emit(ProgressEvent::Locking);
        let lock = SyncLockGuard::acquire(&self.paths.lock_file)?;
        ensure_not_cancelled(&self.cancellation)?;

        fs::create_dir_all(&self.paths.staging_dir)
            .map_err(|error| AppError::io(&self.paths.staging_dir, error))?;
        let staging = tempfile::Builder::new()
            .prefix("sync-")
            .tempdir_in(&self.paths.staging_dir)
            .map_err(|error| AppError::io(&self.paths.staging_dir, error))?;
        let remote = RemoteClient::new(source, Arc::clone(&self.progress))?;
        let snapshot = tokio::select! {
            result = remote.fetch_snapshot(staging.path()) => result?,
            () = self.cancellation.cancelled() => return Err(AppError::Cancelled),
        };
        ensure_not_cancelled(&self.cancellation)?;
        let snapshot_id = snapshot.manifest.snapshot_id().to_string();

        let restore = RestoreService::new(self.paths.clone(), Arc::clone(&self.progress));
        let restored = restore.apply(snapshot, &lock, &source_name)?;

        let settings_path = self.paths.cc_switch_dir.join("settings.json");
        let mut settings = DeviceSettings::load(&settings_path)?;
        let agent_paths = AgentPaths::from_settings(&self.paths.home, &settings);
        let mut repo = AgentRepository::open(&restored.database_path)?;
        let mut projection = {
            let mut projector = ProviderProjector::new(
                &mut repo,
                &mut settings,
                &agent_paths,
                Arc::clone(&self.progress),
            );
            projector.project_all()
        };
        projection.merge(
            McpProjector::new(&repo, &agent_paths, Arc::clone(&self.progress)).project_all(),
        );
        projection.merge(
            SkillProjector::new(&repo, &settings, &agent_paths, Arc::clone(&self.progress))
                .project_all(),
        );

        let duration = started.elapsed();
        persist_state(
            &self.paths.state_file,
            &source_name,
            &snapshot_id,
            &restored.backup_dir,
            duration,
            projection.warnings.len(),
        )?;
        self.progress.emit(ProgressEvent::Completed {
            duration_ms: duration.as_millis(),
            snapshot_id: snapshot_id.clone(),
        });

        Ok(SyncOutcome {
            source_name,
            snapshot_id,
            backup_dir: restored.backup_dir,
            projection,
            duration,
        })
    }
}

pub async fn run_cli(
    paths: AppPaths,
    source_name: Option<String>,
    translator: &Translator,
    cancellation: CancellationToken,
) -> Result<SyncOutcome, AppError> {
    let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    let progress: Arc<dyn ProgressSink> = Arc::new(CliProgress::new(
        translator.language(),
        std::io::stderr().is_terminal(),
    ));
    let mut service = SyncService {
        paths,
        catalog,
        progress,
        cancellation,
    };
    let outcome = service.run(SyncRequest { source_name }).await?;
    println!("{}", render_outcome(translator, &outcome));
    Ok(outcome)
}

pub struct CliProgress {
    language: Language,
    tty: bool,
    downloads: Mutex<HashMap<String, u64>>,
}

impl CliProgress {
    pub fn new(language: Language, tty: bool) -> Self {
        Self {
            language,
            tty,
            downloads: Mutex::new(HashMap::new()),
        }
    }

    fn should_render_download(&self, artifact: &str, downloaded: u64, total: u64) -> bool {
        let percent = downloaded
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(100);
        let mut downloads = self.downloads.lock().expect("download progress lock");
        let previous = downloads.get(artifact).copied();
        let render = downloaded == 0
            || downloaded >= total
            || previous.is_none_or(|previous| percent.saturating_sub(previous) >= 10);
        if render {
            downloads.insert(artifact.to_string(), percent);
        }
        render
    }

    fn render(&self, event: &ProgressEvent) -> Option<String> {
        let translator = Translator::new(self.language);
        let mut args = MessageArgs::default();
        let line = match event {
            ProgressEvent::Locking => translator.text(MessageKey::ProgressLocking, &args),
            ProgressEvent::Connecting { source } => {
                args.0.insert("source", source.clone());
                translator.text(MessageKey::ProgressConnecting, &args)
            }
            ProgressEvent::FetchingManifest => {
                translator.text(MessageKey::ProgressFetchingManifest, &args)
            }
            ProgressEvent::ValidatingManifest => {
                translator.text(MessageKey::ProgressValidatingManifest, &args)
            }
            ProgressEvent::Downloading {
                artifact,
                downloaded,
                total,
            } => {
                if !self.should_render_download(artifact, *downloaded, *total) {
                    return None;
                }
                args.0.insert("artifact", artifact.clone());
                let label = translator.text(MessageKey::ProgressDownloading, &args);
                let percent = downloaded
                    .saturating_mul(100)
                    .checked_div(*total)
                    .unwrap_or(100);
                format!("{label}: {downloaded}/{total} bytes ({percent}%)")
            }
            ProgressEvent::Verifying { artifact } => {
                args.0.insert("artifact", artifact.clone());
                translator.text(MessageKey::ProgressVerifying, &args)
            }
            ProgressEvent::PreparingLocalBackup => {
                translator.text(MessageKey::ProgressPreparingBackup, &args)
            }
            ProgressEvent::RestoringSkills => {
                translator.text(MessageKey::ProgressRestoringSkills, &args)
            }
            ProgressEvent::ImportingDatabase => {
                translator.text(MessageKey::ProgressImportingDatabase, &args)
            }
            ProgressEvent::ApplyingProvider { agent } => format!(
                "{}: {agent}",
                translator.text(MessageKey::ProgressApplyingProvider, &args)
            ),
            ProgressEvent::ApplyingMcp { agent } => format!(
                "{}: {agent}",
                translator.text(MessageKey::ProgressApplyingMcp, &args)
            ),
            ProgressEvent::ApplyingSkills {
                agent,
                completed,
                total,
            } => format!(
                "{}: {agent} {completed}/{total}",
                translator.text(MessageKey::ProgressApplyingSkills, &args)
            ),
            ProgressEvent::Retrying {
                operation,
                attempt,
                max_attempts,
            } => format!(
                "{}: {operation} {attempt}/{max_attempts}",
                translator.text(MessageKey::ProgressRetrying, &args)
            ),
            ProgressEvent::Warning {
                stage,
                agent,
                detail,
                ..
            } => format!(
                "{}: {stage}{}: {detail}",
                translator.text(MessageKey::ProgressWarning, &args),
                agent
                    .as_deref()
                    .map(|agent| format!("/{agent}"))
                    .unwrap_or_default()
            ),
            ProgressEvent::Completed { .. } => {
                translator.text(MessageKey::ProgressCompleted, &args)
            }
            ProgressEvent::Failed { stage, detail, .. } => format!(
                "{}: {stage}: {detail}",
                translator.text(MessageKey::ErrorPrefix, &args)
            ),
        };
        Some(line)
    }
}

impl ProgressSink for CliProgress {
    fn emit(&self, event: ProgressEvent) {
        let Some(line) = self.render(&event) else {
            return;
        };
        if self.tty {
            let terminal = matches!(
                event,
                ProgressEvent::Completed { .. }
                    | ProgressEvent::Failed { .. }
                    | ProgressEvent::Warning { .. }
            );
            if terminal {
                eprintln!("\r\x1b[2K{line}");
            } else {
                eprint!("\r\x1b[2K{line}");
                let _ = std::io::stderr().flush();
            }
        } else {
            eprintln!("{line}");
        }
    }
}

fn render_outcome(translator: &Translator, outcome: &SyncOutcome) -> String {
    let mut args = MessageArgs::default();
    args.0.insert("source", outcome.source_name.clone());
    args.0.insert("snapshot", outcome.snapshot_id.clone());
    args.0.insert(
        "duration",
        format!("{:.2}s", outcome.duration.as_secs_f64()),
    );
    args.0.insert(
        "applied",
        outcome.projection.applied_agents.len().to_string(),
    );
    args.0
        .insert("warnings", outcome.projection.warnings.len().to_string());
    args.0
        .insert("backup", outcome.backup_dir.display().to_string());
    translator.text(MessageKey::SyncSummary, &args)
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), AppError> {
    if cancellation.is_cancelled() {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LastSyncState<'a> {
    source: &'a str,
    snapshot_id: &'a str,
    completed_at: String,
    duration_ms: u128,
    backup_dir: String,
    warnings: usize,
}

fn persist_state(
    path: &Path,
    source: &str,
    snapshot_id: &str,
    backup_dir: &Path,
    duration: Duration,
    warnings: usize,
) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Restore("state path has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    let state = LastSyncState {
        source,
        snapshot_id,
        completed_at: Utc::now().to_rfc3339(),
        duration_ms: duration.as_millis(),
        backup_dir: backup_dir.display().to_string(),
        warnings,
    };
    let mut root = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    root.insert(
        "lastSync".to_string(),
        serde_json::to_value(&state).map_err(|error| AppError::Restore(error.to_string()))?,
    );
    let mut bytes =
        serde_json::to_vec_pretty(&root).map_err(|error| AppError::Restore(error.to_string()))?;
    bytes.push(b'\n');
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|error| AppError::io(parent, error))?;
    temporary
        .write_all(&bytes)
        .map_err(|error| AppError::io(temporary.path(), error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| AppError::io(temporary.path(), error))?;
    temporary
        .persist(path)
        .map_err(|error| AppError::io(path, error.error))?;
    set_private_file(path)
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), AppError> {
    Ok(())
}
