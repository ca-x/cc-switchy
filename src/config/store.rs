use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use super::AppConfig;
use crate::AppError;

pub struct ConfigStore {
    path: PathBuf,
    #[cfg(test)]
    fail_after_persist: bool,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            #[cfg(test)]
            fail_after_persist: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn failing_after_persist(path: PathBuf) -> Self {
        Self {
            path,
            fail_after_persist: true,
        }
    }

    pub fn load(&self) -> Result<AppConfig, AppError> {
        if !self.exists() {
            return Ok(AppConfig::default());
        }

        let mut content = String::new();
        fs::File::open(&self.path)
            .map_err(|error| AppError::io(&self.path, error))?
            .read_to_string(&mut content)
            .map_err(|error| AppError::io(&self.path, error))?;
        let mut config: AppConfig = toml::from_str(&content)?;
        config.normalize();
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), AppError> {
        let mut config = config.clone();
        config.normalize();
        config.validate()?;
        let content = toml::to_string_pretty(&config)?;
        let parent = self.path.parent().ok_or_else(|| {
            AppError::InvalidConfig("configuration path has no parent directory".to_string())
        })?;
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        set_private_dir_permissions(parent)?;

        if self.path.exists() {
            let backup = backup_path(&self.path);
            fs::copy(&self.path, &backup).map_err(|error| AppError::io(&backup, error))?;
            set_private_file_permissions(&backup)?;
        }

        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|error| AppError::io(parent, error))?;
        set_private_file_permissions(temporary.path())?;
        temporary
            .write_all(content.as_bytes())
            .map_err(|error| AppError::io(temporary.path(), error))?;
        temporary
            .flush()
            .map_err(|error| AppError::io(temporary.path(), error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| AppError::io(temporary.path(), error))?;
        temporary.persist(&self.path).map_err(|error| {
            let source = error.error;
            AppError::io(&self.path, source)
        })?;
        #[cfg(test)]
        if self.fail_after_persist {
            return Err(AppError::io(
                &self.path,
                std::io::Error::other("injected post-persist failure"),
            ));
        }
        set_private_file_permissions(&self.path)?;
        sync_directory(parent)?;
        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("toml.bak")
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), AppError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), AppError> {
    Ok(())
}
