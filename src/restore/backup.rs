//! Durable pre-restore backups for database and Skills rollback.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMetadata {
    created_at: String,
    source: String,
    snapshot_id: String,
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
        require_real_backup_root(&paths.backups_dir)?;
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
            source: source.to_string(),
            snapshot_id: snapshot_id.to_string(),
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

    pub fn enforce_retention(&self, max_count: usize) -> Result<(), AppError> {
        let root = self.backup_dir.parent().ok_or_else(|| {
            AppError::Restore("backup directory has no parent directory".to_string())
        })?;
        prune_recognized_backups(root, &self.backup_dir, max_count)
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

fn require_real_backup_root(path: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::Restore(format!(
            "backup root must be a real directory: {}",
            path.display()
        )));
    }
    Ok(())
}

struct RecognizedBackup {
    path: PathBuf,
    name: String,
    created_at: DateTime<chrono::FixedOffset>,
}

fn prune_recognized_backups(
    root: &Path,
    current_backup: &Path,
    max_count: usize,
) -> Result<(), AppError> {
    if max_count == 0 {
        return Ok(());
    }

    let mut recognized = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| AppError::io(root, error))? {
        let entry = entry.map_err(|error| AppError::io(root, error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| AppError::io(&path, error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_backup_directory_name(&name) {
            continue;
        }

        let metadata_path = path.join("metadata.json");
        let file_metadata = match fs::symlink_metadata(&metadata_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(AppError::io(&metadata_path, error)),
        };
        if file_metadata.file_type().is_symlink() || !file_metadata.is_file() {
            continue;
        }
        let bytes =
            fs::read(&metadata_path).map_err(|error| AppError::io(&metadata_path, error))?;
        let Ok(metadata) = serde_json::from_slice::<BackupMetadata>(&bytes) else {
            continue;
        };
        let Ok(created_at) = DateTime::parse_from_rfc3339(&metadata.created_at) else {
            continue;
        };
        recognized.push(RecognizedBackup {
            path,
            name,
            created_at,
        });
    }

    let remove_count = recognized.len().saturating_sub(max_count);
    if remove_count == 0 {
        return Ok(());
    }
    recognized.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    for backup in recognized
        .into_iter()
        .filter(|backup| backup.path != current_backup)
        .take(remove_count)
    {
        fs::remove_dir_all(&backup.path).map_err(|error| AppError::io(&backup.path, error))?;
    }
    Ok(())
}

fn is_backup_directory_name(name: &str) -> bool {
    const TIMESTAMP_LENGTH: usize = 26;

    if !name.is_ascii() || name.len() < TIMESTAMP_LENGTH {
        return false;
    }
    let (timestamp, suffix) = name.split_at(TIMESTAMP_LENGTH);
    if NaiveDateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%S%.9fZ").is_err() {
        return false;
    }
    if suffix.is_empty() {
        return true;
    }
    suffix
        .strip_prefix('-')
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (1..1000).contains(&value))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn retention_keeps_current_and_newest_recognized_backups() {
        let root = TempDir::new().expect("backup root");
        let oldest = completed_backup(
            root.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let middle = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T02:00:00Z",
        );
        let current = completed_backup(
            root.path(),
            "20260714T030000.000000000Z",
            "2026-07-14T03:00:00Z",
        );

        prune_recognized_backups(root.path(), &current, 2).expect("prune backups");

        assert!(!oldest.exists());
        assert!(middle.is_dir());
        assert!(current.is_dir());
    }

    #[test]
    fn retention_zero_leaves_every_backup_untouched() {
        let root = TempDir::new().expect("backup root");
        let oldest = completed_backup(
            root.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let current = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T02:00:00Z",
        );

        prune_recognized_backups(root.path(), &current, 0).expect("unlimited retention");

        assert!(oldest.is_dir());
        assert!(current.is_dir());
    }

    #[test]
    fn retention_orders_by_metadata_time_then_directory_name() {
        let root = TempDir::new().expect("backup root");
        let later_named_but_older = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let earlier_named_but_newer = completed_backup(
            root.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T02:00:00Z",
        );
        let current = completed_backup(
            root.path(),
            "20260714T030000.000000000Z-1",
            "2026-07-14T03:00:00Z",
        );

        prune_recognized_backups(root.path(), &current, 2).expect("prune backups");

        assert!(!later_named_but_older.exists());
        assert!(earlier_named_but_newer.is_dir());
        assert!(current.is_dir());
    }

    #[test]
    fn retention_preserves_unknown_entries_files_and_invalid_metadata() {
        let root = TempDir::new().expect("backup root");
        let recognized = completed_backup(
            root.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let current = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T02:00:00Z",
        );
        let malformed_name = root.path().join("manual-backup");
        fs::create_dir(&malformed_name).expect("malformed directory");
        let invalid_metadata = root.path().join("20260714T040000.000000000Z");
        fs::create_dir(&invalid_metadata).expect("invalid metadata directory");
        fs::write(invalid_metadata.join("metadata.json"), b"not json").expect("invalid metadata");
        let ordinary_file = root.path().join("20260714T050000.000000000Z");
        fs::write(&ordinary_file, b"not a directory").expect("ordinary file");

        prune_recognized_backups(root.path(), &current, 1).expect("prune backups");

        assert!(!recognized.exists());
        assert!(current.is_dir());
        assert!(malformed_name.is_dir());
        assert!(invalid_metadata.is_dir());
        assert!(ordinary_file.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn retention_preserves_symbolic_link_entries() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().expect("backup root");
        let external = TempDir::new().expect("external backup");
        completed_backup(
            external.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let link = root.path().join("20260714T010000.000000000Z");
        symlink(external.path().join("20260714T010000.000000000Z"), &link).expect("backup symlink");
        let current = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T02:00:00Z",
        );

        prune_recognized_backups(root.path(), &current, 1).expect("prune backups");

        assert!(fs::symlink_metadata(&link)
            .expect("link metadata")
            .file_type()
            .is_symlink());
        assert!(current.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn retention_reports_deletion_errors() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("backup root");
        let oldest = completed_backup(
            root.path(),
            "20260714T010000.000000000Z",
            "2026-07-14T01:00:00Z",
        );
        let current = completed_backup(
            root.path(),
            "20260714T020000.000000000Z",
            "2026-07-14T02:00:00Z",
        );
        fs::set_permissions(&oldest, fs::Permissions::from_mode(0o000))
            .expect("block deletion traversal");

        let error = prune_recognized_backups(root.path(), &current, 1)
            .expect_err("unreadable backup must stop cleanup");

        assert!(error.to_string().contains(&oldest.display().to_string()));
        assert!(current.is_dir());
        fs::set_permissions(&oldest, fs::Permissions::from_mode(0o700))
            .expect("restore permissions");
    }

    #[cfg(unix)]
    #[test]
    fn backup_creation_rejects_a_symbolic_link_root() {
        use std::os::unix::fs::symlink;

        let home = TempDir::new().expect("home");
        let external = TempDir::new().expect("external root");
        let paths = AppPaths::from_home(home.path());
        fs::create_dir_all(&paths.app_dir).expect("application directory");
        symlink(external.path(), &paths.backups_dir).expect("backup root symlink");

        let error = match LocalBackup::create(
            &paths,
            &paths.cc_switch_dir.join("skills"),
            "home",
            "snapshot",
        ) {
            Err(error) => error,
            Ok(_) => panic!("symbolic link root was accepted"),
        };

        assert!(error
            .to_string()
            .contains("backup root must be a real directory"));
        assert_eq!(
            fs::read_dir(external.path())
                .expect("external directory")
                .count(),
            0
        );
    }

    fn completed_backup(root: &Path, name: &str, created_at: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir(&path).expect("backup directory");
        let metadata = serde_json::json!({
            "createdAt": created_at,
            "source": "home",
            "snapshotId": "snapshot",
            "originalDatabase": "/tmp/cc-switch.db",
            "originalSkills": "/tmp/skills",
            "databaseExisted": true,
            "skillsExisted": true
        });
        fs::write(
            path.join("metadata.json"),
            serde_json::to_vec_pretty(&metadata).expect("metadata bytes"),
        )
        .expect("metadata file");
        path
    }
}
