use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use cc_switchy::progress::NoopProgress;
use cc_switchy::remote::protocol::{
    compute_snapshot_id, sha256_hex, ArtifactMeta, RemoteLayout, SyncManifest, DB_COMPAT_VERSION,
    PROTOCOL_FORMAT, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_SKILLS_ZIP,
};
use cc_switchy::remote::DownloadedSnapshot;
use cc_switchy::restore::{prepare_database, prepare_skills, RestoreService, SyncLockGuard};
use cc_switchy::{AppError, AppPaths};
use rusqlite::Connection;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

const FIXTURE_SQL: &str = include_str!("fixtures/cc-switch-v2/db.sql");

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        writer.start_file(*name, options).expect("start zip entry");
        writer.write_all(bytes).expect("write zip entry");
    }
    writer.finish().expect("finish zip");
}

fn valid_snapshot(staging: &Path, sql: &[u8], zip_bytes: &[u8]) -> DownloadedSnapshot {
    fs::create_dir_all(staging).expect("staging directory");
    let db_sql_path = staging.join(REMOTE_DB_SQL);
    let skills_zip_path = staging.join(REMOTE_SKILLS_ZIP);
    fs::write(&db_sql_path, sql).expect("write db.sql");
    fs::write(&skills_zip_path, zip_bytes).expect("write skills.zip");

    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(sql),
            size: sql.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(zip_bytes),
            size: zip_bytes.len() as u64,
        },
    );
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: "fixture-host".to_string(),
        created_at: "2026-07-13T10:42:00Z".to_string(),
        snapshot_id: compute_snapshot_id(&artifacts),
        artifacts,
    };
    let manifest_bytes = serde_json::to_vec(&manifest).expect("manifest bytes");
    let manifest = manifest
        .validate(RemoteLayout::Current)
        .expect("valid manifest");

    DownloadedSnapshot {
        manifest,
        manifest_bytes,
        layout: RemoteLayout::Current,
        db_sql_path,
        skills_zip_path,
    }
}

fn create_skills_zip(temp: &TempDir) -> Vec<u8> {
    let path = temp.path().join("skills.zip");
    write_zip(&path, &[("demo/SKILL.md", b"# Demo")]);
    fs::read(path).expect("zip bytes")
}

fn seed_existing_state(paths: &AppPaths) {
    fs::create_dir_all(&paths.cc_switch_dir).expect("cc-switch dir");
    let db_path = paths.cc_switch_dir.join("cc-switch.db");
    let connection = Connection::open(&db_path).expect("existing database");
    connection
        .execute_batch(
            "CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY, model TEXT NOT NULL);
             INSERT INTO proxy_request_logs VALUES ('local-request', 'local-model');
             CREATE TABLE stream_check_logs (id INTEGER PRIMARY KEY, message TEXT NOT NULL);
             INSERT INTO stream_check_logs VALUES (1, 'local-stream');
             CREATE TABLE proxy_live_backup (app_type TEXT PRIMARY KEY, original_config TEXT NOT NULL, backed_up_at TEXT NOT NULL);
             INSERT INTO proxy_live_backup VALUES ('codex', '{}', 'now');
             CREATE TABLE usage_daily_rollups (date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL, PRIMARY KEY (date, app_type, provider_id, model));
             INSERT INTO usage_daily_rollups VALUES ('2026-07-13', 'codex', 'local', 'gpt');
             CREATE TABLE provider_health (provider_id TEXT NOT NULL, app_type TEXT NOT NULL, is_healthy INTEGER NOT NULL DEFAULT 1, PRIMARY KEY (provider_id, app_type));
             INSERT INTO provider_health VALUES ('local-health', 'codex', 1);",
        )
        .expect("seed existing database");
    let old_skill = paths.cc_switch_dir.join("skills/old/SKILL.md");
    fs::create_dir_all(old_skill.parent().expect("skill parent")).expect("old skill dir");
    fs::write(old_skill, "# Old").expect("old skill");
}

#[test]
fn rejects_unsafe_zip_paths_and_does_not_escape_staging() {
    for name in ["../escape", "/absolute/path", "C:\\escape"] {
        let temp = TempDir::new().expect("temp");
        let zip_path = temp.path().join("unsafe.zip");
        write_zip(&zip_path, &[(name, b"bad")]);

        let result = prepare_skills(&zip_path);
        match result {
            Err(AppError::ArchiveUnsafePath { .. }) => {}
            Err(error) => panic!("unsafe entry was misclassified: {name}: {error}"),
            Ok(_) => panic!("unsafe entry was accepted: {name}"),
        }
        assert!(!temp.path().join("escape").exists());
    }
}

#[test]
fn rejects_archives_with_too_many_entries() {
    let temp = TempDir::new().expect("temp");
    let zip_path = temp.path().join("many.zip");
    let file = fs::File::create(&zip_path).expect("zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for index in 0..=10_000 {
        writer
            .start_file(format!("{index}.txt"), options)
            .expect("entry");
    }
    writer.finish().expect("finish");

    assert!(matches!(
        prepare_skills(&zip_path),
        Err(AppError::ArchiveTooManyEntries { .. })
    ));
}

#[test]
fn rejects_archives_whose_declared_output_exceeds_the_limit() {
    let temp = TempDir::new().expect("temp");
    let zip_path = temp.path().join("oversized.zip");
    write_zip(&zip_path, &[("huge.bin", b"x")]);
    let mut bytes = fs::read(&zip_path).expect("zip bytes");
    patch_declared_uncompressed_size(&mut bytes, 512 * 1024 * 1024 + 1);
    fs::write(&zip_path, bytes).expect("patched zip");

    assert!(matches!(
        prepare_skills(&zip_path),
        Err(AppError::ArchiveExtractedTooLarge { .. })
    ));
}

#[test]
fn rejects_symbolic_link_entries() {
    let temp = TempDir::new().expect("temp");
    let zip_path = temp.path().join("symlink.zip");
    let file = fs::File::create(&zip_path).expect("zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer
        .add_symlink("linked-skill", "../../outside", options)
        .expect("symlink entry");
    writer.finish().expect("finish");

    assert!(matches!(
        prepare_skills(&zip_path),
        Err(AppError::ArchiveUnsafePath { .. })
    ));
}

#[test]
fn invalid_sql_is_rejected_without_creating_cc_switch_state() {
    let home = TempDir::new().expect("home");
    let staging = TempDir::new().expect("staging");
    let paths = AppPaths::from_home(home.path());
    let zip = create_skills_zip(&staging);
    let snapshot = valid_snapshot(staging.path(), b"DROP TABLE providers;", &zip);
    let lock = SyncLockGuard::acquire(&paths.lock_file).expect("lock");

    let error = RestoreService::new(paths.clone(), Arc::new(NoopProgress))
        .apply(snapshot, &lock, "fixture")
        .expect_err("invalid SQL must fail");

    assert!(matches!(error, AppError::InvalidSqlExport));
    assert!(!paths.cc_switch_dir.exists());
}

#[test]
fn database_preparation_preserves_local_tables_but_not_provider_health() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    seed_existing_state(&paths);
    let sql_path = home.path().join("db.sql");
    fs::write(&sql_path, FIXTURE_SQL).expect("fixture SQL");

    let prepared = prepare_database(&sql_path, Some(&paths.cc_switch_dir.join("cc-switch.db")))
        .expect("prepare database");
    let connection = Connection::open(prepared.file.path()).expect("prepared database");

    assert_eq!(
        connection
            .query_row("SELECT model FROM proxy_request_logs", [], |row| row
                .get::<_, String>(0))
            .expect("local request row"),
        "local-model"
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM provider_health", [], |row| row
                .get::<_, i64>(0))
            .expect("provider health count"),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| row
                .get::<_, i64>(0))
            .expect("stream logs"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM proxy_live_backup", [], |row| row
                .get::<_, i64>(0))
            .expect("live backup"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| row
                .get::<_, i64>(0))
            .expect("rollups"),
        1
    );
}

#[test]
fn sql_import_cannot_attach_or_modify_an_external_database() {
    let home = TempDir::new().expect("home");
    let sql_path = home.path().join("malicious.sql");
    let outside = home.path().join("outside.db");
    fs::write(
        &sql_path,
        format!(
            "-- CC Switch SQLite 导出\nATTACH DATABASE '{}' AS outside;\nCREATE TABLE outside.stolen (value TEXT);",
            outside.display()
        ),
    )
    .expect("malicious SQL");

    assert!(prepare_database(&sql_path, None).is_err());
    assert!(!outside.exists());
}

#[test]
fn legacy_database_shape_is_migrated_before_validation() {
    let home = TempDir::new().expect("home");
    let sql_path = home.path().join("legacy.sql");
    fs::write(
        &sql_path,
        "-- CC Switch SQLite 导出\n\
         CREATE TABLE providers (id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL, settings_config TEXT NOT NULL, is_current INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (id, app_type));\n\
         CREATE TABLE mcp_servers (id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL);\n\
         INSERT INTO providers VALUES ('legacy', 'claude', 'Legacy', '{}', 1);",
    )
    .expect("legacy SQL");

    let prepared = prepare_database(&sql_path, None).expect("migrate legacy database");
    let connection = Connection::open(prepared.file.path()).expect("prepared database");
    let columns = table_columns(&connection, "providers");
    assert!(columns.contains(&"meta".to_string()));
    assert!(columns.contains(&"sort_index".to_string()));
    assert!(table_columns(&connection, "skills").contains(&"enabled_hermes".to_string()));
}

#[test]
fn valid_snapshot_creates_durable_backup_and_restores_database_and_skills() {
    let home = TempDir::new().expect("home");
    let staging = TempDir::new().expect("staging");
    let paths = AppPaths::from_home(home.path());
    seed_existing_state(&paths);
    let zip = create_skills_zip(&staging);
    let snapshot = valid_snapshot(staging.path(), FIXTURE_SQL.as_bytes(), &zip);
    let lock = SyncLockGuard::acquire(&paths.lock_file).expect("lock");

    let outcome = RestoreService::new(paths.clone(), Arc::new(NoopProgress))
        .apply(snapshot, &lock, "fixture")
        .expect("restore");

    assert_eq!(
        fs::read_to_string(paths.cc_switch_dir.join("skills/demo/SKILL.md"))
            .expect("restored skill"),
        "# Demo"
    );
    assert!(outcome.backup_dir.join("metadata.json").exists());
    assert!(outcome.backup_dir.join("cc-switch.db").exists());
    assert!(outcome.backup_dir.join("skills/old/SKILL.md").exists());
    let connection =
        Connection::open(paths.cc_switch_dir.join("cc-switch.db")).expect("restored database");
    assert_eq!(
        connection
            .query_row(
                "SELECT name FROM providers WHERE id='remote-provider'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("remote provider"),
        "Remote Provider"
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| row
                .get::<_, i64>(0))
            .expect("preserved logs"),
        1
    );
}

#[test]
fn local_unified_skills_setting_selects_agents_ssot() {
    let home = TempDir::new().expect("home");
    let staging = TempDir::new().expect("staging");
    let paths = AppPaths::from_home(home.path());
    seed_existing_state(&paths);
    fs::write(
        paths.cc_switch_dir.join("settings.json"),
        r#"{"skillStorageLocation":"unified","unknownDeviceKey":true}"#,
    )
    .expect("settings");
    let unified_old = paths.home.join(".agents/skills/old/SKILL.md");
    fs::create_dir_all(unified_old.parent().expect("unified parent")).expect("unified dir");
    fs::write(&unified_old, "# Unified Old").expect("unified old");
    let zip = create_skills_zip(&staging);
    let snapshot = valid_snapshot(staging.path(), FIXTURE_SQL.as_bytes(), &zip);
    let lock = SyncLockGuard::acquire(&paths.lock_file).expect("lock");

    let outcome = RestoreService::new(paths.clone(), Arc::new(NoopProgress))
        .apply(snapshot, &lock, "fixture")
        .expect("restore unified");

    assert_eq!(outcome.skills_path, paths.home.join(".agents/skills"));
    assert_eq!(
        fs::read_to_string(paths.home.join(".agents/skills/demo/SKILL.md")).expect("unified skill"),
        "# Demo"
    );
    assert!(paths.cc_switch_dir.join("skills/old/SKILL.md").exists());
}

#[test]
fn sync_lock_rejects_a_second_holder() {
    let home = TempDir::new().expect("home");
    let paths = AppPaths::from_home(home.path());
    let _first = SyncLockGuard::acquire(&paths.lock_file).expect("first lock");

    assert!(matches!(
        SyncLockGuard::acquire(&paths.lock_file),
        Err(AppError::SyncLocked)
    ));
}

#[cfg(unix)]
#[test]
fn database_replacement_failure_rolls_skills_back() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().expect("home");
    let staging = TempDir::new().expect("staging");
    let paths = AppPaths::from_home(home.path());
    seed_existing_state(&paths);
    let db_path = paths.cc_switch_dir.join("cc-switch.db");
    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o444)).expect("readonly database");
    let zip = create_skills_zip(&staging);
    let snapshot = valid_snapshot(staging.path(), FIXTURE_SQL.as_bytes(), &zip);
    let lock = SyncLockGuard::acquire(&paths.lock_file).expect("lock");

    assert!(RestoreService::new(paths.clone(), Arc::new(NoopProgress))
        .apply(snapshot, &lock, "fixture")
        .is_err());
    assert_eq!(
        fs::read_to_string(paths.cc_switch_dir.join("skills/old/SKILL.md"))
            .expect("rolled back skill"),
        "# Old"
    );
    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o600)).expect("restore permissions");
}

fn patch_declared_uncompressed_size(bytes: &mut [u8], size: u32) {
    let encoded = size.to_le_bytes();
    for index in 0..bytes.len().saturating_sub(28) {
        if bytes[index..].starts_with(b"PK\x03\x04") {
            bytes[index + 22..index + 26].copy_from_slice(&encoded);
        }
        if bytes[index..].starts_with(b"PK\x01\x02") {
            bytes[index + 24..index + 28].copy_from_slice(&encoded);
        }
    }
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .expect("table info");
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("column names")
}
