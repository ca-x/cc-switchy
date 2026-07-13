# cc-switchy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Build a bilingual, download-only Rust CLI and Ratatui application that restores CC Switch v2 snapshots from named WebDAV or S3 sources, safely applies provider/MCP/Skills state to local Agents, and ships cross-platform including MUSL.

**Architecture:** A thin Clap entry point selects the TUI, wizard, or non-interactive sync. All interfaces call the same application services: versioned configuration, read-only remote clients, a validated staging/restore transaction, and Agent projection adapters. Progress and errors carry typed bilingual message keys so CLI and TUI behavior cannot drift.

**Tech Stack:** Rust 1.95, edition 2021, Clap 4.5, Ratatui 0.30, Crossterm 0.29, Tokio 1, Reqwest 0.12 with Rustls/webpki roots, Rusqlite bundled SQLite, Serde/TOML/JSON/YAML, zip 8 with pure-Rust Deflate, SHA-256/HMAC, tempfile, fs2, assert_cmd, httpmock, and GitHub Actions with cross.

## Global Constraints

- Remote behavior is cloud-to-local only. Do not add upload, delete, MKCOL, or bucket mutation APIs.
- Preserve CC Switch protocol format cc-switch-webdav-sync, protocol v2, db-v6, manifest.json, db.sql, and skills.zip.
- cc-switchy configuration lives under ~/.cc-switchy; compatible database and managed Skills remain under ~/.cc-switch or the receiver's configured ~/.agents/skills SSOT.
- An explicit sync always re-fetches and re-applies the current snapshot from the selected source.
- Preserve device-local ~/.cc-switch/settings.json values and the local-only database tables named in the design.
- All user-visible CLI, Wizard, TUI, progress, and error text must exist in Simplified Chinese and English.
- Secrets must be redacted before they enter progress events, errors, logs, tests, or rendered widgets.
- Tests must use an isolated temporary home through CC_SWITCHY_TEST_HOME and CC_SWITCH_TEST_HOME.
- Pin Rust 1.95. Use Rustls and bundled SQLite so x86_64/aarch64 MUSL builds remain static.
- No provider/Skills behavior may be claimed compatible without a fixture or integration test derived from the reference implementation.
- Every commit must follow the repository Lore commit protocol.

## Planned File Map

    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    README.md
    THIRD_PARTY_NOTICES.md
    src/main.rs
    src/lib.rs
    src/cli.rs
    src/error.rs
    src/i18n.rs
    src/paths.rs
    src/progress.rs
    src/config/{mod.rs,model.rs,store.rs,catalog.rs}
    src/remote/{mod.rs,protocol.rs,webdav.rs,s3.rs}
    src/restore/{mod.rs,archive.rs,backup.rs,database.rs,schema.rs,service.rs}
    src/agent/{mod.rs,model.rs,paths.rs,settings.rs,repository.rs,mcp.rs,skills.rs}
    src/agent/provider/{mod.rs,claude.rs,codex.rs,gemini.rs,additive.rs,claude_desktop.rs,hermes.rs}
    src/commands/{mod.rs,sync.rs,tui.rs,wizard.rs}
    src/tui/{mod.rs,app.rs,event.rs,keymap.rs,view.rs,wizard.rs}
    tests/{cli_first_run.rs,config_catalog.rs,protocol_compat.rs,webdav_sync.rs,s3_sync.rs,restore_transaction.rs,agent_projection.rs,tui_render.rs}
    tests/fixtures/cc-switch-v2/{manifest.json,db.sql,skills.zip}
    .github/workflows/{ci.yml,release.yml}

---

### Task 1: Bootstrap the crate, bilingual messages, and CLI routing

**Files:**
- Create: Cargo.toml
- Create: rust-toolchain.toml
- Create: src/main.rs
- Create: src/lib.rs
- Create: src/cli.rs
- Create: src/error.rs
- Create: src/i18n.rs
- Create: src/paths.rs
- Create: tests/cli_first_run.rs

**Interfaces:**
- Produces: Language::{Auto,ZhCn,EnUs}, MessageKey, Translator::text(MessageKey, &MessageArgs), Cli, RunMode, AppPaths::discover(), AppError.
- Consumes: none.

- [x] **Step 1: Add failing CLI and language tests**

Create tests/cli_first_run.rs with tests that launch the binary in an isolated home:

    use assert_cmd::Command;
    use predicates::prelude::*;
    use tempfile::TempDir;

    fn command(home: &TempDir) -> Command {
        let mut cmd = Command::cargo_bin("cc-switchy").expect("binary");
        cmd.env("CC_SWITCHY_TEST_HOME", home.path())
            .env("CC_SWITCH_TEST_HOME", home.path())
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .env_remove("LANG");
        cmd
    }

    #[test]
    fn sync_without_configuration_prints_english_wizard_guidance() {
        let home = TempDir::new().expect("temp home");
        command(&home)
            .args(["--sync", "--lang", "en"])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("Run: cc-switchy --wizard"));
    }

    #[test]
    fn sync_without_configuration_prints_chinese_wizard_guidance() {
        let home = TempDir::new().expect("temp home");
        command(&home)
            .args(["--sync", "--lang", "zh"])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("请运行：cc-switchy --wizard"));
    }

    #[test]
    fn wizard_and_sync_are_mutually_exclusive() {
        let home = TempDir::new().expect("temp home");
        command(&home)
            .args(["--wizard", "--sync"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn help_is_rendered_in_the_requested_language() {
        let home = TempDir::new().expect("temp home");
        command(&home)
            .args(["--lang", "zh", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("用法"));
        command(&home)
            .args(["--lang", "en", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage"));
    }

- [x] **Step 2: Run the tests and confirm the binary is absent**

Run:

    cargo test --test cli_first_run

Expected: FAIL because Cargo.toml and the cc-switchy binary do not exist.

- [x] **Step 3: Create the minimal crate and pinned toolchain**

Create rust-toolchain.toml:

    [toolchain]
    channel = "1.95"
    components = ["rustfmt", "clippy"]
    profile = "minimal"

Create Cargo.toml with package version 0.1.0, edition 2021, rust-version 1.95, a lib target, and the binary. Start with these dependencies:

    [dependencies]
    clap = { version = "4.5", features = ["derive"] }
    crossterm = "0.29"
    dirs = "6"
    ratatui = "0.30"
    serde = { version = "1", features = ["derive"] }
    serde_json = "1"
    thiserror = "2"
    toml = "0.9"

    [dev-dependencies]
    assert_cmd = "2"
    predicates = "3"
    tempfile = "3"

    [profile.release]
    codegen-units = 1
    lto = "thin"
    opt-level = "s"
    strip = "symbols"

Create AppPaths with a test override:

    #[derive(Debug, Clone)]
    pub struct AppPaths {
        pub home: std::path::PathBuf,
        pub app_dir: std::path::PathBuf,
        pub config_file: std::path::PathBuf,
        pub state_file: std::path::PathBuf,
        pub lock_file: std::path::PathBuf,
        pub staging_dir: std::path::PathBuf,
        pub backups_dir: std::path::PathBuf,
        pub cc_switch_dir: std::path::PathBuf,
    }

    impl AppPaths {
        pub fn discover() -> Result<Self, crate::error::AppError> {
            let home = std::env::var_os("CC_SWITCHY_TEST_HOME")
                .map(std::path::PathBuf::from)
                .or_else(dirs::home_dir)
                .ok_or(crate::error::AppError::HomeDirectoryUnavailable)?;
            Ok(Self {
                app_dir: home.join(".cc-switchy"),
                config_file: home.join(".cc-switchy").join("config.toml"),
                state_file: home.join(".cc-switchy").join("state.json"),
                lock_file: home.join(".cc-switchy").join("lock"),
                staging_dir: home.join(".cc-switchy").join("staging"),
                backups_dir: home.join(".cc-switchy").join("backups"),
                cc_switch_dir: home.join(".cc-switch"),
                home,
            })
        }

        pub fn from_home(home: impl AsRef<std::path::Path>) -> Self;
    }

- [x] **Step 4: Implement typed bilingual messages and CLI routing**

Create Language, Translator, and message keys for first-run guidance, generic failures, help labels, and mode names. Resolution precedence must be CLI override, CC_SWITCHY_LANG, persisted preference when available, LC_ALL/LC_MESSAGES/LANG, then English.

Use these public signatures:

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub enum Language { Auto, ZhCn, EnUs }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum MessageKey {
        NoSourceConfigured,
        RunWizard,
        UnexpectedError,
    }

    #[derive(Debug, Default)]
    pub struct MessageArgs(pub std::collections::BTreeMap<&'static str, String>);

    pub struct Translator { language: Language }

    impl Translator {
        pub fn new(language: Language) -> Self;
        pub fn language(&self) -> Language;
        pub fn text(&self, key: MessageKey, args: &MessageArgs) -> String;
    }

Create Cli and RunMode:

    #[derive(clap::Parser, Debug)]
    #[command(name = "cc-switchy", version, disable_help_subcommand = true)]
    pub struct Cli {
        #[arg(long, conflicts_with = "sync")]
        pub wizard: bool,
        #[arg(long, conflicts_with = "wizard")]
        pub sync: bool,
        #[arg(long)]
        pub source: Option<String>,
        #[arg(long, value_parser = ["zh", "en"])]
        pub lang: Option<String>,
    }

    pub enum RunMode {
        Tui { source: Option<String> },
        Wizard,
        Sync { source: Option<String> },
    }

Build Clap's Command through CommandFactory so headings, descriptions, argument help, usage, and validation messages use the selected Translator. The temporary mode handlers may return NoSourceConfigured; do not initialize raw terminal mode yet.

- [x] **Step 5: Run the focused tests and static checks**

Run:

    cargo test --test cli_first_run
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Expected: all pass.

- [x] **Step 6: Commit the bootstrap**

    git add Cargo.toml Cargo.lock rust-toolchain.toml src tests/cli_first_run.rs
    git commit -m "Establish a testable command boundary before adding sync behavior" -m "Constraint: Rust 1.95 and bilingual first-run guidance are part of the public contract." -m "Confidence: high" -m "Scope-risk: narrow" -m "Tested: cargo test --test cli_first_run; cargo fmt; cargo clippy"

### Task 2: Implement versioned source configuration and atomic CRUD

**Files:**
- Create: src/config/mod.rs
- Create: src/config/model.rs
- Create: src/config/store.rs
- Create: src/config/catalog.rs
- Modify: src/lib.rs
- Modify: src/i18n.rs
- Test: tests/config_catalog.rs

**Interfaces:**
- Consumes: AppPaths, Language, AppError.
- Produces: AppConfig, SourceConfig, WebDavConfig, S3Config, ConfigStore, SourceCatalog.

- [x] **Step 1: Write failing configuration round-trip and CRUD tests**

Create tests/config_catalog.rs covering unique source names, one default, add/edit/delete/test-independent CRUD, backup creation, Unix 0600 mode, and language persistence. The core fixture is:

    fn webdav(name: &str) -> cc_switchy::config::SourceConfig {
        cc_switchy::config::SourceConfig {
            name: name.to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            kind: cc_switchy::config::SourceKind::WebDav {
                webdav: cc_switchy::config::WebDavConfig {
                    base_url: "https://dav.example.test/root".to_string(),
                    username: "user".to_string(),
                    password: "secret".to_string(),
                },
            },
        }
    }

    #[test]
    fn first_source_becomes_default_and_round_trips() {
        let home = tempfile::TempDir::new().expect("home");
        let paths = cc_switchy::paths::AppPaths::from_home(home.path());
        let store = cc_switchy::config::ConfigStore::new(paths.config_file.clone());
        let mut catalog = cc_switchy::config::SourceCatalog::load(store).expect("load");
        catalog.add(webdav("home")).expect("add");
        assert_eq!(catalog.config().default_source.as_deref(), Some("home"));
        let loaded = cc_switchy::config::SourceCatalog::load(
            cc_switchy::config::ConfigStore::new(paths.config_file),
        ).expect("reload");
        assert_eq!(loaded.config().sources[0].name, "home");
    }

    #[test]
    fn duplicate_names_are_rejected_without_rewriting_config() {
        let home = tempfile::TempDir::new().expect("home");
        let paths = cc_switchy::paths::AppPaths::from_home(home.path());
        let store = cc_switchy::config::ConfigStore::new(paths.config_file.clone());
        let mut catalog = cc_switchy::config::SourceCatalog::load(store).expect("load");
        catalog.add(webdav("home")).expect("first add");
        let before = std::fs::read(&paths.config_file).expect("before bytes");
        assert!(catalog.add(webdav("home")).is_err());
        let after = std::fs::read(&paths.config_file).expect("after bytes");
        assert_eq!(after, before);
    }

    #[test]
    fn deleting_default_selects_the_requested_replacement() {
        let home = tempfile::TempDir::new().expect("home");
        let paths = cc_switchy::paths::AppPaths::from_home(home.path());
        let store = cc_switchy::config::ConfigStore::new(paths.config_file);
        let mut catalog = cc_switchy::config::SourceCatalog::load(store).expect("load");
        catalog.add(webdav("a")).expect("add a");
        catalog.add(webdav("b")).expect("add b");
        catalog.delete("a", Some("b")).expect("delete a");
        assert_eq!(catalog.config().default_source.as_deref(), Some("b"));
    }

- [x] **Step 2: Run the tests and confirm missing config types**

Run:

    cargo test --test config_catalog

Expected: FAIL because src/config does not exist.

- [x] **Step 3: Implement the versioned data model**

Use this exact public model:

    #[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    pub struct AppConfig {
        pub version: u32,
        #[serde(default)]
        pub language: Language,
        #[serde(default)]
        pub default_source: Option<String>,
        #[serde(default)]
        pub sources: Vec<SourceConfig>,
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    pub struct SourceConfig {
        pub name: String,
        #[serde(default = "default_remote_root")]
        pub remote_root: String,
        #[serde(default = "default_profile")]
        pub profile: String,
        #[serde(flatten)]
        pub kind: SourceKind,
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    #[serde(tag = "type", rename_all = "lowercase")]
    pub enum SourceKind {
        WebDav { webdav: WebDavConfig },
        S3 { s3: S3Config },
    }

If TOML serialization of the internally tagged enum does not produce the approved nested tables, implement custom Serialize/Deserialize with a private wire struct and lock the exact example in a golden test. Do not change the approved file format to fit a derive.

Credential-bearing configuration types must not derive Debug. Provide a RedactedSource view for diagnostics and TUI rendering.

- [x] **Step 4: Implement atomic storage**

ConfigStore must expose:

    pub struct ConfigStore { path: std::path::PathBuf }

    impl ConfigStore {
        pub fn new(path: std::path::PathBuf) -> Self;
        pub fn load(&self) -> Result<AppConfig, AppError>;
        pub fn save(&self, config: &AppConfig) -> Result<(), AppError>;
        pub fn exists(&self) -> bool;
    }

save() must validate before writing, create the parent directory, copy the existing file to config.toml.bak, write a sibling temporary file, fsync it, set mode 0600 on Unix, then atomically rename it over config.toml.

- [x] **Step 5: Implement SourceCatalog mutations**

SourceCatalog must persist only after a complete validation succeeds:

    impl SourceCatalog {
        pub fn load(store: ConfigStore) -> Result<Self, AppError>;
        pub fn config(&self) -> &AppConfig;
        pub fn add(&mut self, source: SourceConfig) -> Result<(), AppError>;
        pub fn update(&mut self, original_name: &str, source: SourceConfig) -> Result<(), AppError>;
        pub fn delete(&mut self, name: &str, replacement_default: Option<&str>) -> Result<(), AppError>;
        pub fn set_default(&mut self, name: &str) -> Result<(), AppError>;
        pub fn set_language(&mut self, language: Language) -> Result<(), AppError>;
        pub fn resolve(&self, explicit: Option<&str>) -> Result<&SourceConfig, AppError>;
    }

Normalize whitespace, reject blank or duplicate names, validate HTTP(S) WebDAV URLs, require S3 bucket/region/access key/secret, and default remote_root/profile after trimming.

- [x] **Step 6: Run tests and commit**

Run:

    cargo test --test config_catalog
    cargo test
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Expected: all pass.

Commit:

    git add src/config src/lib.rs src/i18n.rs tests/config_catalog.rs
    git commit -m "Make source selection durable before network access exists" -m "Constraint: Multiple named sources require one stable default and atomic secret-bearing storage." -m "Confidence: high" -m "Scope-risk: narrow" -m "Tested: configuration CRUD, round-trip, backup, permissions, fmt, clippy"

### Task 3: Define progress events and validate the CC Switch v2 protocol

**Files:**
- Create: src/progress.rs
- Create: src/remote/mod.rs
- Create: src/remote/protocol.rs
- Modify: src/error.rs
- Modify: src/i18n.rs
- Modify: src/lib.rs
- Create: tests/protocol_compat.rs
- Create: tests/fixtures/cc-switch-v2/manifest.json

**Interfaces:**
- Consumes: SourceConfig, Translator message keys.
- Produces: ProgressEvent, ProgressSink, SyncManifest, ArtifactMeta, RemoteLayout, ValidatedManifest.

- [x] **Step 1: Add failing manifest and progress tests**

The tests must parse a committed manifest with format cc-switch-webdav-sync, version 2, dbCompatVersion 6, db.sql and skills.zip metadata. Add cases for wrong format, wrong protocol, current db version mismatch, legacy WebDAV db-v5 fallback, missing artifacts, 1 MiB manifest limit, and 512 MiB artifact limit.

Use these assertions:

    let manifest = SyncManifest::parse(&bytes)?;
    let validated = manifest.validate(RemoteLayout::Current)?;
    assert_eq!(validated.db_compat_version(), 6);
    assert_eq!(validated.artifact("db.sql")?.size, 123);

    assert!(matches!(
        SyncManifest::parse(&oversized),
        Err(AppError::ManifestTooLarge { .. })
    ));

Also assert that Downloading events carry downloaded and total byte counts and never carry source credentials.

- [x] **Step 2: Run the focused tests**

Run:

    cargo test --test protocol_compat

Expected: FAIL because the protocol and progress modules do not exist.

- [x] **Step 3: Implement the event contract**

Use:

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ProgressEvent {
        Locking,
        Connecting { source: String },
        FetchingManifest,
        ValidatingManifest,
        Downloading { artifact: String, downloaded: u64, total: u64 },
        Verifying { artifact: String },
        PreparingLocalBackup,
        RestoringSkills,
        ImportingDatabase,
        ApplyingProvider { agent: String },
        ApplyingMcp { agent: String },
        ApplyingSkills { agent: String, completed: usize, total: usize },
        Retrying { operation: String, attempt: u8, max_attempts: u8 },
        Warning { stage: String, agent: Option<String>, message_key: MessageKey, detail: String },
        Completed { duration_ms: u128, snapshot_id: String },
        Failed { stage: String, message_key: MessageKey, detail: String, retryable: bool },
    }

    pub trait ProgressSink: Send + Sync {
        fn emit(&self, event: ProgressEvent);
    }

Provide NoopProgress and ChannelProgress implementations. Ensure secrets are not fields in any event variant.

- [x] **Step 4: Implement protocol parsing and integrity checks**

Port the constants and algorithms from reference sync_protocol.rs:

    pub const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
    pub const PROTOCOL_VERSION: u32 = 2;
    pub const DB_COMPAT_VERSION: u32 = 6;
    pub const LEGACY_DB_COMPAT_VERSION: u32 = 5;
    pub const REMOTE_DB_SQL: &str = "db.sql";
    pub const REMOTE_SKILLS_ZIP: &str = "skills.zip";
    pub const REMOTE_MANIFEST: &str = "manifest.json";
    pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
    pub const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

Add sha2 = "0.10" and hex = "0.4" in this task.

Implement parse(), validate(), artifact(), verify_artifact(), and sha256_hex(). snapshotId validation must sort artifact names, concatenate name:sha256 with |, hash the bytes, and compare to manifest.snapshot_id.

- [x] **Step 5: Run tests and commit**

Run:

    cargo test --test protocol_compat
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add src/progress.rs src/remote src/error.rs src/i18n.rs src/lib.rs tests/protocol_compat.rs tests/fixtures
    git commit -m "Reject incompatible snapshots before transport can affect local state" -m "Constraint: Remote artifacts are untrusted and CC Switch v2 compatibility is exact." -m "Confidence: high" -m "Scope-risk: narrow" -m "Tested: manifest compatibility, limits, hashes, progress data, fmt, clippy"

### Task 4: Build the read-only WebDAV client

**Files:**
- Create: src/remote/webdav.rs
- Modify: src/remote/mod.rs
- Modify: Cargo.toml
- Modify: src/error.rs
- Modify: src/i18n.rs
- Test: tests/webdav_sync.rs

**Interfaces:**
- Consumes: WebDavConfig, SourceConfig, ProgressSink, ValidatedManifest.
- Produces: WebDavClient::test_connection(), WebDavClient::fetch_snapshot().

- [x] **Step 1: Add failing mock WebDAV tests**

Use httpmock with routes for the current manifest, legacy manifest, db.sql, and skills.zip. Assert:

- base URL paths are retained;
- the current db-v6 path is requested first;
- legacy fallback occurs only after a current manifest 404;
- Basic Auth is present;
- artifacts stream progress and match declared bytes;
- 401/403 do not retry;
- 429/5xx retry at most three times;
- no request uses PUT, POST, DELETE, or MKCOL.

The success test should call:

    let snapshot = WebDavClient::new(source, reqwest::Client::new(), progress)
        .fetch_snapshot(&staging_dir)
        .await?;
    assert_eq!(snapshot.manifest.snapshot_id, expected_snapshot_id);
    assert_eq!(std::fs::read(snapshot.db_sql_path)?, expected_db);

- [x] **Step 2: Run the focused test**

Run:

    cargo test --test webdav_sync

Expected: FAIL because WebDavClient and Reqwest are absent.

- [x] **Step 3: Add MUSL-safe HTTP dependencies**

Add:

    futures-util = "0.3"
    reqwest = { version = "0.12", default-features = false, features = ["rustls-tls-webpki-roots", "json", "stream"] }
    tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time"] }
    url = "2.5"

Add httpmock as a dev-dependency. Verify cargo tree -i native-tls returns no packages.

- [x] **Step 4: Implement safe URL construction and read-only requests**

WebDavClient must expose:

    pub struct DownloadedSnapshot {
        pub manifest: ValidatedManifest,
        pub manifest_bytes: Vec<u8>,
        pub layout: RemoteLayout,
        pub db_sql_path: std::path::PathBuf,
        pub skills_zip_path: std::path::PathBuf,
    }

    impl WebDavClient {
        pub fn new(source: SourceConfig, client: reqwest::Client, progress: std::sync::Arc<dyn ProgressSink>) -> Result<Self, AppError>;
        pub async fn test_connection(&self) -> Result<Option<ValidatedManifest>, AppError>;
        pub async fn fetch_snapshot(&self, staging: &std::path::Path) -> Result<DownloadedSnapshot, AppError>;
    }

Join path segments through url::Url path_segments_mut(), never string concatenation. Use GET for manifest/artifacts and an optional Depth: 0 PROPFIND only for connection diagnostics. Stream each response into a newly created staging file while enforcing the declared limit, emitting Downloading events, fsyncing, then validating size and SHA-256.

Retry only connection errors, 429, and 5xx. Redact userinfo and query values from every URL stored in errors.

- [x] **Step 5: Verify read-only behavior and commit**

Run:

    cargo test --test webdav_sync
    cargo tree -i native-tls
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Expected: tests pass; cargo tree reports package ID specification native-tls did not match any packages.

Commit:

    git add Cargo.toml Cargo.lock src/remote src/error.rs src/i18n.rs tests/webdav_sync.rs
    git commit -m "Make WebDAV restoration observable without granting remote write capability" -m "Constraint: Existing base paths and CC Switch legacy WebDAV layout must remain readable." -m "Rejected: Add MKCOL or PUT for connection tests | the product is download-only." -m "Confidence: high" -m "Scope-risk: narrow" -m "Tested: mock WebDAV paths, auth, retries, progress, read-only verbs, fmt, clippy"

### Task 5: Build the read-only S3 SigV4 client

**Files:**
- Create: src/remote/s3.rs
- Modify: src/remote/mod.rs
- Modify: Cargo.toml
- Modify: src/error.rs
- Test: tests/s3_sync.rs

**Interfaces:**
- Consumes: S3Config, SourceConfig, ProgressSink, protocol validation.
- Produces: S3Client::test_connection(), S3Client::fetch_snapshot(), canonical SigV4 request helpers.

- [x] **Step 1: Add failing SigV4 and mock S3 tests**

Cover:

- AWS virtual-hosted URLs for empty endpoint;
- custom endpoint path-style URLs;
- bare custom endpoint defaulting to HTTPS;
- object key remote_root/v2/db-v6/profile/artifact;
- AWS canonical request and signature test vector;
- GET manifest then GET both artifacts;
- byte progress, limits, and SHA-256;
- 404 remote-empty, auth failure without retry, and transient retry;
- no PUT, POST, DELETE, or multipart requests.

Use a fixed clock in tests so Authorization and x-amz-date are deterministic.

- [x] **Step 2: Run the focused test**

Run:

    cargo test --test s3_sync

Expected: FAIL because S3Client and signing helpers do not exist.

- [x] **Step 3: Add cryptographic dependencies and credential redaction**

Add:

    chrono = { version = "0.4", features = ["clock", "serde"] }
    hmac = "0.12"

Define S3Credentials as a private type without Debug. Implement a redacted source description that exposes only source name, bucket, region, safe endpoint, and a partially masked Access Key ID.

- [x] **Step 4: Implement SigV4 GET/HEAD signing**

Use:

    pub trait Clock: Send + Sync {
        fn now(&self) -> chrono::DateTime<chrono::Utc>;
    }

    impl S3Client {
        pub fn new(source: SourceConfig, client: reqwest::Client, progress: std::sync::Arc<dyn ProgressSink>) -> Result<Self, AppError>;
        pub async fn test_connection(&self) -> Result<Option<ValidatedManifest>, AppError>;
        pub async fn fetch_snapshot(&self, staging: &std::path::Path) -> Result<DownloadedSnapshot, AppError>;
    }

Canonicalize method, URI, sorted query, lowercase headers, signed-header names, and payload hash exactly per AWS SigV4. The credential scope is date/region/s3/aws4_request. Sign GET and HEAD only. Stream downloads through the same bounded artifact writer used by WebDAV.

- [x] **Step 5: Run tests and commit**

Run:

    cargo test --test s3_sync
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add Cargo.toml Cargo.lock src/remote/s3.rs src/remote/mod.rs src/error.rs tests/s3_sync.rs
    git commit -m "Read S3-compatible snapshots without importing an SDK or write surface" -m "Constraint: AWS, MinIO, and R2 require compatible SigV4 endpoint styles while MUSL size stays small." -m "Rejected: AWS SDK | unnecessary binary size and configuration surface for three read operations." -m "Confidence: high" -m "Scope-risk: moderate" -m "Tested: SigV4 vectors, endpoint styles, mock downloads, retry rules, fmt, clippy"

### Task 6: Restore archives and databases transactionally

**Files:**
- Create: src/restore/mod.rs
- Create: src/restore/archive.rs
- Create: src/restore/backup.rs
- Create: src/restore/database.rs
- Create: src/restore/schema.rs
- Create: src/restore/service.rs
- Modify: Cargo.toml
- Modify: src/error.rs
- Modify: src/progress.rs
- Test: tests/restore_transaction.rs
- Create: tests/fixtures/cc-switch-v2/db.sql
- Create: tests/fixtures/cc-switch-v2/skills.zip

**Interfaces:**
- Consumes: DownloadedSnapshot, AppPaths, ProgressSink.
- Produces: PreparedSkills, PreparedDatabase, LocalBackup, SyncLockGuard, RestoreOutcome, RestoreService::apply().

- [x] **Step 1: Add failing restore and rollback tests**

Create a minimal CC Switch-compatible SQL fixture with the required header, providers table, mcp_servers table, settings table, skills table, and one provider. Create a ZIP containing one SKILL.md.

Tests must verify:

- invalid SQL header leaves local files unchanged;
- ZIP ../ traversal, absolute paths, more than 10,000 entries, and more than 512 MiB declared output are rejected;
- SQL executes in a temporary database before live replacement;
- local-only tables are copied from the existing database;
- provider_health is not copied;
- legacy db-v5 schema is migrated to the current supported shape;
- a durable backup contains database, Skills, and metadata;
- forced database replacement failure restores the original Skills;
- missing ~/.cc-switch is created only after all remote validation passes.

The primary test calls:

    let outcome = RestoreService::new(paths.clone(), progress)
        .apply(downloaded_snapshot)
        .expect("restore");
    assert!(paths.cc_switch_dir.join("cc-switch.db").exists());
    assert_eq!(std::fs::read_to_string(paths.cc_switch_dir.join("skills/demo/SKILL.md"))?, "# Demo");
    assert!(outcome.backup_dir.join("metadata.json").exists());

- [x] **Step 2: Run the focused test**

Run:

    cargo test --test restore_transaction

Expected: FAIL because restore modules are absent.

- [x] **Step 3: Add static-link-safe archive and SQLite dependencies**

Add:

    fs2 = "0.4"
    rusqlite = { version = "0.40", default-features = false, features = ["bundled", "backup"] }
    tempfile = "3"
    zip = { version = "8", default-features = false, features = ["deflate-flate2-zlib-rs"] }

Use zip only for Stored and Deflated entries. Do not enable bzip2, zstd, xz, or system zlib.

- [x] **Step 4: Implement safe archive preparation**

Implement:

    pub struct PreparedSkills {
        pub extracted_dir: tempfile::TempDir,
        pub entry_count: usize,
        pub total_bytes: u64,
    }

    pub fn prepare_skills(zip_path: &std::path::Path) -> Result<PreparedSkills, AppError>;

For every entry, use enclosed_name(), reject unsafe paths and link escapes, enforce count/size before writing, create only parents beneath the extraction root, and fsync completed files needed for the subsequent rename/copy.

- [x] **Step 5: Implement temporary database validation and local-table preservation**

Implement:

    pub struct PreparedDatabase {
        pub file: tempfile::NamedTempFile,
    }

    pub fn prepare_database(
        sql_path: &std::path::Path,
        existing_db: Option<&std::path::Path>,
    ) -> Result<PreparedDatabase, AppError>;

Check the export header, execute the SQL in the temporary database, verify required tables, verify providers or mcp_servers has at least one row, and copy these tables from the existing database when both schemas contain them:

    proxy_request_logs
    stream_check_logs
    proxy_live_backup
    usage_daily_rollups

Do not copy provider_health.

Port the schema creation and migrations needed to accept the current db-v6 and WebDAV legacy db-v5 exports into restore/schema.rs. Run migrations only on the prepared temporary database, then validate the current required columns and indexes before live replacement.

- [x] **Step 6: Implement durable backup and rollback**

LocalBackup::create() must copy the existing database if present, recursively copy the current Skills SSOT without following links outside the root, and write metadata.json containing timestamp, source, snapshot ID, and original paths. RestoreService::apply() must:

1. prepare Skills and database completely;
2. require a borrowed SyncLockGuard already acquired by the orchestration layer;
3. create the backup;
4. replace Skills;
5. copy the prepared SQLite database into the live database through rusqlite Backup;
6. restore Skills from backup if step 5 fails;
7. clean staging after success or handled failure.

Define SyncLockGuard in this task:

    pub struct SyncLockGuard {
        file: std::fs::File,
    }

    impl SyncLockGuard {
        pub fn acquire(path: &Path) -> Result<Self, AppError>;
    }

Its file handle holds an fs2 exclusive lock until Drop.

- [x] **Step 7: Run tests and commit**

Run:

    cargo test --test restore_transaction
    cargo test
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add Cargo.toml Cargo.lock src/restore src/error.rs src/progress.rs tests/restore_transaction.rs tests/fixtures/cc-switch-v2
    git commit -m "Keep local state recoverable when a cloud snapshot is authoritative" -m "Constraint: Database and Skills cross a shared failure boundary and remote archives are untrusted." -m "Confidence: high" -m "Scope-risk: broad" -m "Directive: Never move validation after the first live filesystem mutation." -m "Tested: archive limits, SQL validation, local table preservation, backups, rollback, fmt, clippy"

### Task 7: Model CC Switch Agents, device settings, and database queries

**Files:**
- Create: src/agent/mod.rs
- Create: src/agent/model.rs
- Create: src/agent/paths.rs
- Create: src/agent/settings.rs
- Create: src/agent/repository.rs
- Modify: src/lib.rs
- Modify: src/error.rs
- Test: tests/agent_projection.rs

**Interfaces:**
- Consumes: AppPaths and the restored cc-switch.db.
- Produces: Agent, Provider, ProviderMeta, McpServer, InstalledSkill, DeviceSettings, AgentRepository, ProjectionReport.

- [x] **Step 1: Add failing repository and device-setting tests**

Seed a temporary database with providers, mcp_servers, skills, settings, and two providers per exclusive Agent. Assert:

    let repo = AgentRepository::open(&db_path)?;
    let codex = repo.providers(Agent::Codex)?;
    assert_eq!(codex.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["sorted", "fallback"]);
    assert_eq!(repo.database_current_provider(Agent::Codex)?.as_deref(), Some("fallback"));

Add settings fixtures for a valid local current provider, a stale local current provider, custom Agent config directories, skillStorageLocation, and skillSyncMethod. Assert valid local selection wins and stale selection falls back to the database.

- [x] **Step 2: Run the focused test**

Run:

    cargo test --test agent_projection repository

Expected: FAIL because the Agent domain does not exist.

- [x] **Step 3: Implement the domain types**

Use:

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub enum Agent {
        Claude,
        ClaudeDesktop,
        Codex,
        Gemini,
        OpenCode,
        OpenClaw,
        Hermes,
    }

    impl Agent {
        pub const ALL: [Agent; 7];
        pub fn db_key(self) -> &'static str;
        pub fn is_additive(self) -> bool;
        pub fn supports_mcp(self) -> bool;
        pub fn supports_skills(self) -> bool;
    }

    pub struct Provider {
        pub id: String,
        pub app: Agent,
        pub name: String,
        pub settings_config: serde_json::Value,
        pub sort_index: Option<i64>,
        pub created_at: Option<i64>,
        pub meta: ProviderMeta,
        pub is_current: bool,
    }

    pub struct ProjectionWarning {
        pub stage: ProjectionStage,
        pub agent: Option<Agent>,
        pub message: String,
    }

    #[derive(Default)]
    pub struct ProjectionReport {
        pub applied_agents: Vec<Agent>,
        pub skipped_agents: Vec<Agent>,
        pub warnings: Vec<ProjectionWarning>,
    }

ProviderMeta must deserialize unknown fields without failure and expose commonConfigEnabled and liveConfigManaged helpers.

- [x] **Step 4: Implement read-only database queries**

AgentRepository opens the restored SQLite database and exposes:

    pub fn providers(&self, agent: Agent) -> Result<Vec<Provider>, AppError>;
    pub fn provider(&self, agent: Agent, id: &str) -> Result<Option<Provider>, AppError>;
    pub fn database_current_provider(&self, agent: Agent) -> Result<Option<String>, AppError>;
    pub fn set_database_current_provider(&mut self, agent: Agent, id: &str) -> Result<(), AppError>;
    pub fn mcp_servers(&self) -> Result<Vec<McpServer>, AppError>;
    pub fn installed_skills(&self) -> Result<Vec<InstalledSkill>, AppError>;
    pub fn setting(&self, key: &str) -> Result<Option<String>, AppError>;

Provider ordering must be:

    ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC

Use parameterized SQL exclusively.

- [x] **Step 5: Implement device-local settings and paths**

DeviceSettings must load ~/.cc-switch/settings.json as a serde_json::Map so unknown keys survive writes. Expose:

    pub fn current_provider(&self, agent: Agent) -> Option<&str>;
    pub fn set_current_provider(&mut self, agent: Agent, id: Option<&str>);
    pub fn config_dir(&self, agent: Agent, home: &Path) -> PathBuf;
    pub fn skills_ssot(&self, home: &Path) -> PathBuf;
    pub fn skill_sync_method(&self) -> SkillSyncMethod;
    pub fn save_atomic(&self, path: &Path) -> Result<(), AppError>;

Implement default and override paths from the approved design. Linux Claude Desktop must return Unsupported instead of inventing a path.

- [x] **Step 6: Implement effective current-provider resolution**

Use:

    pub fn effective_current_provider(
        repo: &AgentRepository,
        settings: &mut DeviceSettings,
        agent: Agent,
    ) -> Result<Option<Provider>, AppError>;

If the local settings ID exists in the restored provider set, return it. If it is stale, clear and atomically persist it, then use the database is_current provider. Do not write is_current during restore projection.

- [x] **Step 7: Run tests and commit**

Run:

    cargo test --test agent_projection repository
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add src/agent src/lib.rs src/error.rs tests/agent_projection.rs
    git commit -m "Separate device choices from restored cloud state" -m "Constraint: Valid local current-provider IDs and path overrides must survive every cloud restore." -m "Confidence: high" -m "Scope-risk: moderate" -m "Tested: repository ordering, current-provider fallback, path overrides, fmt, clippy"

### Task 8: Project providers and support safe manual switching

**Files:**
- Create: src/agent/provider/mod.rs
- Create: src/agent/provider/claude.rs
- Create: src/agent/provider/codex.rs
- Create: src/agent/provider/gemini.rs
- Create: src/agent/provider/additive.rs
- Create: src/agent/provider/claude_desktop.rs
- Create: src/agent/provider/hermes.rs
- Create: resources/codex_native_responses_template.json
- Create: resources/gpt5_5_template.json
- Modify: src/agent/mod.rs
- Modify: Cargo.toml
- Test: tests/agent_projection.rs
- Create: tests/fixtures/projection/provider-cases.json

**Interfaces:**
- Consumes: AgentRepository, DeviceSettings, Agent paths, ProgressSink.
- Produces: ProviderProjector::project_all(), project_agent(), switch_exclusive().

- [x] **Step 1: Add failing provider golden tests**

Create fixture providers for every Agent, including:

- two exclusive providers with a local-current and database-current mismatch;
- additive providers with liveConfigManaged true, absent, and false;
- Codex auth/config/catalog output;
- Gemini env/settings output;
- OpenCode/OpenClaw unknown live providers that must survive;
- Hermes unknown YAML keys that must survive;
- Claude Desktop official, direct, and proxy modes.

Assert:

1. restore projection does not backfill live files into the restored database;
2. exclusive Agents write only the effective current provider;
3. additive Agents merge all managed providers and retain unknown live entries;
4. liveConfigManaged=false is skipped;
5. Codex multi-file failure rolls all Codex files back;
6. Linux Claude Desktop is skipped;
7. Claude Desktop proxy mode becomes a warning because v1 has no proxy runtime.
8. commonConfigEnabled applies the stored common snippet only to supported providers.
9. a failed manual exclusive switch restores the original live files, device current ID, database current ID, and any backfilled provider data.

- [x] **Step 2: Run provider tests**

Run:

    cargo test --test agent_projection provider

Expected: FAIL because ProviderProjector does not exist.

- [x] **Step 3: Add format dependencies and shared atomic writer**

Add:

    indexmap = { version = "2", features = ["serde"] }
    serde_yaml = "0.9"
    toml_edit = "0.23"

Create an internal atomic_write(path, bytes) that writes a sibling temporary file, preserves existing Unix permissions when possible, fsyncs, and renames. Create a MultiFileBackup used by Codex and Claude Desktop so either all managed files update or all originals restore.

- [x] **Step 4: Implement restore-time provider projection**

Use:

    pub struct ProviderProjector<'a> {
        pub repo: &'a mut AgentRepository,
        pub settings: &'a mut DeviceSettings,
        pub paths: &'a AgentPaths,
        pub progress: std::sync::Arc<dyn ProgressSink>,
    }

    impl ProviderProjector<'_> {
        pub fn project_all(&mut self) -> ProjectionReport;
        pub fn project_agent(&mut self, agent: Agent) -> Result<(), AppError>;
        pub fn switch_exclusive(&mut self, agent: Agent, provider_id: &str) -> Result<(), AppError>;
    }

project_all() must iterate Agent::ALL, emit ApplyingProvider, continue after one Agent failure, and aggregate warnings. For exclusive Agents, resolve the effective current provider and write it. For additive Agents, merge all providers except liveConfigManaged=false.

Adapt the writer semantics from these reference files at commit c6197ae32450cd70e2bf03b35e3f5f53ac12044c:

    services/provider/live.rs
    codex_config.rs
    gemini_config.rs
    opencode_config.rs
    openclaw_config.rs
    hermes_config.rs
    claude_desktop_config.rs

Do not port proxy takeover, hot-switch, usage, failover, session, OMO, or Tauri state.

- [x] **Step 5: Implement manual exclusive switching separately**

switch_exclusive() is used only by the TUI. It must validate the provider, snapshot the affected database rows/settings/live files, backfill the prior live provider only for compatible normal mode, update device-local current provider, update database is_current, write the target live files, and rely on Task 9 to re-project that Agent's MCP. Any failure before completion restores the snapshot so the original provider remains current.

Restore-time project_all() must never call switch_exclusive(), because backfill would contaminate the newly restored database.

- [x] **Step 6: Preserve upstream attribution**

Add file-level comments to substantially adapted modules and stage THIRD_PARTY_NOTICES.md content naming CC Switch, its MIT license, upstream copyright, the pinned reference commit, and the adapted behavior.

- [x] **Step 7: Run tests and commit**

Run:

    cargo test --test agent_projection provider
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add Cargo.toml Cargo.lock src/agent/provider src/agent/mod.rs resources tests/fixtures/projection THIRD_PARTY_NOTICES.md
    git commit -m "Apply restored providers without feeding local drift back into the snapshot" -m "Constraint: Restore projection and manual provider switching require different state transitions." -m "Rejected: Reuse the full CC Switch switch path for restore | live backfill would mutate restored data." -m "Confidence: medium" -m "Scope-risk: broad" -m "Directive: Provider projection must remain before MCP projection, especially for Codex config.toml." -m "Tested: per-Agent golden output, additive preservation, Codex rollback, proxy warning, fmt, clippy"

### Task 9: Project MCP servers after providers

**Files:**
- Create: src/agent/mcp.rs
- Modify: src/agent/mod.rs
- Modify: src/agent/provider/mod.rs
- Modify: src/progress.rs
- Test: tests/agent_projection.rs
- Create: tests/fixtures/projection/mcp-cases.json

**Interfaces:**
- Consumes: AgentRepository::mcp_servers(), AgentPaths, provider-written live files.
- Produces: McpProjector::project_all(), project_agent().

- [x] **Step 1: Add failing MCP preservation tests**

Seed stdio, HTTP, and SSE MCP servers with per-Agent enabled flags. Seed live files with one unknown user-managed MCP and one known disabled MCP. Assert:

- Claude updates ~/.claude.json;
- Codex updates config.toml mcp_servers after provider projection;
- Gemini updates settings.json.mcpServers;
- OpenCode converts to local/remote format;
- Hermes preserves unrelated YAML fields;
- Claude Desktop and OpenClaw are skipped;
- known disabled entries are removed;
- unknown live entries are retained;
- one corrupt Agent config creates a warning while other Agents still apply.

- [x] **Step 2: Run MCP tests**

Run:

    cargo test --test agent_projection mcp

Expected: FAIL because McpProjector does not exist.

- [x] **Step 3: Implement MCP projection adapters**

Use:

    pub struct McpProjector<'a> {
        pub repo: &'a AgentRepository,
        pub paths: &'a AgentPaths,
        pub progress: std::sync::Arc<dyn ProgressSink>,
    }

    impl McpProjector<'_> {
        pub fn project_all(&self) -> ProjectionReport;
        pub fn project_agent(&self, agent: Agent) -> Result<(), AppError>;
    }

Adapt only projection and validation behavior from reference services/mcp.rs and mcp/{claude,codex,gemini,opencode,hermes}.rs. Do not port MCP import, CRUD, discovery, or Tauri commands.

- [x] **Step 4: Wire manual provider switching to re-project MCP**

After ProviderProjector::switch_exclusive() writes the target provider, call McpProjector::project_agent(agent). If MCP fails, keep the provider switch and return a structured warning suitable for TUI Activity.

- [x] **Step 5: Run tests and commit**

Run:

    cargo test --test agent_projection mcp
    cargo test --test agent_projection provider
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add src/agent/mcp.rs src/agent/mod.rs src/agent/provider/mod.rs src/progress.rs tests
    git commit -m "Restore MCP after provider files settle" -m "Constraint: Codex provider writes replace config.toml sections, so MCP must be projected afterward." -m "Confidence: medium" -m "Scope-risk: broad" -m "Tested: per-Agent MCP golden output, unknown-entry preservation, partial warnings, fmt, clippy"

### Task 10: Reconcile managed Skills to each Agent

**Files:**
- Create: src/agent/skills.rs
- Modify: src/agent/mod.rs
- Modify: src/agent/model.rs
- Test: tests/agent_projection.rs
- Create: tests/fixtures/projection/skills/

**Interfaces:**
- Consumes: installed Skills rows, DeviceSettings Skills SSOT/method, Agent paths.
- Produces: SkillProjector::project_all(), project_agent().

- [x] **Step 1: Add failing Skills reconciliation tests**

Create fixtures for enabled and disabled managed Skills, an orphan symlink pointing into the SSOT, an unrelated ordinary directory, Auto/Symlink/Copy methods, forced symlink failure, a Skill missing SKILL.md, and Claude Desktop/OpenClaw capability skips.

Assert disabled and orphaned managed targets are removed, unrelated directories remain, Auto falls back to copy, copy uses temp-then-rename, and one bad Skill creates a warning without preventing other Skills or Agents.

- [x] **Step 2: Run Skills tests**

Run:

    cargo test --test agent_projection skills

Expected: FAIL because SkillProjector does not exist.

- [x] **Step 3: Implement safe Skills projection**

Use:

    pub struct SkillProjector<'a> {
        pub repo: &'a AgentRepository,
        pub settings: &'a DeviceSettings,
        pub paths: &'a AgentPaths,
        pub progress: std::sync::Arc<dyn ProgressSink>,
    }

    impl SkillProjector<'_> {
        pub fn project_all(&self) -> ProjectionReport;
        pub fn project_agent(&self, agent: Agent) -> Result<(), AppError>;
    }

Port only SyncMethod, SkillStorageLocation, SSOT validation, symlink/copy, orphan cleanup, and sync_to_app semantics from reference services/skill.rs. Never follow a source symlink outside the SSOT. Preserve ordinary unmanaged directories.

- [x] **Step 4: Run tests and commit**

Run:

    cargo test --test agent_projection skills
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

Commit:

    git add src/agent/skills.rs src/agent/mod.rs src/agent/model.rs tests
    git commit -m "Reconcile managed Skills without claiming ownership of user directories" -m "Constraint: SSOT links may be cleaned, but ordinary unmanaged directories must survive." -m "Confidence: high" -m "Scope-risk: moderate" -m "Tested: enable/disable, orphan cleanup, symlink/copy fallback, partial warnings, fmt, clippy"

### Task 11: Orchestrate sync, progress, locking, and CLI results

**Files:**
- Create: src/commands/mod.rs
- Create: src/commands/sync.rs
- Modify: src/remote/mod.rs
- Modify: src/restore/service.rs
- Modify: src/main.rs
- Modify: src/cli.rs
- Modify: src/i18n.rs
- Modify: src/progress.rs
- Create: tests/sync_end_to_end.rs
- Modify: tests/cli_first_run.rs

**Interfaces:**
- Consumes: SourceCatalog, WebDavClient/S3Client, RestoreService, ProviderProjector, McpProjector, SkillProjector.
- Produces: SyncService::run(), SyncRequest, SyncOutcome, CLI TTY/non-TTY progress renderer.

- [x] **Step 1: Add failing end-to-end and exit-code tests**

Test default source resolution, explicit --source override without persistence, fresh manifest/artifact fetch on consecutive syncs, global lock rejection before transport, Provider/MCP/Skills event ordering, exit codes 0/1/2, redirected line progress, and Ctrl+C before restore.

- [x] **Step 2: Run focused tests**

Run:

    cargo test --test sync_end_to_end

Expected: FAIL because SyncService is absent.

- [x] **Step 3: Acquire the existing global lock at the orchestration boundary**

Use SyncLockGuard from Task 6. Acquire it before connecting to any source and hold it through cleanup and projection. RestoreService::apply() receives &SyncLockGuard and must not acquire another lock.

- [x] **Step 4: Implement the remote-client enum and sync pipeline**

Use:

    pub struct SyncRequest {
        pub source_name: Option<String>,
    }

    pub struct SyncOutcome {
        pub snapshot_id: String,
        pub backup_dir: PathBuf,
        pub projection: ProjectionReport,
        pub duration: Duration,
    }

    pub struct SyncService {
        pub paths: AppPaths,
        pub catalog: SourceCatalog,
        pub progress: Arc<dyn ProgressSink>,
        pub cancellation: CancellationToken,
    }

    impl SyncService {
        pub async fn run(&mut self, request: SyncRequest) -> Result<SyncOutcome, AppError>;
    }

run() must resolve the source, acquire the lock, create unique staging, fetch and verify the current snapshot, restore it, reload repository/settings, project Provider then MCP then Skills, aggregate warnings, persist last-sync state, emit Completed, and clean staging.

Add tokio-util = { version = "0.7", features = ["rt"] } for CancellationToken.

- [x] **Step 5: Implement CLI progress and result rendering**

TTY rendering uses a compact spinner and byte bars without alternate screen. Redirected stderr renders one line per stage and throttles Downloading lines. Final stdout is bilingual and includes source, snapshot, duration, counts, warnings, and backup path.

- [x] **Step 6: Run tests and commit**

Run:

    cargo test --test sync_end_to_end
    cargo test --test cli_first_run
    cargo test
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings

Commit:

    git add src/commands src/remote/mod.rs src/restore/service.rs src/main.rs src/cli.rs src/i18n.rs src/progress.rs tests
    git commit -m "Make one explicit command carry a snapshot through every safe boundary" -m "Constraint: A source lock must cover download, restore, projection, and cleanup across both transports." -m "Confidence: high" -m "Scope-risk: broad" -m "Tested: end-to-end sync, fresh fetch, lock contention, event ordering, exit codes, cancellation, fmt, clippy"

### Task 12: Build the Ratatui main UI and shared Wizard

**Files:**
- Create: src/commands/tui.rs
- Create: src/commands/wizard.rs
- Create: src/tui/mod.rs
- Create: src/tui/app.rs
- Create: src/tui/event.rs
- Create: src/tui/keymap.rs
- Create: src/tui/view.rs
- Create: src/tui/wizard.rs
- Modify: src/main.rs
- Modify: src/i18n.rs
- Modify: src/config/catalog.rs
- Create: tests/tui_render.rs

**Interfaces:**
- Consumes: SourceCatalog, SyncService progress channel, AgentRepository, ProviderProjector, McpProjector.
- Produces: default TUI, Providers/Skills/Activity/Sources views, shared Wizard CRUD, terminal guard.

- [x] **Step 1: Add failing reducer and TestBackend rendering tests**

Use Ratatui TestBackend at 140x36, 100x30, 70x24, and 50x15. Assert:

- no-source empty state contains cc-switchy --wizard in both languages;
- 140 columns renders Agents, Providers, and Details;
- 100 columns renders two panes;
- 70 columns uses single-pane navigation;
- 50x15 renders a resize message;
- ] changes Agent from Claude to Codex without modifying provider state;
- Enter on an exclusive provider emits SwitchProvider;
- Enter on an additive Agent emits ReapplyProviders;
- 4 opens Sources;
- s on Sources syncs selected without changing default;
- m changes default through SourceCatalog;
- L switches language immediately;
- per-Agent cursor/scroll state survives navigation;
- progress view shows stage, bytes, elapsed time, and log;
- warning view lists failed Agents and retry action.

- [x] **Step 2: Run focused tests**

Run:

    cargo test --test tui_render

Expected: FAIL because TUI modules are absent.

- [x] **Step 3: Implement pure TUI state and actions**

Use:

    pub enum MainView { Providers, Skills, Activity, Sources }
    pub enum FocusPane { Agents, List, Details, Activity }
    pub enum TuiAction {
        Quit,
        SwitchView(MainView),
        Move(i32),
        FocusNext,
        FocusPrevious,
        PreviousAgent,
        NextAgent,
        SwitchProvider { agent: Agent, provider_id: String },
        ReapplyProviders { agent: Agent },
        SyncSource { source: String },
        TestSource { source: String },
        MakeDefault { source: String },
        OpenWizard,
        ChangeLanguage(Language),
        RetryWarnings,
    }

App::update(action) must be pure except for queuing commands. Store a cursor and scroll offset per Agent. High-frequency navigation changes state immediately with no animation or artificial delay.

Persist last view, selected Agent/source, pane, and per-Agent cursors to state.json using atomic mode-0600 writes. Corrupt state.json must fall back to defaults without blocking startup.

- [x] **Step 4: Implement responsive rendering**

Render the approved four views and key hints. Use one accent color plus semantic green/yellow/red, but always include glyph/text status. Mask passwords and secrets; partially mask Access Key IDs. The progress view consumes ProgressEvent values and keeps bounded scrollback.

- [x] **Step 5: Implement the shared Wizard state machine**

WizardState supports List, Details, ChooseType, EditWebDav, EditS3, ConfirmDelete, ChooseReplacementDefault, TestConnection, and LanguageSelect. Form passwords are masked. Each confirmed mutation calls SourceCatalog immediately. Esc discards the active form only. q exits standalone Wizard or returns to the TUI.

- [x] **Step 6: Implement safe terminal lifecycle and background commands**

Create TerminalGuard that enables raw mode, enters alternate screen, hides the cursor, and restores all three in Drop and the panic hook. Run sync/test/switch commands on Tokio tasks and send results/progress to the event loop. During local replacement cancellation is deferred; before that boundary Esc/Ctrl+C cancels and cleans staging.

- [x] **Step 7: Run tests and commit**

Run:

    cargo test --test tui_render
    cargo test
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings

Commit:

    git add src/commands src/tui src/main.rs src/i18n.rs src/config/catalog.rs tests/tui_render.rs
    git commit -m "Keep routine Agent work immediate while making destructive state visible" -m "Constraint: Keyboard navigation must not animate, and every sync mutation needs durable progress feedback." -m "Confidence: high" -m "Scope-risk: broad" -m "Tested: responsive TestBackend layouts, keymap, wizard CRUD, language switching, terminal guard, fmt, clippy"

### Task 13: Document operation, attribution, and security limits

**Files:**
- Create: README.md
- Complete: THIRD_PARTY_NOTICES.md
- Modify: docs/superpowers/specs/2026-07-13-cc-switchy-design.md
- Create: tests/readme_commands.rs

**Interfaces:**
- Consumes: final CLI behavior and supported target list.
- Produces: bilingual usage documentation, snapshot security warning, verified command examples.

- [x] **Step 1: Add a documentation command smoke test**

Create tests/readme_commands.rs that asserts README contains and the binary accepts:

    cc-switchy
    cc-switchy --wizard
    cc-switchy --sync
    cc-switchy --sync --source backup-s3
    cc-switchy --lang zh
    cc-switchy --lang en

Also assert README names WebDAV, S3, db.sql secret exposure, exit codes 0/1/2, ~/.cc-switchy, ~/.cc-switch, and the six release targets.

- [x] **Step 2: Write bilingual README sections**

Include installation, first run, Wizard keys, TUI keys, source TOML example, sync semantics, backup/rollback, language selection, Agent capability matrix, exit codes, private CA limitation for webpki roots, GNU glibc baseline guidance, MUSL recommendation, and snapshot-secret warning.

- [x] **Step 3: Complete attribution**

THIRD_PARTY_NOTICES.md must reproduce the upstream MIT notice and identify pinned reference commit c6197ae32450cd70e2bf03b35e3f5f53ac12044c. List protocol, database restore, provider projection, MCP projection, and Skills behavior as adapted areas.

- [x] **Step 4: Run tests and commit**

Run:

    cargo test --test readme_commands
    cargo test

Commit:

    git add README.md THIRD_PARTY_NOTICES.md docs tests/readme_commands.rs
    git commit -m "Make restore authority and compatibility limits visible to operators" -m "Constraint: Cloud snapshots and local config contain plaintext secrets that documentation must not obscure." -m "Confidence: high" -m "Scope-risk: narrow" -m "Tested: README command smoke test and full cargo test"

### Task 14: Add CI, static MUSL verification, and tagged releases

**Files:**
- Create: .github/workflows/ci.yml
- Create: .github/workflows/release.yml
- Modify: README.md

**Interfaces:**
- Consumes: Cargo.lock, Rust 1.95 toolchain, release targets.
- Produces: required checks and six-target archives with SHA256SUMS.

- [x] **Step 1: Add CI workflow**

ci.yml must contain:

- Ubuntu quality job: cargo fmt --all -- --check; cargo clippy --all-targets --all-features -- -D warnings; cargo test --all-features --locked.
- Native release-build matrix for x86_64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-apple-darwin, and x86_64-pc-windows-msvc.
- x86_64-unknown-linux-musl smoke build using cross.
- file and readelf -d assertions showing the MUSL binary has no dynamic NEEDED entries.
- execution of target/x86_64-unknown-linux-musl/release/cc-switchy --version.

Pin actions/checkout, dtolnay/rust-toolchain, and cross installation to stable explicit versions. Do not create Cross.toml in v1.

- [x] **Step 2: Add tag release workflow**

release.yml triggers on v* tags with fail-fast false and these targets:

    x86_64-unknown-linux-gnu
    x86_64-unknown-linux-musl
    aarch64-unknown-linux-musl
    x86_64-apple-darwin
    aarch64-apple-darwin
    x86_64-pc-windows-msvc

Use cargo for native targets and cross for both MUSL targets. Package Unix targets as tar.gz and Windows as zip. Every archive contains binary, README.md, LICENSE, and THIRD_PARTY_NOTICES.md. A final Ubuntu job downloads all archives, runs sha256sum, writes SHA256SUMS, and publishes with softprops/action-gh-release.

- [x] **Step 3: Validate workflows locally**

Run:

    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features --locked
    cargo build --locked --release
    actionlint .github/workflows/ci.yml .github/workflows/release.yml

If Docker and cross are available, also run:

    cross build --locked --release --target x86_64-unknown-linux-musl
    file target/x86_64-unknown-linux-musl/release/cc-switchy
    readelf -d target/x86_64-unknown-linux-musl/release/cc-switchy

Expected: no NEEDED dynamic entries. If local Docker/cross is unavailable, record that exact gap and rely on CI; do not claim local MUSL verification.

- [x] **Step 4: Commit workflows**

    git add .github/workflows README.md
    git commit -m "Make every supported binary reproducible before publishing" -m "Constraint: Linux MUSL targets require cross because ring and bundled SQLite need target C toolchains." -m "Rejected: Maintain cargo-zigbuild and cross simultaneously | duplicate release paths increase drift." -m "Confidence: high" -m "Scope-risk: moderate" -m "Tested: fmt, clippy, tests, native release build, actionlint; MUSL evidence recorded separately"

### Task 15: Run full compatibility, security, and release-readiness verification

**Files:**
- Modify only files required by failures found in this task.
- Update: docs/superpowers/plans/2026-07-13-cc-switchy.md checkboxes.

**Interfaces:**
- Consumes: complete application.
- Produces: verified first release candidate with no known errors.

- [x] **Step 1: Generate and pin a reference compatibility fixture**

Using the pinned CC Switch checkout, generate or verify manifest.json, db.sql, and skills.zip for the committed fixture. Record upstream commit, SHA-256 values, and generation command in tests/fixtures/cc-switch-v2/README.md. The fixture must include providers, MCP, enabled/disabled Skills, and additive/exclusive Agents.

- [x] **Step 2: Run the full automated verification suite**

Run sequentially:

    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features --locked
    cargo build --locked --release
    cargo test --test restore_transaction -- --nocapture
    cargo test --test agent_projection -- --nocapture
    cargo test --test sync_end_to_end -- --nocapture
    cargo test --test tui_render -- --nocapture

Expected: all pass with zero ignored compatibility tests.

- [x] **Step 3: Run manual isolated-home smoke tests**

With a temporary directory exported as both CC_SWITCHY_TEST_HOME and CC_SWITCH_TEST_HOME:

1. Run --sync before config and confirm bilingual wizard guidance.
2. Run --wizard, add WebDAV and S3 sources, edit, test, set default, delete, and exit.
3. Run TUI in Chinese and English, navigate four views, switch Agents, switch an exclusive provider, and reapply an additive Agent.
4. Run --sync against a fixture source and inspect progress, result, backup, database, and projected files.
5. Force one Agent permission failure and confirm restore succeeds with exit code 2.
6. Tamper with db.sql and skills.zip and confirm local state remains byte-for-byte unchanged.

- [x] **Step 4: Inspect dependency and secret surfaces**

Run:

    cargo tree -d
    cargo tree -i native-tls
    rg -n "password|secret_access_key|Authorization|X-Amz-Signature" src tests

Review every match so secret fields are private/redacted and test values synthetic. Confirm native-tls is absent.

- [x] **Step 5: Review the diff against the approved design**

Check every acceptance criterion in docs/superpowers/specs/2026-07-13-cc-switchy-design.md against a passing test or manual evidence. Confirm no upload remote methods, proxy runtime, scheduler, provider CRUD, or other non-goal entered implementation.

- [x] **Step 6: Commit final verification fixes**

After fixing any failures:

    git add -A
    git commit -m "Close the first release only after compatibility evidence is complete" -m "Constraint: Completion requires CC Switch fixture compatibility, rollback evidence, bilingual UI, and release builds." -m "Confidence: high" -m "Scope-risk: broad" -m "Tested: full test suite, focused restore/projection/TUI tests, native release build, manual isolated-home smoke tests"
