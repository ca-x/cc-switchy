//! Read-only S3 transport adapted from CC Switch's SigV4 implementation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Method, Response, StatusCode, Url};
use sha2::{Digest, Sha256};

use super::protocol::{
    RemoteLayout, SyncManifest, ValidatedManifest, DB_COMPAT_VERSION, MAX_MANIFEST_BYTES,
    PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};
use super::{read_limited_response, write_verified_artifact, DownloadedSnapshot};
use crate::config::{S3Config, SourceConfig, SourceKind};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::AppError;

const MAX_ATTEMPTS: u8 = 3;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
const EMPTY_BODY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

struct S3Credentials {
    access_key_id: String,
    secret_access_key: String,
    region: String,
    bucket: String,
    endpoint: String,
}

impl From<&S3Config> for S3Credentials {
    fn from(config: &S3Config) -> Self {
        Self {
            access_key_id: config.access_key_id.clone(),
            secret_access_key: config.secret_access_key.clone(),
            region: config.region.clone(),
            bucket: config.bucket.clone(),
            endpoint: config.endpoint.clone(),
        }
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadMethod {
    Get,
    Head,
}

impl ReadMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
        }
    }

    fn as_reqwest(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Head => Method::HEAD,
        }
    }
}

pub struct S3Client {
    source_name: String,
    remote_root: String,
    profile: String,
    config: S3Config,
    client: reqwest::Client,
    progress: Arc<dyn ProgressSink>,
    clock: Arc<dyn Clock>,
}

impl S3Client {
    pub fn new(
        source: SourceConfig,
        client: reqwest::Client,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Self, AppError> {
        Self::new_with_clock(source, client, progress, Arc::new(SystemClock))
    }

    pub fn new_with_clock(
        mut source: SourceConfig,
        client: reqwest::Client,
        progress: Arc<dyn ProgressSink>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, AppError> {
        source.normalize();
        source.validate()?;
        let config = match source.kind {
            SourceKind::S3 { s3 } => s3,
            SourceKind::WebDav { .. } => {
                return Err(AppError::InvalidConfig(
                    "an S3 source is required".to_string(),
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
            clock,
        })
    }

    pub async fn test_connection(&self) -> Result<Option<ValidatedManifest>, AppError> {
        self.progress.emit(ProgressEvent::Connecting {
            source: self.source_name.clone(),
        });
        let bucket_url = build_bucket_url(&self.config)?;
        if self
            .send_read(ReadMethod::Head, &bucket_url, "HEAD bucket")
            .await?
            .is_none()
        {
            return Err(AppError::S3Http {
                operation: "HEAD bucket",
                status: StatusCode::NOT_FOUND.as_u16(),
                url: redact_url(&bucket_url),
            });
        }
        Ok(self.fetch_manifest().await?.map(|snapshot| snapshot.0))
    }

    pub async fn fetch_snapshot(&self, staging: &Path) -> Result<DownloadedSnapshot, AppError> {
        self.progress.emit(ProgressEvent::Connecting {
            source: self.source_name.clone(),
        });
        let (manifest, manifest_bytes) =
            self.fetch_manifest()
                .await?
                .ok_or_else(|| AppError::RemoteEmpty {
                    source_name: self.source_name.clone(),
                })?;
        tokio::fs::create_dir_all(staging)
            .await
            .map_err(|error| AppError::io(staging, error))?;
        let db_sql_path = self
            .download_artifact(staging, REMOTE_DB_SQL, manifest.artifact(REMOTE_DB_SQL)?)
            .await?;
        let skills_zip_path = self
            .download_artifact(
                staging,
                REMOTE_SKILLS_ZIP,
                manifest.artifact(REMOTE_SKILLS_ZIP)?,
            )
            .await?;

        Ok(DownloadedSnapshot {
            manifest,
            manifest_bytes,
            layout: RemoteLayout::Current,
            db_sql_path,
            skills_zip_path,
        })
    }

    async fn fetch_manifest(&self) -> Result<Option<(ValidatedManifest, Vec<u8>)>, AppError> {
        self.progress.emit(ProgressEvent::FetchingManifest);
        let url = self.object_url(REMOTE_MANIFEST)?;
        let Some(response) = self
            .send_read(ReadMethod::Get, &url, "GET manifest")
            .await?
        else {
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
        let manifest = SyncManifest::parse(&bytes)?.validate(RemoteLayout::Current)?;
        Ok(Some((manifest, bytes)))
    }

    async fn download_artifact(
        &self,
        staging: &Path,
        artifact: &str,
        meta: &super::protocol::ArtifactMeta,
    ) -> Result<PathBuf, AppError> {
        let url = self.object_url(artifact)?;
        let response = self
            .send_read(ReadMethod::Get, &url, "GET artifact")
            .await?
            .ok_or_else(|| AppError::RemoteArtifactMissing {
                artifact: artifact.to_string(),
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

    async fn send_read(
        &self,
        method: ReadMethod,
        url: &Url,
        operation: &'static str,
    ) -> Result<Option<Response>, AppError> {
        for attempt in 1..=MAX_ATTEMPTS {
            let headers = sign_read_headers(method, url, &self.config, self.clock.now())?;
            let response = self
                .client
                .request(method.as_reqwest(), url.clone())
                .headers(headers)
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
                    return Err(AppError::S3Http {
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

    fn object_url(&self, artifact: &str) -> Result<Url, AppError> {
        build_object_url(&self.config, &self.object_key(artifact))
    }

    fn object_key(&self, artifact: &str) -> String {
        let mut segments = split_path(&self.remote_root);
        segments.push(format!("v{PROTOCOL_VERSION}"));
        segments.push(format!("db-v{DB_COMPAT_VERSION}"));
        segments.extend(split_path(&self.profile));
        segments.extend(split_path(artifact));
        segments.join("/")
    }
}

pub fn build_bucket_url(config: &S3Config) -> Result<Url, AppError> {
    let credentials = S3Credentials::from(config);
    if is_aws_endpoint(&credentials.endpoint) {
        return parse_url(&format!(
            "https://{}.s3.{}.amazonaws.com/",
            credentials.bucket, credentials.region
        ));
    }

    let mut url = parse_custom_endpoint(&credentials.endpoint)?;
    append_path_segments(&mut url, [&credentials.bucket], true)?;
    Ok(url)
}

pub fn build_object_url(config: &S3Config, key: &str) -> Result<Url, AppError> {
    let credentials = S3Credentials::from(config);
    let mut url = if is_aws_endpoint(&credentials.endpoint) {
        parse_url(&format!(
            "https://{}.s3.{}.amazonaws.com/",
            credentials.bucket, credentials.region
        ))?
    } else {
        let mut url = parse_custom_endpoint(&credentials.endpoint)?;
        append_path_segments(&mut url, [&credentials.bucket], false)?;
        url
    };
    append_path_segments(&mut url, split_path(key), false)?;
    Ok(url)
}

pub fn sign_read_headers(
    method: ReadMethod,
    url: &Url,
    config: &S3Config,
    now: DateTime<Utc>,
) -> Result<HeaderMap, AppError> {
    let credentials = S3Credentials::from(config);
    let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let datestamp = now.format("%Y%m%d").to_string();
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    };

    let mut headers = HeaderMap::new();
    headers.insert("host", header_value(&host)?);
    headers.insert("x-amz-date", header_value(&timestamp)?);
    headers.insert(
        "x-amz-content-sha256",
        HeaderValue::from_static(EMPTY_BODY_SHA256),
    );

    let mut query_pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    query_pairs.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let canonical_query = query_pairs
        .iter()
        .map(|(key, value)| format!("{}={}", uri_encode(key), uri_encode(value)))
        .collect::<Vec<_>>()
        .join("&");

    let mut header_names = headers
        .keys()
        .map(|name| name.as_str().to_ascii_lowercase())
        .collect::<Vec<_>>();
    header_names.sort();
    header_names.dedup();
    let canonical_headers = header_names
        .iter()
        .map(|name| {
            let value = headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .trim();
            format!("{name}:{value}\n")
        })
        .collect::<String>();
    let signed_headers = header_names.join(";");
    let canonical_uri = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.as_str(),
        canonical_uri,
        canonical_query,
        canonical_headers,
        signed_headers,
        EMPTY_BODY_SHA256
    );
    let scope = format!("{datestamp}/{}/s3/aws4_request", credentials.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac_sha256(
        format!("AWS4{}", credentials.secret_access_key).as_bytes(),
        datestamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, credentials.region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    );
    headers.insert("authorization", header_value(&authorization)?);
    Ok(headers)
}

fn is_aws_endpoint(endpoint: &str) -> bool {
    if endpoint.trim().is_empty() {
        return true;
    }
    let candidate = if endpoint.contains("://") {
        endpoint.to_string()
    } else {
        format!("https://{endpoint}")
    };
    Url::parse(&candidate)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "amazonaws.com" || host.ends_with(".amazonaws.com"))
}

fn parse_custom_endpoint(endpoint: &str) -> Result<Url, AppError> {
    let candidate = if endpoint.contains("://") {
        endpoint.to_string()
    } else {
        format!("https://{endpoint}")
    };
    parse_url(&candidate)
}

fn parse_url(raw: &str) -> Result<Url, AppError> {
    let url =
        Url::parse(raw).map_err(|_| AppError::InvalidConfig("S3 URL is invalid".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::InvalidConfig(
            "S3 URL must use HTTP or HTTPS and include a host".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidConfig(
            "S3 endpoint must not include URL credentials".to_string(),
        ));
    }
    Ok(url)
}

fn append_path_segments<I, S>(
    url: &mut Url,
    segments: I,
    trailing_slash: bool,
) -> Result<(), AppError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut path = url.path_segments_mut().map_err(|_| {
        AppError::InvalidConfig("S3 endpoint cannot accept path segments".to_string())
    })?;
    path.pop_if_empty();
    for segment in segments {
        path.push(segment.as_ref());
    }
    if trailing_slash {
        path.push("");
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

fn uri_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(char::from(byte));
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(output, "%{byte:02X}");
            }
        }
    }
    output
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn header_value(value: &str) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value)
        .map_err(|_| AppError::S3Signing("credential contains invalid characters".to_string()))
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
    AppError::S3Transport {
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
