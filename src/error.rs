#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("home directory is unavailable")]
    HomeDirectoryUnavailable,
    #[error("no source is configured")]
    NoSourceConfigured,
}
