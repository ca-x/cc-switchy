# Source Sync Status Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make successful source tests and remote-to-local syncs visibly finish on the Sources view, preserve their transient status across reloads, show the current Skill name during restore, preserve CC Switch 3.17.0 schema-v13 local usage semantics, and publish `v0.2.1`.

**Architecture:** Keep remote and restore semantics unchanged. Store transient feedback on each existing `ViewSource`, preserve it only for an identical `SourceConfig`, and update it at test/sync lifecycle boundaries. Add a provided `ProgressSink::emit_skill` compatibility method: external sinks receive the unchanged public event while built-in CLI/TUI sinks receive sanitized Skill display details. Keep sync protocol v2/db-v6 while adding the two schema-v13 `input_token_semantics` columns to cc-switchy's minimal local-table repair so older snapshot restores retain current-device usage semantics.

**Tech Stack:** Rust 1.95, Tokio, Ratatui/Crossterm, Cargo integration tests, GitHub Actions.

## Global Constraints

- Synchronization remains remote-to-local only; add no upload, merge, delete, or bucket-creation behavior.
- Do not persist test status across process restarts.
- Do not change the public `ProgressEvent` variant shape or the meaning of its `agent` field, and do not add public `MessageKey` variants.
- Reuse existing localized messages for testing, connecting, snapshot, warning count, and failures.
- Preserve credentials and request redaction; TOML parser source lines must not reach public CLI/TUI errors.
- Keep CC Switch sync compatibility at protocol v2/db-v6; schema-v13 adaptation is limited to local-only table columns and preservation.
- Release version is exactly `0.2.1` and tag is exactly `v0.2.1`.

---

### Task 1: Preserve per-source runtime status across application reloads

**Files:**
- Modify: `src/tui/app.rs`

**Interfaces:**
- Produces: `pub(crate) fn set_source_status(&mut self, source: &str, status: impl Into<String>)`.
- Produces: `pub(crate) fn preserve_source_statuses_from(&mut self, old: &App)`.
- Consumes: existing `ViewSource.status: Option<String>` and source names.

- [ ] **Step 1: Write failing unit tests**

Add tests in `src/tui/app.rs` that build old/new `App` values with sources named `home`, `renamed`, and `other`. Assert that an exact-name source inherits `Some("✓ Snapshot abc")`, a renamed source receives `None`, and `set_source_status` changes only the named source.

```rust
#[test]
fn source_statuses_survive_reload_only_for_exact_names() {
    let old = app_with_sources([("home", Some("✓ Snapshot abc")), ("old", Some("× failed"))]);
    let mut new = app_with_sources([("home", None), ("renamed", None)]);

    new.preserve_source_statuses_from(&old);

    assert_eq!(new.sources[0].status.as_deref(), Some("✓ Snapshot abc"));
    assert_eq!(new.sources[1].status, None);
}

#[test]
fn source_status_updates_only_the_requested_source() {
    let mut app = app_with_sources([("home", None), ("work", None)]);
    app.set_source_status("work", "Testing…");
    assert_eq!(app.sources[0].status, None);
    assert_eq!(app.sources[1].status.as_deref(), Some("Testing…"));
}
```

- [ ] **Step 2: Run the focused tests and verify red**

Run: `cargo test --lib source_status -- --nocapture`

Expected: compilation fails because the two `App` methods do not exist.

- [ ] **Step 3: Implement exact-name status preservation**

Use the existing `HashMap` import to collect cloned statuses by source name, copy them only to matching sources, and provide the named setter. Do not serialize these values.

```rust
pub(crate) fn set_source_status(&mut self, source: &str, status: impl Into<String>) {
    if let Some(item) = self.sources.iter_mut().find(|item| item.config.name == source) {
        item.status = Some(status.into());
    }
}

pub(crate) fn preserve_source_statuses_from(&mut self, old: &App) {
    let statuses = old.sources.iter().filter_map(|source| {
        source.status.clone().map(|status| (source.config.name.clone(), status))
    }).collect::<HashMap<_, _>>();
    for source in &mut self.sources {
        source.status = statuses.get(&source.config.name).cloned();
    }
}
```

- [ ] **Step 4: Run the focused tests and verify green**

Run: `cargo test --lib source_status -- --nocapture`

Expected: both focused tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tui/app.rs
git commit -m "Preserve source status across TUI reloads"
```

### Task 2: Make test and sync lifecycle results visible on Sources

**Files:**
- Modify: `src/commands/tui.rs`
- Modify: `tests/tui_render.rs`

**Interfaces:**
- Consumes: `App::set_source_status` and `App::preserve_source_statuses_from` from Task 1.
- Produces: `RuntimeMessage::SyncFinished { source: String, result: Result<SyncOutcome, AppError> }`.
- Produces: source statuses built only from existing localized `MessageKey` variants.

- [ ] **Step 1: Add failing lifecycle regression tests**

In the `src/commands/tui.rs` test module, construct an app whose progress is active, deliver `SourceTestFinished`, and assert that the global working state is cleared and the source contains the terminal result. Add a reload test that asserts an existing source status survives `reload_app`.

```rust
#[test]
fn source_test_completion_clears_working_state_and_sets_status() {
    let home = TempDir::new().expect("home");
    let paths = paths_with_source(&home, "home");
    let mut app = load_app(&paths, Language::EnUs, PersistedUiState::default()).expect("app");
    app.progress.active = true;
    let mut cancel = None;
    let mut started = None;

    handle_message(
        &paths,
        &mut app,
        RuntimeMessage::SourceTestFinished {
            source: "home".to_string(),
            result: Ok(SourceTestStatus::Snapshot("abc123".to_string())),
        },
        &mut cancel,
        &mut started,
    ).expect("message");

    assert!(!app.progress.active);
    assert_eq!(app.sources[0].status.as_deref(), Some("✓ Snapshot abc123"));
}
```

In `tests/tui_render.rs`, set a selected source status to a literal synchronized result and assert that the Sources details pane renders it instead of `Not tested`.

- [ ] **Step 2: Run focused tests and verify red**

Run: `cargo test source_test_completion_clears_working_state_and_sets_status -- --nocapture`

Expected: the assertion fails because source-test progress remains active.

- [ ] **Step 3: Implement lifecycle feedback**

Make these surgical changes in `src/commands/tui.rs`:

- set the source status to existing localized `WizardTesting` before spawning a test;
- use `NoopProgress` for source connection tests so they do not masquerade as a cancellable global sync;
- defensively clear `app.progress.active` and `active_started` on `SourceTestFinished`;
- set the source status to existing localized `ProgressConnecting` before sync starts;
- include the requested source name in `SyncFinished` even when the operation fails;
- on successful sync, call `reload_app`, then set the source status to `ActivitySnapshot` plus `ActivitySyncFinished`;
- on failed sync, set the source status and Activity entry to the same redacted error;
- update `reload_app` to call `preserve_source_statuses_from` before moving `old.progress`.

Do not add new `MessageKey` variants.

- [ ] **Step 4: Run focused lifecycle and rendering tests**

Run: `cargo test source_test_completion_clears_working_state_and_sets_status -- --nocapture`

Run: `cargo test --test tui_render source_operation_status_is_visible_in_details -- --nocapture`

Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add src/commands/tui.rs tests/tui_render.rs
git commit -m "Show source test and sync results in the TUI"
```

### Task 3: Display the current Skill name during restore

**Files:**
- Modify: `src/agent/skills.rs`
- Modify: `tests/agent_projection.rs`
- Modify: `tests/sync_end_to_end.rs`

**Interfaces:**
- Consumes: existing `ProgressEvent::ApplyingSkills { agent, completed, total }`.
- Produces: default `ProgressSink::emit_skill` forwarding the unchanged public event, with built-in CLI/TUI overrides rendering `<Agent> · <Skill display name>`.

- [ ] **Step 1: Add failing progress assertions**

Extend `consecutive_syncs_refetch_and_project_in_provider_mcp_skills_order` to require an event matching:

```rust
ProgressEvent::ApplyingSkills {
    agent,
    completed: 1,
    total: 1,
} if agent == "Codex · Demo"
```

Add a focused projection test with an empty Skill `name` and directory `good`, asserting an event label of `Codex · good`.

- [ ] **Step 2: Run focused tests and verify red**

Run: `cargo test --test sync_end_to_end consecutive_syncs_refetch_and_project_in_provider_mcp_skills_order -- --nocapture`

Expected: failure because the current label is only `Codex`.

- [ ] **Step 3: Implement the display label**

In `SkillProjector::project_agent`, select the trimmed display name unless it is blank, otherwise select `skill.directory`; sanitize controls and cap it at 80 characters plus ellipsis; then call `emit_skill` with the exact agent identity. The trait default forwards:

```rust
fn emit_skill(&self, agent: String, _skill: String, completed: usize, total: usize) {
    self.emit(ProgressEvent::ApplyingSkills {
        agent,
        completed,
        total,
    });
}
```

- [ ] **Step 4: Run both focused tests and verify green**

Run: `cargo test --test agent_projection skill_progress_uses_directory_when_display_name_is_blank -- --nocapture`

Run: `cargo test --test sync_end_to_end consecutive_syncs_refetch_and_project_in_provider_mcp_skills_order -- --nocapture`

Expected: both pass and event counts remain unchanged.

- [ ] **Step 5: Commit**

```bash
git add src/agent/skills.rs tests/agent_projection.rs tests/sync_end_to_end.rs
git commit -m "Show Skill names during restore progress"
```

### Task 4: Prepare version 0.2.1

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `README.md`

**Interfaces:**
- Produces: package and binary version `0.2.1`.
- Produces: version-independent Chinese private-CA limitation wording.

- [ ] **Step 1: Update release metadata**

Change the root package version in `Cargo.toml` from `0.2.0` to `0.2.1`. Change README line 181 to “当前版本不提供自定义 CA 导入参数” so future patch releases do not create stale documentation.

- [ ] **Step 2: Regenerate only the root lockfile package version**

Run: `cargo check`

Expected: `Cargo.lock` root `cc-switchy` package entry changes to `0.2.1`; dependency versions do not change.

- [ ] **Step 3: Verify release metadata**

Run: `cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "cc-switchy") | .version'`

Run: `rg -n 'v0\.2\.0|version = "0\.2\.0"' Cargo.toml README.md`

Expected: metadata prints `0.2.1`; the stale-version search returns no matches.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock README.md
git commit -m "Prepare cc-switchy v0.2.1"
```

### Task 5: Adapt CC Switch 3.17.0 schema-v13 local tables

**Files:**
- Modify: `src/restore/schema.rs`
- Modify: `tests/restore_transaction.rs`
- Modify: `tests/fixtures/cc-switch-v2/{README.md,db.sql,manifest.json}`
- Modify: `README.md`
- Modify: `THIRD_PARTY_NOTICES.md`

**Interfaces:**
- Consumes: CC Switch `v3.17.0` schema version 13 and unchanged sync protocol v2/db-v6.
- Produces: repaired `input_token_semantics` columns on `proxy_request_logs` and `usage_daily_rollups` without adding proxy/usage behavior.

- [ ] **Step 1: Compare the pinned upstream baseline with `v3.17.0`**

Verify that `SCHEMA_VERSION` changed from 12 to 13, the only schema additions are `input_token_semantics INTEGER NOT NULL DEFAULT 0` on the two local-only usage tables, and `DB_COMPAT_VERSION` remains 6.

- [ ] **Step 2: Repair and preserve the v13 columns**

Add both columns to minimal table creation and legacy-column repair. Extend preservation tests so a local v13 value survives restoration from a v12 snapshot.

- [ ] **Step 3: Refresh the compatibility fixture**

Set fixture `user_version=13`, add both columns, update manifest size/hash/snapshot ID, and pin the fixture documentation to CC Switch `v3.17.0` commit `3d176b98cc0bfd151a42882e88ab59b62083b92f`.

- [ ] **Step 4: Verify the focused compatibility paths**

```bash
cargo test --test protocol_compat committed_fixture -- --nocapture
cargo test --test restore_transaction cc_switch_v13_input_semantics_survive_an_older_snapshot_restore -- --nocapture
```

Expected: the v13 fixture matches its manifest and both local semantic values survive an older snapshot restore.

### Task 6: Verify, tag, push, and validate the release

**Files:**
- Verify: all changed files and generated release artifacts.
- Create tag: `v0.2.1`.
- Push: `main` and annotated tag `v0.2.1`.

**Interfaces:**
- Consumes: all prior task commits.
- Produces: GitHub stable Release `v0.2.1` with six archives and `SHA256SUMS`.

- [ ] **Step 1: Run repository gates**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
cargo build --release --locked
target/release/cc-switchy --version
```

Expected: all commands exit 0, all tests pass, and the binary prints `cc-switchy 0.2.1`.

- [ ] **Step 2: Run real configured-source PTY smoke**

Open `target/release/cc-switchy --lang zh`, press `4`, then `t`. Verify the selected source reaches a terminal snapshot/empty/error status and no false global working indicator remains. Press `s`, wait for completion, and verify the selected source retains a visible synchronized snapshot status. In Activity, verify Skill restore stages include both agent and Skill name.

- [ ] **Step 3: Review release diff and tag availability**

```bash
git status --short --branch -uall
git diff --check v0.2.0..HEAD
git log v0.2.0..HEAD --oneline
git tag --list v0.2.1
git ls-remote --tags origin refs/tags/v0.2.1
```

Expected: clean worktree, only approved design/plan/fix/version commits, and no existing local or remote `v0.2.1` tag.

- [ ] **Step 4: Create and verify the annotated tag**

```bash
git tag -a v0.2.1 -m "cc-switchy v0.2.1"
git show --stat --oneline v0.2.1
```

Expected: the tag dereferences to the verified release commit.

- [ ] **Step 5: Push branch and tag**

```bash
git push origin main
git push origin v0.2.1
git ls-remote origin refs/heads/main refs/tags/v0.2.1 'refs/tags/v0.2.1^{}'
```

Expected: remote `main` and the dereferenced tag point at the release commit.

- [ ] **Step 6: Wait for CI and verify published assets**

Use `gh run list --repo ca-x/cc-switchy --commit <release-commit>` until main CI, tag CI, and Release are completed successfully. Then verify `gh release view v0.2.1 --repo ca-x/cc-switchy` reports a non-draft, non-prerelease stable release with six platform archives and `SHA256SUMS`. Download `SHA256SUMS` and the x86_64 GNU archive, verify the GitHub asset digests, archive contents, executable mode, and `cc-switchy 0.2.1` binary output.
