use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use cc_switchy::commands::{SyncRequest, SyncService};
use cc_switchy::config::{ConfigStore, SourceCatalog, SourceConfig, SourceKind, WebDavConfig};
use cc_switchy::progress::{ProgressEvent, ProgressSink};
use cc_switchy::remote::protocol::{
    compute_snapshot_id, sha256_hex, ArtifactMeta, SyncManifest, DB_COMPAT_VERSION,
    PROTOCOL_FORMAT, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};
use cc_switchy::restore::SyncLockGuard;
use cc_switchy::{AppError, AppPaths};
use httpmock::prelude::*;
use httpmock::Mock;
use predicates::prelude::*;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const MANIFEST: &[u8] = include_bytes!("fixtures/cc-switch-v2/manifest.json");
const DATABASE: &[u8] = include_bytes!("fixtures/cc-switch-v2/db.sql");
const SKILLS: &[u8] = include_bytes!("fixtures/cc-switch-v2/skills.zip");

#[derive(Default)]
struct RecordingProgress {
    events: Mutex<Vec<ProgressEvent>>,
}

impl RecordingProgress {
    fn events(&self) -> Vec<ProgressEvent> {
        self.events.lock().expect("events lock").clone()
    }
}

impl ProgressSink for RecordingProgress {
    fn emit(&self, event: ProgressEvent) {
        self.events.lock().expect("events lock").push(event);
    }
}

struct CancelBeforeRestore {
    cancellation: CancellationToken,
    events: Mutex<Vec<ProgressEvent>>,
}

impl ProgressSink for CancelBeforeRestore {
    fn emit(&self, event: ProgressEvent) {
        if matches!(
            &event,
            ProgressEvent::Verifying { artifact } if artifact == REMOTE_SKILLS_ZIP
        ) {
            self.cancellation.cancel();
        }
        self.events.lock().expect("events lock").push(event);
    }
}

fn source(name: &str, base_url: String) -> SourceConfig {
    SourceConfig {
        name: name.to_string(),
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

fn catalog(paths: &AppPaths, sources: impl IntoIterator<Item = SourceConfig>) -> SourceCatalog {
    let mut catalog =
        SourceCatalog::load(ConfigStore::new(paths.config_file.clone())).expect("catalog");
    for source in sources {
        catalog.add(source).expect("add source");
    }
    catalog
}

fn mount_snapshot(server: &MockServer) -> (Mock<'_>, Mock<'_>, Mock<'_>) {
    mount_snapshot_bytes(server, MANIFEST, DATABASE, SKILLS)
}

fn mount_snapshot_bytes<'a>(
    server: &'a MockServer,
    manifest_bytes: &'a [u8],
    database_bytes: &'a [u8],
    skills_bytes: &'a [u8],
) -> (Mock<'a>, Mock<'a>, Mock<'a>) {
    let manifest = server.mock(|when, then| {
        when.method(GET).path(format!(
            "/cc-switch-sync/v2/db-v6/default/{REMOTE_MANIFEST}"
        ));
        then.status(200).body(manifest_bytes);
    });
    let database = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/cc-switch-sync/v2/db-v6/default/{REMOTE_DB_SQL}"));
        then.status(200).body(database_bytes);
    });
    let skills = server.mock(|when, then| {
        when.method(GET).path(format!(
            "/cc-switch-sync/v2/db-v6/default/{REMOTE_SKILLS_ZIP}"
        ));
        then.status(200).body(skills_bytes);
    });
    (manifest, database, skills)
}

fn manifest_for(database: &[u8], skills: &[u8]) -> Vec<u8> {
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(database),
            size: database.len() as u64,
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
        device_name: "fixture".to_string(),
        created_at: "2026-07-13T10:42:00Z".to_string(),
        artifacts,
        snapshot_id,
    })
    .expect("manifest")
}

#[tokio::test]
async fn consecutive_syncs_refetch_and_project_in_provider_mcp_skills_order() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let server = MockServer::start();
    let (manifest, database, skills) = mount_snapshot(&server);
    let progress = Arc::new(RecordingProgress::default());
    let mut service = SyncService {
        paths: paths.clone(),
        catalog: catalog(&paths, [source("home", server.base_url())]),
        progress: progress.clone(),
        cancellation: CancellationToken::new(),
    };

    let first = service
        .run(SyncRequest { source_name: None })
        .await
        .expect("first sync");
    let second = service
        .run(SyncRequest { source_name: None })
        .await
        .expect("second sync");

    assert_eq!(first.source_name, "home");
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert!(first.backup_dir.is_dir());
    assert!(second.backup_dir.is_dir());
    manifest.assert_calls(2);
    database.assert_calls(2);
    skills.assert_calls(2);
    assert!(paths.cc_switch_dir.join("cc-switch.db").is_file());
    assert!(paths.state_file.is_file());
    assert_eq!(
        fs::read_dir(&paths.staging_dir).expect("staging").count(),
        0
    );

    let events = progress.events();
    let provider = events
        .iter()
        .position(|event| matches!(event, ProgressEvent::ApplyingProvider { .. }))
        .expect("provider event");
    let mcp = events
        .iter()
        .position(|event| matches!(event, ProgressEvent::ApplyingMcp { .. }))
        .expect("MCP event");
    let skill = events
        .iter()
        .position(|event| matches!(event, ProgressEvent::ApplyingSkills { .. }))
        .expect("Skills event");
    assert!(provider < mcp && mcp < skill);
    assert!(matches!(events.first(), Some(ProgressEvent::Locking)));
    assert!(events
        .iter()
        .any(|event| matches!(event, ProgressEvent::Completed { .. })));
}

#[tokio::test]
async fn explicit_source_override_does_not_change_the_persisted_default() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let default_server = MockServer::start();
    let override_server = MockServer::start();
    let (default_manifest, _, _) = mount_snapshot(&default_server);
    let (override_manifest, override_database, override_skills) = mount_snapshot(&override_server);
    let mut service = SyncService {
        paths: paths.clone(),
        catalog: catalog(
            &paths,
            [
                source("default", default_server.base_url()),
                source("override", override_server.base_url()),
            ],
        ),
        progress: Arc::new(RecordingProgress::default()),
        cancellation: CancellationToken::new(),
    };

    let outcome = service
        .run(SyncRequest {
            source_name: Some("override".to_string()),
        })
        .await
        .expect("override sync");

    assert_eq!(outcome.source_name, "override");
    default_manifest.assert_calls(0);
    override_manifest.assert_calls(1);
    override_database.assert_calls(1);
    override_skills.assert_calls(1);
    let persisted = ConfigStore::new(paths.config_file)
        .load()
        .expect("persisted config");
    assert_eq!(persisted.default_source.as_deref(), Some("default"));
}

#[tokio::test]
async fn lock_contention_is_reported_before_any_transport_request() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let server = MockServer::start();
    let (manifest, _, _) = mount_snapshot(&server);
    let _lock = SyncLockGuard::acquire(&paths.lock_file).expect("first lock");
    let mut service = SyncService {
        paths: paths.clone(),
        catalog: catalog(&paths, [source("home", server.base_url())]),
        progress: Arc::new(RecordingProgress::default()),
        cancellation: CancellationToken::new(),
    };

    assert!(matches!(
        service.run(SyncRequest { source_name: None }).await,
        Err(AppError::SyncLocked)
    ));
    manifest.assert_calls(0);
}

#[tokio::test]
async fn cancellation_after_download_stops_before_local_restore() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let server = MockServer::start();
    let (manifest, database, skills) = mount_snapshot(&server);
    let cancellation = CancellationToken::new();
    let progress = Arc::new(CancelBeforeRestore {
        cancellation: cancellation.clone(),
        events: Mutex::new(Vec::new()),
    });
    let mut service = SyncService {
        paths: paths.clone(),
        catalog: catalog(&paths, [source("home", server.base_url())]),
        progress,
        cancellation,
    };

    assert!(matches!(
        service.run(SyncRequest { source_name: None }).await,
        Err(AppError::Cancelled)
    ));
    manifest.assert_calls(1);
    database.assert_calls(1);
    skills.assert_calls(1);
    assert!(!paths.cc_switch_dir.join("cc-switch.db").exists());
    assert!(!paths.state_file.exists());
}

#[test]
fn redirected_cli_prints_stage_lines_summary_and_exit_codes() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let server = MockServer::start();
    let (manifest, database, skills) = mount_snapshot(&server);
    let _catalog = catalog(&paths, [source("home", server.base_url())]);

    let mut command = Command::cargo_bin("cc-switchy").expect("binary");
    command
        .env("CC_SWITCHY_TEST_HOME", home.path())
        .env("CC_SWITCH_TEST_HOME", home.path())
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG")
        .args(["--sync", "--lang", "en"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("Sync succeeded"))
        .stdout(predicate::str::contains("Source: home"))
        .stderr(predicate::str::contains("Acquiring the sync lock"))
        .stderr(predicate::str::contains("Downloading db.sql"))
        .stderr(predicate::str::contains("Applying providers"));
    manifest.assert_calls(1);
    database.assert_calls(1);
    skills.assert_calls(1);

    let _lock = SyncLockGuard::acquire(&paths.lock_file).expect("held lock");
    let mut locked = Command::cargo_bin("cc-switchy").expect("binary");
    locked
        .env("CC_SWITCHY_TEST_HOME", home.path())
        .env("CC_SWITCH_TEST_HOME", home.path())
        .args(["--sync", "--lang", "en"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Another sync or restore operation is already running",
        ));
}

#[test]
fn cli_sync_returns_zero_when_every_projection_succeeds() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let server = MockServer::start();
    let valid_database = String::from_utf8(DATABASE.to_vec())
        .expect("fixture SQL")
        .replace(
            r#"{"api_key":"fixture-only"}"#,
            r#"{"auth":{},"config":"model = \"gpt-5\"\n"}"#,
        )
        .into_bytes();
    let manifest = manifest_for(&valid_database, SKILLS);
    let (manifest_mock, database_mock, skills_mock) =
        mount_snapshot_bytes(&server, &manifest, &valid_database, SKILLS);
    let _catalog = catalog(&paths, [source("home", server.base_url())]);

    let mut command = Command::cargo_bin("cc-switchy").expect("binary");
    command
        .env("CC_SWITCHY_TEST_HOME", home.path())
        .env("CC_SWITCH_TEST_HOME", home.path())
        .args(["--sync", "--lang", "en"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Warnings: 0"));
    manifest_mock.assert_calls(1);
    database_mock.assert_calls(1);
    skills_mock.assert_calls(1);
}
