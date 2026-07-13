//! Safe preparation of CC Switch `skills.zip` snapshots.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tempfile::{tempdir, TempDir};
use zip::{CompressionMethod, ZipArchive};

use crate::remote::protocol::MAX_SYNC_ARTIFACT_BYTES;
use crate::AppError;

const MAX_EXTRACT_ENTRIES: usize = 10_000;

pub struct PreparedSkills {
    pub extracted_dir: TempDir,
    pub entry_count: usize,
    pub total_bytes: u64,
}

pub fn prepare_skills(zip_path: &Path) -> Result<PreparedSkills, AppError> {
    let file = fs::File::open(zip_path).map_err(|error| AppError::io(zip_path, error))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| AppError::Archive(error.to_string()))?;
    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(AppError::ArchiveTooManyEntries {
            count: archive.len(),
            max: MAX_EXTRACT_ENTRIES,
        });
    }

    let (paths, declared_total) = preflight(&mut archive)?;
    let extracted_dir = tempdir().map_err(|error| AppError::io("skills extraction", error))?;
    let mut actual_total = 0_u64;

    for (index, relative) in paths.into_iter().enumerate() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Archive(error.to_string()))?;
        let output = extracted_dir.path().join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| AppError::io(&output, error))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|error| AppError::io(&output, error))?;
        copy_entry_bounded(&mut entry, &mut file, &mut actual_total, &output)?;
        file.flush().map_err(|error| AppError::io(&output, error))?;
        file.sync_all()
            .map_err(|error| AppError::io(&output, error))?;
        apply_safe_permissions(&output, entry.unix_mode())?;
    }

    if actual_total != declared_total {
        return Err(AppError::Archive(format!(
            "declared size {declared_total} did not match extracted size {actual_total}"
        )));
    }

    Ok(PreparedSkills {
        extracted_dir,
        entry_count: archive.len(),
        total_bytes: actual_total,
    })
}

fn preflight(archive: &mut ZipArchive<fs::File>) -> Result<(Vec<PathBuf>, u64), AppError> {
    let mut paths = Vec::with_capacity(archive.len());
    let mut seen = HashSet::with_capacity(archive.len());
    let mut total = 0_u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::Archive(error.to_string()))?;
        let display_name = entry.name().to_string();
        if unsafe_zip_name(&display_name) {
            return Err(AppError::ArchiveUnsafePath { path: display_name });
        }
        let Some(path) = entry.enclosed_name() else {
            return Err(AppError::ArchiveUnsafePath { path: display_name });
        };
        if path.as_os_str().is_empty() || entry.is_symlink() {
            return Err(AppError::ArchiveUnsafePath { path: display_name });
        }
        if entry.encrypted() {
            return Err(AppError::Archive(format!(
                "encrypted ZIP entries are unsupported: {display_name}"
            )));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(AppError::UnsupportedArchiveCompression { path: display_name });
        }
        if !seen.insert(path.clone()) {
            return Err(AppError::Archive(format!(
                "duplicate ZIP entry: {}",
                path.display()
            )));
        }
        if !entry.is_dir() {
            total = total
                .checked_add(entry.size())
                .ok_or(AppError::ArchiveExtractedTooLarge {
                    size: u64::MAX,
                    max: MAX_SYNC_ARTIFACT_BYTES,
                })?;
            if total > MAX_SYNC_ARTIFACT_BYTES {
                return Err(AppError::ArchiveExtractedTooLarge {
                    size: total,
                    max: MAX_SYNC_ARTIFACT_BYTES,
                });
            }
        }
        paths.push(path);
    }
    Ok((paths, total))
}

fn unsafe_zip_name(name: &str) -> bool {
    let normalized = name.replace('\\', "/");
    let first = normalized.split('/').next().unwrap_or_default();
    normalized.starts_with('/')
        || first.as_bytes().get(1) == Some(&b':')
        || normalized.split('/').any(|component| component == "..")
}

fn copy_entry_bounded<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    total: &mut u64,
    output: &Path,
) -> Result<(), AppError> {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| AppError::io(output, error))?;
        if read == 0 {
            return Ok(());
        }
        let next = total.saturating_add(read as u64);
        if next > MAX_SYNC_ARTIFACT_BYTES {
            return Err(AppError::ArchiveExtractedTooLarge {
                size: next,
                max: MAX_SYNC_ARTIFACT_BYTES,
            });
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| AppError::io(output, error))?;
        *total = next;
    }
}

#[cfg(unix)]
fn apply_safe_permissions(path: &Path, mode: Option<u32>) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(mode) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
            .map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_safe_permissions(_path: &Path, _mode: Option<u32>) -> Result<(), AppError> {
    Ok(())
}
