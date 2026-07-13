use std::path::PathBuf;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub home: PathBuf,
    pub app_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_file: PathBuf,
    pub lock_file: PathBuf,
    pub staging_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub cc_switch_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, AppError> {
        let home = std::env::var_os("CC_SWITCHY_TEST_HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .ok_or(AppError::HomeDirectoryUnavailable)?;
        Ok(Self::from_home(home))
    }

    pub fn from_home(home: impl AsRef<std::path::Path>) -> Self {
        let home = home.as_ref().to_path_buf();
        let app_dir = home.join(".cc-switchy");

        Self {
            config_file: app_dir.join("config.toml"),
            state_file: app_dir.join("state.json"),
            lock_file: app_dir.join("lock"),
            staging_dir: app_dir.join("staging"),
            backups_dir: app_dir.join("backups"),
            cc_switch_dir: home.join(".cc-switch"),
            home,
            app_dir,
        }
    }
}
