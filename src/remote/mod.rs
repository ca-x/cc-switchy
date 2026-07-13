use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::config::{SourceConfig, SourceKind};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::AppError;

pub mod protocol;
pub mod s3;
pub mod webdav;

pub enum RemoteClient {
    WebDav(webdav::WebDavClient),
    S3(s3::S3Client),
}

impl RemoteClient {
    pub fn new(
        source: SourceConfig,
        progress: std::sync::Arc<dyn ProgressSink>,
    ) -> Result<Self, AppError> {
        let client = reqwest::Client::builder().build().map_err(|error| {
            AppError::InvalidConfig(format!("HTTP client setup failed: {error}"))
        })?;
        match &source.kind {
            SourceKind::WebDav { .. } => {
                webdav::WebDavClient::new(source, client, progress).map(Self::WebDav)
            }
            SourceKind::S3 { .. } => s3::S3Client::new(source, client, progress).map(Self::S3),
        }
    }

    pub async fn fetch_snapshot(&self, staging: &Path) -> Result<DownloadedSnapshot, AppError> {
        match self {
            Self::WebDav(client) => client.fetch_snapshot(staging).await,
            Self::S3(client) => client.fetch_snapshot(staging).await,
        }
    }

    pub async fn test_connection(&self) -> Result<Option<protocol::ValidatedManifest>, AppError> {
        match self {
            Self::WebDav(client) => client.test_connection().await,
            Self::S3(client) => client.test_connection().await,
        }
    }
}

#[derive(Debug)]
pub struct DownloadedSnapshot {
    pub manifest: protocol::ValidatedManifest,
    pub manifest_bytes: Vec<u8>,
    pub layout: protocol::RemoteLayout,
    pub db_sql_path: std::path::PathBuf,
    pub skills_zip_path: std::path::PathBuf,
}

pub(crate) async fn read_limited_response<F>(
    response: reqwest::Response,
    target: &str,
    max: u64,
    read_error: F,
) -> Result<Vec<u8>, AppError>
where
    F: Fn(&reqwest::Error) -> AppError,
{
    enforce_content_length(response.content_length(), target, max)?;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| read_error(&error))?;
        let size = bytes.len().saturating_add(chunk.len()) as u64;
        if size > max {
            return Err(AppError::ResponseTooLarge {
                target: target.to_string(),
                size,
                max,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(crate) async fn write_verified_artifact<F>(
    response: reqwest::Response,
    staging: &Path,
    artifact: &str,
    meta: &protocol::ArtifactMeta,
    progress: &dyn ProgressSink,
    read_error: F,
) -> Result<PathBuf, AppError>
where
    F: Fn(&reqwest::Error) -> AppError,
{
    enforce_content_length(
        response.content_length(),
        artifact,
        protocol::MAX_SYNC_ARTIFACT_BYTES,
    )?;
    let target = staging.join(artifact);
    let result =
        write_verified_artifact_inner(response, &target, artifact, meta, progress, read_error)
            .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&target).await;
    }
    result.map(|()| target)
}

async fn write_verified_artifact_inner<F>(
    response: reqwest::Response,
    target: &Path,
    artifact: &str,
    meta: &protocol::ArtifactMeta,
    progress: &dyn ProgressSink,
    read_error: F,
) -> Result<(), AppError>
where
    F: Fn(&reqwest::Error) -> AppError,
{
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .await
        .map_err(|error| AppError::io(target, error))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    let mut hasher = Sha256::new();
    progress.emit(ProgressEvent::Downloading {
        artifact: artifact.to_string(),
        downloaded,
        total: meta.size,
    });

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| read_error(&error))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > protocol::MAX_SYNC_ARTIFACT_BYTES {
            return Err(AppError::ResponseTooLarge {
                target: artifact.to_string(),
                size: downloaded,
                max: protocol::MAX_SYNC_ARTIFACT_BYTES,
            });
        }
        if downloaded > meta.size {
            return Err(AppError::ArtifactSizeMismatch {
                artifact: artifact.to_string(),
                expected: meta.size,
                actual: downloaded,
            });
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| AppError::io(target, error))?;
        hasher.update(&chunk);
        progress.emit(ProgressEvent::Downloading {
            artifact: artifact.to_string(),
            downloaded,
            total: meta.size,
        });
    }
    file.flush()
        .await
        .map_err(|error| AppError::io(target, error))?;
    file.sync_all()
        .await
        .map_err(|error| AppError::io(target, error))?;

    if downloaded != meta.size {
        return Err(AppError::ArtifactSizeMismatch {
            artifact: artifact.to_string(),
            expected: meta.size,
            actual: downloaded,
        });
    }
    progress.emit(ProgressEvent::Verifying {
        artifact: artifact.to_string(),
    });
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(&meta.sha256) {
        return Err(AppError::ArtifactHashMismatch {
            artifact: artifact.to_string(),
            expected: meta.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

fn enforce_content_length(length: Option<u64>, target: &str, max: u64) -> Result<(), AppError> {
    if let Some(size) = length {
        if size > max {
            return Err(AppError::ResponseTooLarge {
                target: target.to_string(),
                size,
                max,
            });
        }
    }
    Ok(())
}
