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
    #[error("manifest exceeds the {max} byte limit: {size} bytes")]
    ManifestTooLarge { size: usize, max: usize },
    #[error("failed to parse manifest: {0}")]
    ManifestParse(#[source] serde_json::Error),
    #[error("manifest format is incompatible: {found}")]
    ManifestFormatIncompatible { found: String },
    #[error("manifest protocol version {found} is incompatible with local version {supported}")]
    ManifestVersionIncompatible { found: u32, supported: u32 },
    #[error("manifest is missing the database compatibility version")]
    DatabaseVersionMissing,
    #[error(
        "database compatibility version {found} is incompatible with local version {supported}"
    )]
    DatabaseVersionIncompatible { found: u32, supported: u32 },
    #[error("manifest is missing required artifact {artifact}")]
    ManifestMissingArtifact { artifact: String },
    #[error("artifact {artifact} exceeds the {max} byte limit: {size} bytes")]
    ArtifactTooLarge {
        artifact: String,
        size: u64,
        max: u64,
    },
    #[error("artifact {artifact} has an invalid SHA-256 value")]
    InvalidArtifactHash { artifact: String },
    #[error("snapshot ID mismatch: expected {expected}, found {actual}")]
    SnapshotIdMismatch { expected: String, actual: String },
    #[error("artifact {artifact} size mismatch: expected {expected}, found {actual}")]
    ArtifactSizeMismatch {
        artifact: String,
        expected: u64,
        actual: u64,
    },
    #[error("artifact {artifact} SHA-256 mismatch: expected {expected}, found {actual}")]
    ArtifactHashMismatch {
        artifact: String,
        expected: String,
        actual: String,
    },
    #[error("WebDAV {operation} failed ({reason}): {url}")]
    WebDavTransport {
        operation: &'static str,
        url: String,
        reason: &'static str,
    },
    #[error("WebDAV {operation} failed with HTTP {status}: {url}")]
    WebDavHttp {
        operation: &'static str,
        status: u16,
        url: String,
    },
    #[error("source {source_name} has no downloadable snapshot")]
    RemoteEmpty { source_name: String },
    #[error("remote artifact is missing: {artifact}")]
    RemoteArtifactMissing { artifact: String },
    #[error("response for {target} exceeds the {max} byte limit: {size} bytes")]
    ResponseTooLarge { target: String, size: u64, max: u64 },
}

impl AppError {
    pub fn io(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
