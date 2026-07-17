//! Validated restore transaction and process-wide sync lock.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};

use super::archive::prepare_skills;
use super::backup::{copy_tree_preserve_links, remove_path_if_exists, LocalBackup};
use super::database::prepare_database;
use crate::config::BackupConfig;
use crate::paths::AppPaths;
use crate::progress::{ProgressEvent, ProgressSink};
use crate::remote::DownloadedSnapshot;
use crate::AppError;

pub struct SyncLockGuard {
    file: fs::File,
}

impl SyncLockGuard {
    pub fn acquire(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| AppError::io(path, error))?;
        set_private_file(path)?;
        FileExt::try_lock_exclusive(&file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                AppError::SyncLocked
            } else {
                AppError::io(path, error)
            }
        })?;
        Ok(Self { file })
    }
}

impl Drop for SyncLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Debug)]
pub struct RestoreOutcome {
    pub backup_dir: Option<PathBuf>,
    pub database_path: PathBuf,
    pub skills_path: PathBuf,
}

pub(crate) struct RestoreDetails {
    pub outcome: RestoreOutcome,
    pub restored_skills: usize,
}

pub struct RestoreService {
    paths: AppPaths,
    progress: Arc<dyn ProgressSink>,
    backup_config: BackupConfig,
}

impl RestoreService {
    pub fn new(
        paths: AppPaths,
        progress: Arc<dyn ProgressSink>,
        backup_config: BackupConfig,
    ) -> Self {
        Self {
            paths,
            progress,
            backup_config,
        }
    }

    pub fn apply(
        &self,
        snapshot: DownloadedSnapshot,
        lock: &SyncLockGuard,
        source: &str,
    ) -> Result<RestoreOutcome, AppError> {
        self.apply_with_details(snapshot, lock, source)
            .map(|details| details.outcome)
    }

    pub(crate) fn apply_with_details(
        &self,
        snapshot: DownloadedSnapshot,
        _lock: &SyncLockGuard,
        source: &str,
    ) -> Result<RestoreDetails, AppError> {
        let result = self.apply_inner(&snapshot, source);
        cleanup_downloaded_files(&snapshot);
        result
    }

    fn apply_inner(
        &self,
        snapshot: &DownloadedSnapshot,
        source: &str,
    ) -> Result<RestoreDetails, AppError> {
        let skills_path = resolve_skills_path(&self.paths)?;
        let prepared_skills = prepare_skills(&snapshot.skills_zip_path)?;
        let restored_skills = count_skill_directories(prepared_skills.extracted_dir.path())?;
        let database_path = self.paths.cc_switch_dir.join("cc-switch.db");
        let prepared_database = prepare_database(
            &snapshot.db_sql_path,
            database_path.exists().then_some(database_path.as_path()),
        )?;

        let backup = if self.backup_config.enabled {
            self.progress.emit(ProgressEvent::PreparingLocalBackup);
            let backup = LocalBackup::create(
                &self.paths,
                &skills_path,
                source,
                snapshot.manifest.snapshot_id(),
            )?;
            backup.enforce_retention(self.backup_config.max_count)?;
            Some(backup)
        } else {
            None
        };

        self.progress.emit_restored_skills(restored_skills);
        if let Err(restore_error) =
            install_prepared_skills(prepared_skills.extracted_dir.path(), &skills_path)
        {
            return if let Some(backup) = &backup {
                match backup.restore_skills() {
                    Ok(()) => Err(restore_error),
                    Err(rollback_error) => Err(AppError::Rollback(format!(
                        "{restore_error}; Skills rollback failed: {rollback_error}; backup: {}",
                        backup.backup_dir.display()
                    ))),
                }
            } else {
                Err(rollback_unavailable(restore_error))
            };
        }

        self.progress.emit(ProgressEvent::ImportingDatabase);
        if let Err(database_error) = replace_database(prepared_database.file.path(), &database_path)
        {
            let Some(backup) = &backup else {
                return Err(rollback_unavailable(database_error));
            };
            let skills_rollback = backup.restore_skills();
            let database_rollback = backup.restore_database();
            if let (Ok(()), Ok(())) = (&skills_rollback, &database_rollback) {
                return Err(database_error);
            }
            return Err(AppError::Rollback(format!(
                "{database_error}; Skills rollback: {}; database rollback: {}; backup: {}",
                display_rollback(&skills_rollback),
                display_rollback(&database_rollback),
                backup.backup_dir.display()
            )));
        }

        Ok(RestoreDetails {
            outcome: RestoreOutcome {
                backup_dir: backup.map(|backup| backup.backup_dir),
                database_path,
                skills_path,
            },
            restored_skills,
        })
    }
}

fn count_skill_directories(root: &Path) -> Result<usize, AppError> {
    let mut count = 0;
    for entry in fs::read_dir(root).map_err(|error| AppError::io(root, error))? {
        let entry = entry.map_err(|error| AppError::io(root, error))?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|error| AppError::io(&path, error))?
            .is_dir()
            && path.join("SKILL.md").is_file()
        {
            count += 1;
        }
    }
    Ok(count)
}

fn rollback_unavailable(error: AppError) -> AppError {
    AppError::Restore(format!(
        "{error}; rollback unavailable because backups are disabled"
    ))
}

fn install_prepared_skills(source: &Path, target: &Path) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::Restore("Skills path has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    let staging = tempfile::Builder::new()
        .prefix(".cc-switchy-skills-")
        .tempdir_in(parent)
        .map_err(|error| AppError::io(parent, error))?;
    let prepared = staging.path().join("skills");
    copy_tree_preserve_links(source, &prepared)?;
    remove_path_if_exists(target)?;
    fs::rename(&prepared, target).map_err(|error| AppError::io(target, error))?;
    Ok(())
}

fn replace_database(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    let source_connection = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(database_error)?;
    let mut target_connection = Connection::open(target).map_err(database_error)?;
    let backup = Backup::new(&source_connection, &mut target_connection).map_err(database_error)?;
    backup.step(-1).map_err(database_error)?;
    drop(backup);
    target_connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(database_error)?;
    set_private_file(target)
}

fn resolve_skills_path(paths: &AppPaths) -> Result<PathBuf, AppError> {
    let settings_path = paths.cc_switch_dir.join("settings.json");
    if settings_path.exists() {
        let bytes =
            fs::read(&settings_path).map_err(|error| AppError::io(&settings_path, error))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            AppError::Restore(format!(
                "failed to parse {}: {error}",
                settings_path.display()
            ))
        })?;
        if value
            .get("skillStorageLocation")
            .and_then(serde_json::Value::as_str)
            == Some("unified")
        {
            return Ok(paths.home.join(".agents/skills"));
        }
    }
    Ok(paths.cc_switch_dir.join("skills"))
}

fn cleanup_downloaded_files(snapshot: &DownloadedSnapshot) {
    let _ = fs::remove_file(&snapshot.db_sql_path);
    let _ = fs::remove_file(&snapshot.skills_zip_path);
}

fn display_rollback(result: &Result<(), AppError>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(error) => error.to_string(),
    }
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
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
