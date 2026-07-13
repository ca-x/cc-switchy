//! Read-only WebDAV transport adapted from CC Switch.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Response, StatusCode, Url};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::protocol::{
    RemoteLayout, SyncManifest, ValidatedManifest, MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES,
    REMOTE_DB_SQL, REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};
use super::DownloadedSnapshot;
use crate::config::{SourceConfig, SourceKind, WebDavConfig};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::AppError;

const MAX_ATTEMPTS: u8 = 3;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);

pub struct WebDavClient {
    source_name: String,
    remote_root: String,
    profile: String,
    config: WebDavConfig,
    client: reqwest::Client,
    progress: Arc<dyn ProgressSink>,
}

impl WebDavClient {
    pub fn new(
        mut source: SourceConfig,
        client: reqwest::Client,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Self, AppError> {
        source.normalize();
        source.validate()?;
        let config = match source.kind {
            SourceKind::WebDav { webdav } => webdav,
            SourceKind::S3 { .. } => {
                return Err(AppError::InvalidConfig(
                    "a WebDAV source is required".to_string(),
                ));
            }
        };
        Ok(Self {
            source_name: source.name,
            remote_root: source.remote_root,
            profile: source.profile,
            config,
            client,
            progress,
        })
    }

    pub async fn test_connection(&self) -> Result<Option<ValidatedManifest>, AppError> {
        self.progress.emit(ProgressEvent::Connecting {
            source: self.source_name.clone(),
        });
        Ok(self.find_manifest().await?.map(|snapshot| snapshot.0))
    }

    pub async fn fetch_snapshot(&self, staging: &Path) -> Result<DownloadedSnapshot, AppError> {
        self.progress.emit(ProgressEvent::Connecting {
            source: self.source_name.clone(),
        });
        let (manifest, manifest_bytes) =
            self.find_manifest()
                .await?
                .ok_or_else(|| AppError::RemoteEmpty {
                    source_name: self.source_name.clone(),
                })?;
        tokio::fs::create_dir_all(staging)
            .await
            .map_err(|error| AppError::io(staging, error))?;
        let layout = manifest.layout();
        let db_sql_path = self
            .download_artifact(
                staging,
                layout,
                REMOTE_DB_SQL,
                manifest.artifact(REMOTE_DB_SQL)?,
            )
            .await?;
        let skills_zip_path = self
            .download_artifact(
                staging,
                layout,
                REMOTE_SKILLS_ZIP,
                manifest.artifact(REMOTE_SKILLS_ZIP)?,
            )
            .await?;

        Ok(DownloadedSnapshot {
            manifest,
            manifest_bytes,
            layout,
            db_sql_path,
            skills_zip_path,
        })
    }

    async fn find_manifest(&self) -> Result<Option<(ValidatedManifest, Vec<u8>)>, AppError> {
        if let Some(snapshot) = self.fetch_manifest(RemoteLayout::Current).await? {
            return Ok(Some(snapshot));
        }
        self.fetch_manifest(RemoteLayout::Legacy).await
    }

    async fn fetch_manifest(
        &self,
        layout: RemoteLayout,
    ) -> Result<Option<(ValidatedManifest, Vec<u8>)>, AppError> {
        self.progress.emit(ProgressEvent::FetchingManifest);
        let url = self.remote_file_url(layout, REMOTE_MANIFEST)?;
        let Some(response) = self.send_get(&url, "GET manifest").await? else {
            return Ok(None);
        };
        let bytes =
            read_limited_response(response, REMOTE_MANIFEST, MAX_MANIFEST_BYTES as u64).await?;
        self.progress.emit(ProgressEvent::ValidatingManifest);
        let manifest = SyncManifest::parse(&bytes)?.validate(layout)?;
        Ok(Some((manifest, bytes)))
    }

    async fn download_artifact(
        &self,
        staging: &Path,
        layout: RemoteLayout,
        artifact: &str,
        meta: &super::protocol::ArtifactMeta,
    ) -> Result<PathBuf, AppError> {
        let url = self.remote_file_url(layout, artifact)?;
        let response = self.send_get(&url, "GET artifact").await?.ok_or_else(|| {
            AppError::RemoteArtifactMissing {
                artifact: artifact.to_string(),
            }
        })?;
        enforce_content_length(response.content_length(), artifact, MAX_SYNC_ARTIFACT_BYTES)?;

        let target = staging.join(artifact);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .await
            .map_err(|error| AppError::io(&target, error))?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        let mut hasher = Sha256::new();
        self.progress.emit(ProgressEvent::Downloading {
            artifact: artifact.to_string(),
            downloaded,
            total: meta.size,
        });

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&target).await;
                    return Err(transport_error("read response", &url, &error));
                }
            };
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > MAX_SYNC_ARTIFACT_BYTES {
                drop(file);
                let _ = tokio::fs::remove_file(&target).await;
                return Err(AppError::ResponseTooLarge {
                    target: artifact.to_string(),
                    size: downloaded,
                    max: MAX_SYNC_ARTIFACT_BYTES,
                });
            }
            if downloaded > meta.size {
                drop(file);
                let _ = tokio::fs::remove_file(&target).await;
                return Err(AppError::ArtifactSizeMismatch {
                    artifact: artifact.to_string(),
                    expected: meta.size,
                    actual: downloaded,
                });
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| AppError::io(&target, error))?;
            hasher.update(&chunk);
            self.progress.emit(ProgressEvent::Downloading {
                artifact: artifact.to_string(),
                downloaded,
                total: meta.size,
            });
        }
        file.flush()
            .await
            .map_err(|error| AppError::io(&target, error))?;
        file.sync_all()
            .await
            .map_err(|error| AppError::io(&target, error))?;

        if downloaded != meta.size {
            drop(file);
            let _ = tokio::fs::remove_file(&target).await;
            return Err(AppError::ArtifactSizeMismatch {
                artifact: artifact.to_string(),
                expected: meta.size,
                actual: downloaded,
            });
        }
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&meta.sha256) {
            drop(file);
            let _ = tokio::fs::remove_file(&target).await;
            return Err(AppError::ArtifactHashMismatch {
                artifact: artifact.to_string(),
                expected: meta.sha256.clone(),
                actual,
            });
        }
        self.progress.emit(ProgressEvent::Verifying {
            artifact: artifact.to_string(),
        });
        Ok(target)
    }

    async fn send_get(
        &self,
        url: &Url,
        operation: &'static str,
    ) -> Result<Option<Response>, AppError> {
        for attempt in 1..=MAX_ATTEMPTS {
            let response = self
                .client
                .get(url.clone())
                .basic_auth(&self.config.username, Some(&self.config.password))
                .timeout(TRANSFER_TIMEOUT)
                .send()
                .await;
            match response {
                Ok(response) if response.status() == StatusCode::NOT_FOUND => return Ok(None),
                Ok(response)
                    if should_retry_status(response.status()) && attempt < MAX_ATTEMPTS =>
                {
                    self.emit_retry(operation, attempt);
                    retry_delay(attempt).await;
                }
                Ok(response) if !response.status().is_success() => {
                    return Err(AppError::WebDavHttp {
                        operation,
                        status: response.status().as_u16(),
                        url: redact_url(url),
                    });
                }
                Ok(response) => return Ok(Some(response)),
                Err(error) if should_retry_error(&error) && attempt < MAX_ATTEMPTS => {
                    self.emit_retry(operation, attempt);
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(transport_error(operation, url, &error)),
            }
        }
        unreachable!("retry loop always returns on the final attempt")
    }

    fn emit_retry(&self, operation: &str, attempt: u8) {
        self.progress.emit(ProgressEvent::Retrying {
            operation: operation.to_string(),
            attempt: attempt + 1,
            max_attempts: MAX_ATTEMPTS,
        });
    }

    fn remote_file_url(&self, layout: RemoteLayout, file_name: &str) -> Result<Url, AppError> {
        let mut url = Url::parse(&self.config.base_url)
            .map_err(|_| AppError::InvalidConfig("WebDAV URL is invalid".to_string()))?;
        let segments = self.remote_segments(layout, file_name);
        {
            let mut path = url.path_segments_mut().map_err(|_| {
                AppError::InvalidConfig("WebDAV URL cannot accept path segments".to_string())
            })?;
            path.pop_if_empty();
            for segment in &segments {
                path.push(segment);
            }
        }
        Ok(url)
    }

    fn remote_segments(&self, layout: RemoteLayout, file_name: &str) -> Vec<String> {
        let mut segments = split_path(&self.remote_root);
        segments.push("v2".to_string());
        if layout == RemoteLayout::Current {
            segments.push("db-v6".to_string());
        }
        segments.extend(split_path(&self.profile));
        segments.extend(split_path(file_name));
        segments
    }
}

async fn read_limited_response(
    response: Response,
    target: &str,
    max: u64,
) -> Result<Vec<u8>, AppError> {
    enforce_content_length(response.content_length(), target, max)?;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::WebDavTransport {
            operation: "read response",
            url: "<redacted>".to_string(),
            reason: transport_reason(&error),
        })?;
        if bytes.len().saturating_add(chunk.len()) as u64 > max {
            return Err(AppError::ResponseTooLarge {
                target: target.to_string(),
                size: bytes.len().saturating_add(chunk.len()) as u64,
                max,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
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

fn split_path(raw: &str) -> Vec<String> {
    raw.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn should_retry_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout()
}

async fn retry_delay(attempt: u8) {
    tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt))).await;
}

fn transport_error(operation: &'static str, url: &Url, error: &reqwest::Error) -> AppError {
    AppError::WebDavTransport {
        operation,
        url: redact_url(url),
        reason: transport_reason(error),
    }
}

fn transport_reason(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_request() {
        "request construction failed"
    } else {
        "response read failed"
    }
}

fn redact_url(url: &Url) -> String {
    let mut parsed = url.clone();
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    let mut keys = parsed
        .query_pairs()
        .map(|(key, _)| key.into_owned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    parsed.set_query(None);
    parsed.set_fragment(None);
    let mut safe = parsed.to_string();
    if !keys.is_empty() {
        safe.push_str("?[keys:");
        safe.push_str(&keys.join(","));
        safe.push(']');
    }
    safe
}
