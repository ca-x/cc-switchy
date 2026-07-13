#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("home directory is unavailable")]
    HomeDirectoryUnavailable,
    #[error("no source is configured")]
    NoSourceConfigured,
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("source already exists: {0}")]
    DuplicateSource(String),
    #[error("source not found: {0}")]
    SourceNotFound(String),
    #[error("a replacement default source is required")]
    ReplacementDefaultRequired,
    #[error("failed to read or write {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse configuration: {0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("failed to serialize configuration: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),
}

impl AppError {
    pub fn io(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
