//! Read-only WebDAV transport adapted from CC Switch.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Response, StatusCode, Url};

use super::protocol::{
    RemoteLayout, SyncManifest, ValidatedManifest, MAX_MANIFEST_BYTES, REMOTE_DB_SQL,
    REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};
use super::{read_limited_response, write_verified_artifact, DownloadedSnapshot};
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
        let bytes = read_limited_response(
            response,
            REMOTE_MANIFEST,
            MAX_MANIFEST_BYTES as u64,
            |error| transport_error("read manifest response", &url, error),
        )
        .await?;
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
        write_verified_artifact(
            response,
            staging,
            artifact,
            meta,
            self.progress.as_ref(),
            |error| transport_error("read artifact response", &url, error),
        )
        .await
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
