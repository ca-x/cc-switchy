use std::collections::BTreeMap;
use std::sync::Arc;

use cc_switchy::config::{S3Config, SourceConfig, SourceKind};
use cc_switchy::progress::{ChannelProgress, NoopProgress, ProgressEvent};
use cc_switchy::remote::protocol::{
    compute_snapshot_id, sha256_hex, ArtifactMeta, SyncManifest, DB_COMPAT_VERSION,
    PROTOCOL_FORMAT, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_SKILLS_ZIP,
};
use cc_switchy::remote::s3::{
    build_bucket_url, build_object_url, sign_read_headers, Clock, ReadMethod, S3Client,
};
use cc_switchy::AppError;
use chrono::{TimeZone, Utc};
use httpmock::prelude::*;
use tempfile::TempDir;

#[derive(Clone)]
struct FixedClock(chrono::DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        self.0
    }
}

fn fixed_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(
        Utc.with_ymd_and_hms(2026, 7, 13, 10, 42, 0)
            .single()
            .expect("valid fixed clock"),
    ))
}

fn s3_config(endpoint: String) -> S3Config {
    S3Config {
        region: "us-east-1".to_string(),
        bucket: "backup-bucket".to_string(),
        endpoint,
        access_key_id: "TESTACCESSKEY".to_string(),
        secret_access_key: "test-secret-key".to_string(),
    }
}

fn source(endpoint: String) -> SourceConfig {
    SourceConfig {
        name: "backup-s3".to_string(),
        remote_root: "cc-switch-sync".to_string(),
        profile: "default".to_string(),
        kind: SourceKind::S3 {
            s3: s3_config(endpoint),
        },
    }
}

fn manifest(db: &[u8], skills: &[u8]) -> Vec<u8> {
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(db),
            size: db.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(skills),
            size: skills.len() as u64,
        },
    );
    let snapshot_id = compute_snapshot_id(&artifacts);
    serde_json::to_vec(&SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: "remote-host".to_string(),
        created_at: "2026-07-13T10:42:00Z".to_string(),
        artifacts,
        snapshot_id,
    })
    .expect("manifest JSON")
}

#[test]
fn builds_aws_and_custom_endpoint_urls() {
    let aws = s3_config(String::new());
    assert_eq!(
        build_object_url(&aws, "cc-switch-sync/v2/db-v6/default/manifest.json")
            .expect("AWS object URL")
            .as_str(),
        "https://backup-bucket.s3.us-east-1.amazonaws.com/cc-switch-sync/v2/db-v6/default/manifest.json"
    );

    let custom = s3_config("http://minio.example.test:9000/storage/root/".to_string());
    assert_eq!(
        build_bucket_url(&custom)
            .expect("custom bucket URL")
            .as_str(),
        "http://minio.example.test:9000/storage/root/backup-bucket/"
    );
    assert_eq!(
        build_object_url(&custom, "folder/file name.json")
            .expect("custom object URL")
            .as_str(),
        "http://minio.example.test:9000/storage/root/backup-bucket/folder/file%20name.json"
    );

    let bare = s3_config("minio.example.test:9000".to_string());
    assert_eq!(
        build_bucket_url(&bare).expect("bare endpoint").scheme(),
        "https"
    );
}

#[test]
fn redacted_source_masks_access_key_and_secret() {
    let source = source("https://storage.example.test/root?token=private".to_string());
    let rendered = format!("{:?}", source.redacted());

    assert!(rendered.contains("TEST*********"));
    assert!(!rendered.contains("TESTACCESSKEY"));
    assert!(!rendered.contains("test-secret-key"));
    assert!(!rendered.contains("private"));
}

#[test]
fn signs_the_aws_documentation_read_vector() {
    let config = S3Config {
        region: "us-east-1".to_string(),
        bucket: "examplebucket".to_string(),
        endpoint: String::new(),
        access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
    };
    let url = reqwest::Url::parse("https://examplebucket.s3.amazonaws.com/?lifecycle")
        .expect("vector URL");
    let now = Utc
        .with_ymd_and_hms(2013, 5, 24, 0, 0, 0)
        .single()
        .expect("vector clock");

    let headers = sign_read_headers(ReadMethod::Get, &url, &config, now)
        .expect("sign AWS documentation request");
    let authorization = headers
        .get("authorization")
        .expect("authorization")
        .to_str()
        .expect("ASCII authorization");

    assert!(authorization
        .contains("Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request"));
    assert!(authorization.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
    assert!(authorization
        .contains("Signature=fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"));
}

#[tokio::test]
async fn downloads_current_snapshot_with_sigv4_progress_and_path_style_endpoint() {
    let server = MockServer::start();
    let db = b"-- CC Switch SQLite export";
    let skills = b"PK fixture zip";
    let manifest_bytes = manifest(db, skills);
    let root = "/storage/root/backup-bucket/cc-switch-sync/v2/db-v6/default";
    let manifest_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("{root}/manifest.json"))
            .header("x-amz-date", "20260713T104200Z")
            .header_prefix(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=TESTACCESSKEY/20260713/us-east-1/s3/aws4_request",
            );
        then.status(200).body(manifest_bytes.clone());
    });
    let db_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("{root}/db.sql"))
            .header_exists("authorization");
        then.status(200).body(db.as_slice());
    });
    let skills_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("{root}/skills.zip"))
            .header_exists("authorization");
        then.status(200).body(skills.as_slice());
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    let progress = Arc::new(ChannelProgress::new(sender));
    let staging = TempDir::new().expect("staging");
    let client = S3Client::new_with_clock(
        source(format!("{}/storage/root", server.base_url())),
        reqwest::Client::new(),
        progress,
        fixed_clock(),
    )
    .expect("client");

    let snapshot = client
        .fetch_snapshot(staging.path())
        .await
        .expect("download snapshot");

    assert_eq!(std::fs::read(snapshot.db_sql_path).unwrap(), db);
    assert_eq!(std::fs::read(snapshot.skills_zip_path).unwrap(), skills);
    manifest_mock.assert_calls(1);
    db_mock.assert_calls(1);
    skills_mock.assert_calls(1);
    let events = receiver.try_iter().collect::<Vec<_>>();
    assert!(events.iter().any(|event| matches!(
        event,
        ProgressEvent::Downloading { artifact, downloaded, total }
            if artifact == REMOTE_SKILLS_ZIP
                && *downloaded == skills.len() as u64
                && *total == skills.len() as u64
    )));
}

#[tokio::test]
async fn connection_test_uses_head_then_reports_an_empty_snapshot() {
    let server = MockServer::start();
    let bucket = server.mock(|when, then| {
        when.method("HEAD")
            .path("/backup-bucket/")
            .header_exists("authorization");
        then.status(200);
    });
    let manifest = server.mock(|when, then| {
        when.method(GET)
            .path("/backup-bucket/cc-switch-sync/v2/db-v6/default/manifest.json");
        then.status(404);
    });
    let client = S3Client::new_with_clock(
        source(format!("{}?token=private", server.base_url())),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
        fixed_clock(),
    )
    .expect("client");

    assert!(client
        .test_connection()
        .await
        .expect("connection")
        .is_none());
    bucket.assert_calls(1);
    manifest.assert_calls(1);
}

#[tokio::test]
async fn authentication_failures_are_not_retried() {
    let server = MockServer::start();
    let forbidden = server.mock(|when, then| {
        when.method("HEAD").path("/backup-bucket/");
        then.status(403);
    });
    let client = S3Client::new_with_clock(
        source(server.base_url()),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
        fixed_clock(),
    )
    .expect("client");

    let error = client
        .test_connection()
        .await
        .expect_err("authentication should fail");
    assert!(matches!(&error, AppError::S3Http { status: 403, .. }));
    let rendered = error.to_string();
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("test-secret-key"));
    forbidden.assert_calls(1);
}

#[tokio::test]
async fn transient_failures_retry_at_most_three_times() {
    let server = MockServer::start();
    let unavailable = server.mock(|when, then| {
        when.method(GET)
            .path("/backup-bucket/cc-switch-sync/v2/db-v6/default/manifest.json");
        then.status(503);
    });
    let client = S3Client::new_with_clock(
        source(server.base_url()),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
        fixed_clock(),
    )
    .expect("client");
    let staging = TempDir::new().expect("staging");

    assert!(matches!(
        client.fetch_snapshot(staging.path()).await,
        Err(AppError::S3Http { status: 503, .. })
    ));
    unavailable.assert_calls(3);
}

#[test]
fn implementation_has_no_remote_write_surface() {
    let implementation = include_str!("../src/remote/s3.rs");
    assert!(!implementation.contains(".put("));
    assert!(!implementation.contains(".post("));
    assert!(!implementation.contains(".delete("));
    assert!(!implementation.contains("multipart"));
}
