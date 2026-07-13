//! Durable pre-restore backups for database and Skills rollback.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::paths::AppPaths;
use crate::AppError;

pub struct LocalBackup {
    pub backup_dir: PathBuf,
    database_backup: Option<PathBuf>,
    skills_backup: Option<PathBuf>,
    original_database: PathBuf,
    original_skills: PathBuf,
    database_existed: bool,
    skills_existed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupMetadata<'a> {
    created_at: String,
    source: &'a str,
    snapshot_id: &'a str,
    original_database: String,
    original_skills: String,
    database_existed: bool,
    skills_existed: bool,
}

impl LocalBackup {
    pub fn create(
        paths: &AppPaths,
        skills_path: &Path,
        source: &str,
        snapshot_id: &str,
    ) -> Result<Self, AppError> {
        fs::create_dir_all(&paths.backups_dir)
            .map_err(|error| AppError::io(&paths.backups_dir, error))?;
        set_private_directory(&paths.backups_dir)?;
        let backup_dir = unique_backup_dir(&paths.backups_dir)?;
        fs::create_dir(&backup_dir).map_err(|error| AppError::io(&backup_dir, error))?;
        set_private_directory(&backup_dir)?;

        let original_database = paths.cc_switch_dir.join("cc-switch.db");
        let database_existed = original_database.is_file();
        let database_backup = if database_existed {
            let target = backup_dir.join("cc-switch.db");
            backup_database(&original_database, &target)?;
            Some(target)
        } else {
            None
        };

        let skills_existed = skills_path.exists();
        let skills_backup = if skills_existed {
            let target = backup_dir.join("skills");
            copy_tree_preserve_links(skills_path, &target)?;
            Some(target)
        } else {
            None
        };

        let metadata = BackupMetadata {
            created_at: Utc::now().to_rfc3339(),
            source,
            snapshot_id,
            original_database: original_database.display().to_string(),
            original_skills: skills_path.display().to_string(),
            database_existed,
            skills_existed,
        };
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|error| AppError::Restore(error.to_string()))?;
        write_new_file(&backup_dir.join("metadata.json"), &metadata_bytes)?;

        Ok(Self {
            backup_dir,
            database_backup,
            skills_backup,
            original_database,
            original_skills: skills_path.to_path_buf(),
            database_existed,
            skills_existed,
        })
    }

    pub fn restore_skills(&self) -> Result<(), AppError> {
        remove_path_if_exists(&self.original_skills)?;
        if self.skills_existed {
            let backup = self.skills_backup.as_ref().ok_or_else(|| {
                AppError::Rollback("durable Skills backup is missing".to_string())
            })?;
            copy_tree_preserve_links(backup, &self.original_skills)?;
        }
        Ok(())
    }

    pub fn restore_database(&self) -> Result<(), AppError> {
        if self.database_existed {
            let backup = self.database_backup.as_ref().ok_or_else(|| {
                AppError::Rollback("durable database backup is missing".to_string())
            })?;
            restore_database(backup, &self.original_database)
        } else {
            remove_path_if_exists(&self.original_database)
        }
    }
}

pub(crate) fn copy_tree_preserve_links(source: &Path, target: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| AppError::io(source, error))?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Restore(format!(
            "Skills root must not be a symbolic link: {}",
            source.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(AppError::Restore(format!(
            "Skills root is not a directory: {}",
            source.display()
        )));
    }
    copy_directory(source, target)
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::io(target, error))?;
    if let Ok(metadata) = fs::metadata(source) {
        let _ = fs::set_permissions(target, metadata.permissions());
    }
    for entry in fs::read_dir(source).map_err(|error| AppError::io(source, error))? {
        let entry = entry.map_err(|error| AppError::io(source, error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::io(&source_path, error))?;
        if metadata.file_type().is_symlink() {
            copy_symlink(&source_path, &target_path)?;
        } else if metadata.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| AppError::io(&target_path, error))?;
            fs::set_permissions(&target_path, metadata.permissions())
                .map_err(|error| AppError::io(&target_path, error))?;
            fs::File::open(&target_path)
                .and_then(|file| file.sync_all())
                .map_err(|error| AppError::io(&target_path, error))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::symlink;

    let link = fs::read_link(source).map_err(|error| AppError::io(source, error))?;
    symlink(link, target).map_err(|error| AppError::io(target, error))
}

#[cfg(windows)]
fn copy_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let link = fs::read_link(source).map_err(|error| AppError::io(source, error))?;
    if source.is_dir() {
        symlink_dir(link, target).map_err(|error| AppError::io(target, error))
    } else {
        symlink_file(link, target).map_err(|error| AppError::io(target, error))
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_symlink(source: &Path, _target: &Path) -> Result<(), AppError> {
    Err(AppError::Restore(format!(
        "symbolic links are unsupported on this platform: {}",
        source.display()
    )))
}

fn backup_database(source: &Path, target: &Path) -> Result<(), AppError> {
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

fn restore_database(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    let source_connection = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(database_error)?;
    let mut target_connection = Connection::open(target).map_err(database_error)?;
    let backup = Backup::new(&source_connection, &mut target_connection).map_err(database_error)?;
    backup.step(-1).map(|_| ()).map_err(database_error)
}

fn unique_backup_dir(root: &Path) -> Result<PathBuf, AppError> {
    let base = Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    for suffix in 0..1000_u16 {
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        let candidate = root.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::Restore(
        "could not allocate a unique backup directory".to_string(),
    ))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| AppError::io(path, error))?;
    file.write_all(bytes)
        .map_err(|error| AppError::io(path, error))?;
    file.sync_all().map_err(|error| AppError::io(path, error))?;
    set_private_file(path)
}

pub(crate) fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| AppError::io(path, error))
    } else {
        fs::remove_file(path).map_err(|error| AppError::io(path, error))
    }
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), AppError> {
    Ok(())
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

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
