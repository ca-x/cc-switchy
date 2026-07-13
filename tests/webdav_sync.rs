use std::collections::BTreeMap;
use std::sync::Arc;

use cc_switchy::config::{SourceConfig, SourceKind, WebDavConfig};
use cc_switchy::progress::{ChannelProgress, NoopProgress, ProgressEvent};
use cc_switchy::remote::protocol::{
    compute_snapshot_id, sha256_hex, ArtifactMeta, SyncManifest, DB_COMPAT_VERSION,
    PROTOCOL_FORMAT, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_SKILLS_ZIP,
};
use cc_switchy::remote::webdav::WebDavClient;
use cc_switchy::AppError;
use httpmock::prelude::*;
use tempfile::TempDir;

fn source(base_url: String) -> SourceConfig {
    SourceConfig {
        name: "home-webdav".to_string(),
        remote_root: "cc-switch-sync".to_string(),
        profile: "default".to_string(),
        kind: SourceKind::WebDav {
            webdav: WebDavConfig {
                base_url,
                username: "user".to_string(),
                password: "secret".to_string(),
            },
        },
    }
}

fn manifest(db: &[u8], skills: &[u8], db_version: Option<u32>) -> Vec<u8> {
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
        db_compat_version: db_version,
        device_name: "remote-host".to_string(),
        created_at: "2026-07-13T10:42:00Z".to_string(),
        artifacts,
        snapshot_id,
    })
    .expect("manifest JSON")
}

#[tokio::test]
async fn downloads_current_snapshot_with_auth_progress_and_retained_base_path() {
    let server = MockServer::start();
    let db = b"-- CC Switch SQLite export";
    let skills = b"PK fixture zip";
    let manifest_bytes = manifest(db, skills, Some(DB_COMPAT_VERSION));
    let auth = "Basic dXNlcjpzZWNyZXQ=";
    let manifest_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/dav/base/cc-switch-sync/v2/db-v6/default/manifest.json")
            .header("authorization", auth);
        then.status(200).body(manifest_bytes.clone());
    });
    let db_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/dav/base/cc-switch-sync/v2/db-v6/default/db.sql")
            .header("authorization", auth);
        then.status(200).body(db.as_slice());
    });
    let skills_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/dav/base/cc-switch-sync/v2/db-v6/default/skills.zip")
            .header("authorization", auth);
        then.status(200).body(skills.as_slice());
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    let progress = Arc::new(ChannelProgress::new(sender));
    let staging = TempDir::new().expect("staging");
    let client = WebDavClient::new(
        source(format!("{}/dav/base/", server.base_url())),
        reqwest::Client::new(),
        progress,
    )
    .expect("client");

    let snapshot = client
        .fetch_snapshot(staging.path())
        .await
        .expect("download snapshot");

    assert_eq!(
        snapshot.manifest.snapshot_id(),
        compute_snapshot_id(&snapshot.manifest.manifest.artifacts)
    );
    assert_eq!(std::fs::read(snapshot.db_sql_path).unwrap(), db);
    assert_eq!(std::fs::read(snapshot.skills_zip_path).unwrap(), skills);
    manifest_mock.assert_calls(1);
    db_mock.assert_calls(1);
    skills_mock.assert_calls(1);
    let events = receiver.try_iter().collect::<Vec<_>>();
    assert!(events.iter().any(|event| matches!(
        event,
        ProgressEvent::Downloading { artifact, downloaded, total }
            if artifact == REMOTE_DB_SQL && *downloaded == db.len() as u64 && *total == db.len() as u64
    )));
}

#[tokio::test]
async fn falls_back_to_legacy_only_when_current_manifest_is_missing() {
    let server = MockServer::start();
    let db = b"db";
    let skills = b"skills";
    let current = server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/db-v6/default/manifest.json");
        then.status(404);
    });
    let legacy = server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/default/manifest.json");
        then.status(200).body(manifest(db, skills, None));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/default/db.sql");
        then.status(200).body(db.as_slice());
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/default/skills.zip");
        then.status(200).body(skills.as_slice());
    });
    let staging = TempDir::new().expect("staging");
    let client = WebDavClient::new(
        source(format!("{}/root", server.base_url())),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
    )
    .expect("client");

    let snapshot = client
        .fetch_snapshot(staging.path())
        .await
        .expect("legacy snapshot");

    assert_eq!(snapshot.layout.as_str(), "legacy");
    assert_eq!(snapshot.manifest.db_compat_version(), 5);
    current.assert_calls(1);
    legacy.assert_calls(1);
}

#[tokio::test]
async fn authentication_failures_are_not_retried_and_errors_are_redacted() {
    let server = MockServer::start();
    let unauthorized = server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/db-v6/default/manifest.json");
        then.status(401);
    });
    let client = WebDavClient::new(
        source(format!("{}/root?token=private", server.base_url())),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
    )
    .expect("client");

    let error = client
        .test_connection()
        .await
        .expect_err("authentication should fail");

    unauthorized.assert_calls(1);
    let rendered = error.to_string();
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("secret"));
}

#[tokio::test]
async fn transient_server_failures_retry_at_most_three_times() {
    let server = MockServer::start();
    let unavailable = server.mock(|when, then| {
        when.method(GET)
            .path("/root/cc-switch-sync/v2/db-v6/default/manifest.json");
        then.status(503);
    });
    let client = WebDavClient::new(
        source(format!("{}/root", server.base_url())),
        reqwest::Client::new(),
        Arc::new(NoopProgress),
    )
    .expect("client");

    assert!(matches!(
        client.test_connection().await,
        Err(AppError::WebDavHttp { status: 503, .. })
    ));
    unavailable.assert_calls(3);
}

#[test]
fn implementation_has_no_remote_write_verbs() {
    let implementation = include_str!("../src/remote/webdav.rs");
    assert!(!implementation.contains(".put("));
    assert!(!implementation.contains(".post("));
    assert!(!implementation.contains(".delete("));
    assert!(!implementation.contains("MKCOL"));
}
