# Configurable Backup Retention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a user-controlled backup switch and global retention limit, defaulting to enabled with the newest 10 recognized backups retained, then publish `v0.3.0`.

**Architecture:** Store one global `BackupConfig` in `AppConfig`, pass it from `SyncService` into `RestoreService`, and keep the existing restore transaction unchanged when backups are enabled. Retention is implemented next to `LocalBackup` so candidate recognition, metadata parsing, ordering, and deletion remain inside the backup safety boundary. The wizard edits a draft copy and persists it atomically through `SourceCatalog` only after an explicit disable confirmation.

**Tech Stack:** Rust 1.95, Serde/TOML/JSON, Chrono, Ratatui/Crossterm, Cargo integration tests, GitHub Actions.

## Global Constraints

- Do not use subagents for this implementation.
- Do not run a local release build; GitHub Actions builds all release targets.
- Backups default to `enabled = true` and `max_count = 10`.
- `max_count = 0` means unlimited retention; it does not disable backup creation.
- `enabled = false` creates no backup and provides no rollback.
- Retention is global across sources and runs only after creating the current backup and before live state mutation.
- Never delete unknown entries, files, symbolic links, malformed directory names, or directories with invalid `metadata.json`.
- The current backup is never a deletion candidate.
- Release version is exactly `0.3.0` and tag is exactly `v0.3.0`.

---

### Task 1: Persist the global backup policy

**Files:**
- Modify: `src/config/model.rs`
- Modify: `src/config/catalog.rs`
- Modify: `src/config/mod.rs`
- Modify: `tests/config_catalog.rs`

**Interfaces:**
- Produces: `pub struct BackupConfig { pub enabled: bool, pub max_count: usize }`.
- Produces: `impl Default for BackupConfig` with `true` and `10`.
- Produces: `pub backup: BackupConfig` on `AppConfig` with `#[serde(default)]`.
- Produces: `SourceCatalog::set_backup_config(&mut self, backup: BackupConfig)`.

- [x] **Step 1: Add configuration regression tests**

Add tests that load a legacy TOML without `[backup]`, persist disabled/unlimited and enabled/positive policies through the catalog, and assert the exact TOML values.

```rust
assert_eq!(loaded.config().backup, BackupConfig::default());
catalog.set_backup_config(BackupConfig { enabled: false, max_count: 0 })?;
assert!(!reloaded.config().backup.enabled);
assert_eq!(reloaded.config().backup.max_count, 0);
```

- [x] **Step 2: Run focused tests and verify red**

Run: `cargo test --test config_catalog backup -- --nocapture`

Expected: compilation fails because `BackupConfig` and `AppConfig::backup` do not exist.

- [x] **Step 3: Implement the model and atomic catalog setter**

Use Serde defaults so existing files remain valid, export `BackupConfig`, and commit a cloned `AppConfig` through the existing `SourceCatalog::commit` path.

- [x] **Step 4: Run focused tests and verify green**

Run: `cargo test --test config_catalog backup -- --nocapture`

Expected: all backup configuration tests pass.

### Task 2: Recognize and prune completed backups safely

**Files:**
- Modify: `src/restore/backup.rs`

**Interfaces:**
- Produces: owned `BackupMetadata` with `Serialize + Deserialize`.
- Produces: `LocalBackup::enforce_retention(&self, max_count: usize) -> Result<(), AppError>`.
- Consumes: the current `backup_dir`, which is excluded from deletion.

- [x] **Step 1: Add backup retention unit tests**

Cover positive limits, unlimited zero, deterministic oldest-first ordering, malformed directory names, invalid metadata, ordinary files, and Unix symbolic links. Build recognized fixtures with names matching `%Y%m%dT%H%M%S%.9fZ` and valid camelCase metadata.

- [x] **Step 2: Run focused tests and verify red**

Run: `cargo test --lib restore::backup::tests -- --nocapture`

Expected: compilation fails because retention support does not exist.

- [x] **Step 3: Implement recognition and deletion**

For every direct entry under the backup root:

1. call `symlink_metadata` and require a non-symlink directory;
2. require the directory name to parse as the cc-switchy timestamp plus optional numeric collision suffix;
3. read and deserialize `metadata.json` and parse `created_at` as RFC 3339;
4. sort recognized entries by parsed creation time and then directory name;
5. remove only the oldest entries needed to make the recognized total at most the positive maximum, never including the current directory.

Any read-directory, metadata, or deletion error for an entry that is being inspected or removed returns `AppError` before live restore begins. A zero maximum returns without scanning or deleting.

- [x] **Step 4: Run focused tests and verify green**

Run: `cargo test --lib restore::backup::tests -- --nocapture`

Expected: all retention safety tests pass.

### Task 3: Make backup and rollback optional in restore

**Files:**
- Modify: `src/restore/service.rs`
- Modify: `tests/restore_transaction.rs`

**Interfaces:**
- Changes: `RestoreOutcome.backup_dir: Option<PathBuf>`.
- Changes: `RestoreService::new(paths, progress, backup_config)`.
- Consumes: `BackupConfig` from Task 1.

- [x] **Step 1: Add enabled and disabled restore tests**

Keep existing rollback tests with `BackupConfig::default()`. Add a successful disabled restore asserting `backup_dir == None` and no backups root. Add a disabled failure case asserting the error contains `rollback unavailable because backups are disabled` and no backup was created.

- [x] **Step 2: Run focused tests and verify red**

Run: `cargo test --test restore_transaction backup -- --nocapture`

Expected: compilation fails until the constructor and outcome types change.

- [x] **Step 3: Implement the conditional transaction**

When enabled, emit `PreparingLocalBackup`, create a `LocalBackup`, enforce retention, and use it for the existing Skills/database rollback paths. When disabled, skip those operations and append the explicit rollback-unavailable explanation to Skills or database restore errors. Return `Some(path)` only for enabled backups.

- [x] **Step 4: Run restore tests and verify green**

Run: `cargo test --test restore_transaction -- --nocapture`

Expected: all restore and rollback tests pass.

### Task 4: Propagate optional backup results through sync and UI state

**Files:**
- Modify: `src/commands/sync.rs`
- Modify: `src/commands/tui.rs`
- Modify: `tests/sync_end_to_end.rs`

**Interfaces:**
- Changes: `SyncOutcome.backup_dir: Option<PathBuf>`.
- Changes: saved `lastSync.backupDir: Option<String>`.
- Consumes: `catalog.config().backup.clone()` when constructing `RestoreService`.

- [x] **Step 1: Add sync regression coverage**

Update enabled assertions to unwrap the backup path. Add a disabled end-to-end sync asserting no backup directory is created and `state.json` contains `"backupDir": null`.

- [x] **Step 2: Run focused tests and verify red**

Run: `cargo test --test sync_end_to_end backup -- --nocapture`

Expected: compilation or assertions fail until backup paths become optional.

- [x] **Step 3: Implement optional propagation and localized output**

Pass the policy into restore, serialize the optional path, and render the backup summary argument as either the path or a localized `Backup disabled; not created` / `备份已关闭；未创建` value. Update TUI test fixtures to use `Some(path)`.

- [x] **Step 4: Run focused tests and verify green**

Run: `cargo test --test sync_end_to_end -- --nocapture && cargo test --lib commands::tui::tests -- --nocapture`

Expected: all sync and TUI runtime tests pass.

### Task 5: Add wizard backup settings and disable confirmation

**Files:**
- Modify: `src/tui/wizard.rs`
- Modify: `src/commands/wizard.rs`
- Modify: `src/i18n.rs`
- Modify: `tests/tui_render.rs`

**Interfaces:**
- Adds: `WizardMode::BackupSettings` and `WizardMode::ConfirmDisableBackup`.
- Adds: `WizardAction::BackupSettings` and `WizardAction::ToggleBackup`.
- Adds: `WizardCommand::ChangeBackup(BackupConfig)`.
- Adds: `WizardState::new_with_backup(language, sources, default_source, backup)` while preserving the default-policy constructor.

- [x] **Step 1: Add interaction and rendering tests**

Verify `b` opens the settings screen, Space toggles the switch, only ASCII digits enter `max_count`, `0` renders as unlimited, disabling requires a second Enter confirmation, Cancel preserves the old policy, persistence failure keeps the draft visible, and successful persistence updates the active policy.

- [x] **Step 2: Run focused tests and verify red**

Run: `cargo test --test tui_render wizard_backup -- --nocapture`

Expected: compilation fails until the new wizard modes/actions/command exist.

- [x] **Step 3: Implement the draft form and command handling**

Keep `backup` and `backup_draft` in `WizardState`. Open with `b`, use Space to toggle, use Backspace/digits for the count, use Enter to save, and transition to the confirmation mode only for `true -> false`. `commands/wizard.rs` persists with `SourceCatalog::set_backup_config`; success replaces the active policy and returns to the list, while failure remains on the settings screen with the draft intact.

- [x] **Step 4: Add bilingual copy and render the settings**

Add message keys for the title, enabled/disabled values, maximum, unlimited explanation, rollback warning, confirmation warning, disabled summary, and settings/footer help. Render the two controls and explicit warning without exposing source credentials.

- [x] **Step 5: Run wizard tests and verify green**

Run: `cargo test --test tui_render wizard -- --nocapture`

Expected: all wizard interaction and rendering tests pass.

### Task 6: Document the policy and prepare version 0.3.0

**Files:**
- Modify: `README.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Produces: documented `[backup]` TOML contract and wizard shortcut.
- Produces: package and binary version `0.3.0`.

- [x] **Step 1: Update bilingual documentation**

Add `b` to wizard keys, add `[backup] enabled = true / max_count = 10` to the example, document `0 = unlimited`, explain that disabling backup also disables rollback, and state that excess recognized backups are pruned during the next enabled synchronization.

- [x] **Step 2: Update version metadata**

Change the root package version to `0.3.0`, run `cargo check` to update only the root lockfile entry, and verify `cargo metadata --no-deps --format-version 1` reports `0.3.0`.

- [x] **Step 3: Run README contract tests**

Run: `cargo test --test readme_commands -- --nocapture`

Expected: README command and safety contract tests pass.

### Task 7: Verify, commit, tag, push, and monitor release

**Files:**
- Verify all changed source, tests, docs, and version files.

- [x] **Step 1: Run local quality gates**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
cargo run --locked -- --version
```

Expected: all commands exit 0 and the binary prints `cc-switchy 0.3.0`. Do not run a local release build.

- [x] **Step 2: Review the release diff and version surfaces**

Review `v0.2.1..HEAD`, verify the worktree contains only intended files, verify no dependency additions, and confirm local/remote `v0.3.0` do not exist.

- [ ] **Step 3: Commit intended changes**

Create implementation and version commits with only the files named by this plan, then re-read `git status`, `git log`, and `HEAD`.

- [ ] **Step 4: Create and push the release tag**

Create annotated tag `v0.3.0`, push `main`, push the tag, and re-read remote refs. User authorization for these public actions is explicit in the current thread.

- [ ] **Step 5: Monitor GitHub Actions and verify release assets**

Wait for main CI, tag CI, and Release to complete successfully. Verify the stable GitHub Release contains six platform archives plus `SHA256SUMS`; download the GNU archive and checksum file, validate the checksum, archive contents, executable mode, and `cc-switchy 0.3.0` output.
