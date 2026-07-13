# cc-switchy Design

**Date:** 2026-07-13  
**Status:** Release candidate verified locally; GitHub release pending
**Reference implementation:** `/home/czyt/code/others/cc-switch`

## 1. Product Definition

`cc-switchy` is a standalone Rust command-line application for restoring the
latest CC Switch cloud snapshot to the local machine. It is intentionally
one-way: cloud to local only. It provides a Ratatui interface by default, an
interactive source-management wizard, and a non-interactive one-command sync.

The tool does not require the CC Switch GUI or a running Tauri process. It does,
however, preserve CC Switch's remote protocol, local database layout, managed
Skills layout, provider switching semantics, and live Agent projections.

### Goals

- Read CC Switch v2 snapshots from WebDAV and S3-compatible storage.
- Manage multiple named WebDAV and S3 sources under `~/.cc-switchy`.
- Select one default source for `cc-switchy --sync`.
- Restore `db.sql` and `skills.zip` safely into the compatible local layout.
- Apply restored providers, MCP servers, and Skills to supported Agents.
- Let users browse Agents and switch providers from a keyboard-first TUI.
- Provide stage progress, byte progress, actionable errors, and durable results.
- Provide complete Simplified Chinese and English user interfaces.
- Build and release Linux GNU, Linux MUSL, macOS, and Windows binaries.

### Non-goals for v1

- Uploading, deleting, or modifying remote data.
- Bidirectional synchronization, merging, or conflict resolution.
- Scheduled/background synchronization.
- Choosing the newest snapshot across multiple sources.
- S3 session-token credentials, object-version selection, or server-side
  encryption configuration.
- Editing provider definitions or enabling/disabling Skills inside the TUI.
- Reimplementing CC Switch proxy, failover, usage, or session-management UI.

## 2. Command-Line Contract

The binary uses flag-based commands to match the requested interface.

```text
cc-switchy
cc-switchy --wizard
cc-switchy --sync
cc-switchy --sync --source <name>
cc-switchy --source <name>
cc-switchy --lang zh
cc-switchy --lang en
cc-switchy --help
cc-switchy --version
```

Behavior:

- No arguments opens the Ratatui interface.
- `--wizard` opens the interactive source manager.
- `--sync` performs a non-interactive pull, restore, and local apply.
- `--source <name>` selects a source for the current invocation without changing
  the configured default. With `--sync`, it syncs immediately; without
  `--sync`, it opens the TUI with that source selected for the session.
- `--lang zh|en` overrides the configured or detected language for the current
  invocation without changing the persisted preference.
- `--wizard` and `--sync` are mutually exclusive.
- If no source is configured, both the TUI and `--sync` show the exact guidance
  `Run: cc-switchy --wizard`.

Exit codes:

| Code | Meaning |
| --- | --- |
| `0` | Sync and every requested local projection completed successfully. |
| `1` | Configuration, transport, validation, restore, or rollback failed. |
| `2` | Database and Skills were restored, but one or more live Agent projections returned warnings. |

Interactive progress is written to the terminal UI. Non-interactive progress is
written as stable, timestamped lines to stderr so stdout remains suitable for a
final summary.

## 3. Local Storage

`cc-switchy` owns:

```text
~/.cc-switchy/
├── config.toml
├── config.toml.bak
├── state.json
├── lock
├── staging/
└── backups/
    └── <timestamp>/
        ├── metadata.json
        ├── cc-switch.db
        └── skills/
```

- `config.toml` is atomically written and mode `0600` on Unix.
- `state.json` stores non-secret UI state such as the last viewed Agent, pane,
  source, and per-Agent provider cursor.
- `lock` is a global process lock shared by WebDAV and S3 operations.
- `staging` contains only incomplete or verified-but-not-applied downloads and
  is cleaned after success, failure, or safe cancellation.
- `backups` contains the pre-restore database and managed Skills tree. v1 does
  not automatically delete backups.

CC Switch-compatible state remains in:

```text
~/.cc-switch/cc-switch.db
~/.cc-switch/settings.json
~/.cc-switch/skills/
```

If the receiver's local CC Switch settings select `~/.agents/skills` as the
Skills SSOT, that directory is used instead of `~/.cc-switch/skills`.
`~/.cc-switch` is created only when a valid remote snapshot is ready to apply.

## 4. Multi-source Configuration

The versioned TOML schema is:

```toml
version = 1
language = "auto"
default_source = "home-webdav"

[[sources]]
name = "home-webdav"
type = "webdav"
remote_root = "cc-switch-sync"
profile = "default"

[sources.webdav]
base_url = "https://dav.example.com/remote.php/dav/files/user"
username = "user"
password = "secret"

[[sources]]
name = "backup-s3"
type = "s3"
remote_root = "cc-switch-sync"
profile = "default"

[sources.s3]
region = "auto"
bucket = "cc-switch"
endpoint = "https://example.r2.cloudflarestorage.com"
access_key_id = "access-key"
secret_access_key = "secret-key"
```

Rules:

- `language` is `auto`, `zh-CN`, or `en-US` and defaults to `auto`.
- Source names are unique, non-empty stable identifiers.
- Exactly one configured source is the default when at least one source exists.
- `remote_root` defaults to `cc-switch-sync`.
- `profile` defaults to `default`.
- WebDAV requires an HTTP(S) URL, username, and password and uses Basic Auth.
- Empty S3 `endpoint` means AWS virtual-hosted style. A custom endpoint uses
  path-style addressing and defaults to HTTPS when the scheme is omitted.
- S3 v1 credentials are Access Key ID plus Secret Access Key only.
- Secrets are stored as plaintext for CC Switch compatibility but protected by
  file permissions and never included in logs or TUI details.

## 5. Wizard

`cc-switchy --wizard` and the TUI `w` action use the same source-management
state machine.

```text
┌ Sync Sources ────────────────────────────────────────────────────────┐
│ › home-webdav    WebDAV   DEFAULT   ✓ Connected                     │
│   work-s3        S3                  ✓ Snapshot available            │
│   nas-webdav     WebDAV              ! Authentication failed         │
├─────────────────────────────────────────────────────────────────────┤
│ Remote snapshot  devbox · 2026-07-13 10:42                          │
│ Path             cc-switch-sync/v2/db-v6/default                    │
└ a add  e edit  Enter details  x delete  t test  m make default  q exit
```

Actions:

- `a`: add a WebDAV or S3 source.
- `Enter`: inspect the selected source with secrets masked.
- `e`: edit the selected source.
- `x`: confirm and delete the selected source.
- `t`: test connection and fetch the current remote manifest.
- `m`: make the selected source the default.
- `L`: select `Auto`, `简体中文`, or `English` and persist the preference.
- `q`: exit the standalone wizard or return to the main TUI.
- `Esc`: discard the active form edit and return to the source list.

Each confirmed mutation is validated and atomically persisted immediately.
Deleting the default source requires choosing a replacement unless it is the
last configured source. An empty but reachable remote may be saved with a
warning.

## 6. CC Switch Remote Compatibility

The fixed remote wire contract is:

```text
format             cc-switch-webdav-sync
protocol version   2
db compat version  6
artifacts           manifest.json, db.sql, skills.zip
manifest limit      1 MiB
artifact limit      512 MiB each
```

Manifest shape:

```json
{
  "format": "cc-switch-webdav-sync",
  "version": 2,
  "dbCompatVersion": 6,
  "deviceName": "devbox",
  "createdAt": "2026-07-13T10:42:00Z",
  "artifacts": {
    "db.sql": { "sha256": "...", "size": 123 },
    "skills.zip": { "sha256": "...", "size": 456 }
  },
  "snapshotId": "..."
}
```

Current paths:

```text
WebDAV: {baseUrl}/{remoteRoot}/v2/db-v6/{profile}/{artifact}
S3 key: {remoteRoot}/v2/db-v6/{profile}/{artifact}
```

WebDAV falls back to the legacy path `{remoteRoot}/v2/{profile}` when the
current manifest is absent. A legacy manifest without `dbCompatVersion` is
treated as db-v5. S3 does not use the legacy fallback.

`cc-switchy --sync` always fetches the current manifest from the selected
source. It does not compare timestamps across sources and does not treat the
local database as newer. An explicit sync makes the selected cloud snapshot
authoritative even when its `createdAt` is older than local state.

No remote cache is trusted for restore. The current manifest and both artifacts
are fetched and re-applied on every explicit sync so local drift is repaired.

## 7. Download-only Transport Boundary

Transport code exposes only:

```text
fetch_manifest()
fetch_artifact(name, expected_size, progress_sink)
test_connection()
```

There are no PUT, POST, DELETE, MKCOL, multipart-upload, or bucket-management
operations in `cc-switchy`.

WebDAV uses HTTP(S), Basic Auth, HEAD/GET, and a read-only Depth=0 PROPFIND when
needed for connection diagnosis. Existing base URL path segments are retained.

S3 implements AWS Signature Version 4 directly:

- AWS endpoints use virtual-hosted style.
- Custom endpoints use path-style.
- Query strings and signed URLs are redacted from errors.
- The implementation supports AWS S3, MinIO, Cloudflare R2, and compatible
  services within the credential limits above.

Idempotent read operations retry transient connection errors, HTTP 429, and
HTTP 5xx up to three attempts with bounded backoff. Authentication failures,
404 manifest absence, protocol incompatibility, and integrity failures are not
retried.

## 8. Progress and Feedback

The application core publishes transport-independent events:

```text
Locking
Connecting
FetchingManifest
ValidatingManifest
Downloading { artifact, downloaded, total }
Verifying { artifact }
PreparingLocalBackup
RestoringSkills
ImportingDatabase
ApplyingProvider { agent }
ApplyingMcp { agent }
ApplyingSkills { agent, completed, total }
Retrying { operation, attempt, max_attempts }
Completed { duration, snapshot_id, counts }
Warning { stage, agent, message }
Failed { stage, message, retryable }
```

The CLI renderer shows byte progress in a TTY and line-oriented progress when
stderr is redirected. The TUI shows overall progress, the current operation,
per-artifact byte progress, elapsed time, remote snapshot metadata, and a
scrollable activity log.

Secrets, Authorization headers, API keys, passwords, S3 signatures, and URL
query values are redacted before an event reaches any renderer.

Progress events carry stable message keys and typed arguments rather than
pre-rendered English or Chinese sentences. This keeps the CLI, wizard, TUI,
logs, and retry feedback consistent in both languages.

## 9. Restore Transaction

The restore pipeline is:

```text
acquire global lock
→ resolve default or explicit source
→ fetch and validate manifest
→ download artifacts into a unique staging directory
→ verify declared sizes and SHA-256 hashes
→ validate ZIP safety and SQL export header
→ build and validate a temporary SQLite database
→ create a durable local backup
→ replace the receiver's Skills SSOT
→ atomically copy the validated SQLite database into the live connection
→ project providers, MCP, and Skills to local Agents
→ persist sync result and clean staging
→ release lock
```

Database rules:

- `db.sql` must begin with `-- CC Switch SQLite 导出`.
- SQL is executed only against a temporary database first.
- Required schema and basic state are validated before live replacement.
- The remote database must contain provider or MCP state, matching CC Switch's
  import safety requirement.
- The local-only tables `proxy_request_logs`, `stream_check_logs`,
  `proxy_live_backup`, and `usage_daily_rollups` are preserved when present.
- `provider_health` is not preserved and may rebuild.
- A missing local database is allowed; the compatible directory and database
  are created only after remote validation succeeds.

Skills rules:

- ZIP paths are normalized and rejected on absolute paths, parent traversal,
  drive prefixes, or extraction outside staging.
- At most 10,000 entries and 512 MiB total extracted bytes are allowed.
- The existing Skills SSOT is backed up before replacement.
- If database replacement fails after Skills replacement, Skills are rolled
  back from the durable backup.
- Disabled managed Skills and orphaned SSOT symlinks are reconciled during
  projection; unrelated ordinary directories are preserved.

Cancellation rules:

- During network download or staging validation, cancellation stops promptly,
  removes staging, and leaves local data unchanged.
- Once local replacement begins, cancellation is deferred until the database
  transaction completes or rollback finishes.
- Terminal raw mode and cursor visibility are restored on every exit path.

## 10. Device-local Settings

`~/.cc-switch/settings.json` is device-local and is never replaced by the cloud
snapshot. This preserves:

- Current provider overrides when the referenced provider still exists.
- Per-Agent configuration directory overrides.
- Skills storage location and synchronization method.
- Other CC Switch device settings not stored in the database snapshot.

After restore, a valid local current-provider ID wins. If it no longer exists,
the invalid local ID is cleared and the restored database `is_current` provider
becomes the fallback.

## 11. Agent Projection

Supported Agents and default paths:

| Agent | Provider mode | Live configuration | Skills |
| --- | --- | --- | --- |
| Claude | Exclusive | `~/.claude/settings.json`, with existing `claude.json` compatibility | `~/.claude/skills` |
| Claude Desktop | Exclusive | Supported macOS/Windows third-party profile paths | Not supported |
| Codex | Exclusive | `~/.codex/auth.json`, `config.toml`, `cc-switch-model-catalog.json` | `~/.codex/skills` |
| Gemini | Exclusive | `~/.gemini/.env`, `settings.json` | `~/.gemini/skills` |
| OpenCode | Additive | `~/.config/opencode/opencode.json` | `~/.config/opencode/skills` |
| OpenClaw | Additive | `~/.openclaw/openclaw.json` | Not supported by CC Switch |
| Hermes | Additive | `~/.hermes/config.yaml` on Linux/macOS; platform path on Windows | `<hermes-dir>/skills` |

Exclusive provider switching performs the complete compatible operation:

```text
validate target provider
→ backfill the previous live provider where required
→ update device-local current provider
→ update database is_current
→ write target live configuration
→ re-project that Agent's MCP configuration
```

Additive Agents write every provider whose metadata does not opt out of managed
live configuration. The TUI presents these as applied sets, not radio-button
choices.

Claude Desktop official and direct providers are supported on macOS and
Windows. Claude Desktop proxy-mode providers depend on the CC Switch proxy
runtime, which is outside v1 scope; they produce a per-Agent projection warning
and exit code 2 while other Agents continue.

Provider lists use CC Switch ordering:

```sql
ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC
```

## 12. Ratatui Interaction Design

The TUI is the default interface and uses instant keyboard state changes. There
are no decorative transitions for navigation. Animation is limited to actual
work indicators such as spinners and linear progress.

Main views:

```text
1 Providers   2 Skills   3 Activity   4 Sources
```

The Providers view uses three columns when the terminal is at least 120 cells
wide: Agents, providers for the selected Agent, and details. Widths 80–119 use
two panes, and narrower supported terminals use a single-pane page flow.
Terminals below 60x18 show a resize message without corrupting terminal state.

Navigation:

- `↑`/`↓` or `j`/`k`: move within the focused list.
- `Tab`/`Shift+Tab`: move focus between panes.
- `←`/`h` and `→`/`l`: move between adjacent panes.
- `[`/`]`: switch to the previous or next Agent from any main pane.
- Moving in the Agent list immediately changes the viewed Agent; it does not
  modify live configuration.
- `Enter` on an exclusive provider applies it.
- `Enter` on an additive Agent re-applies all managed providers.
- `s`: sync the default source, or the selected source in the Sources view.
- `t`: test the selected source in the Sources view.
- `m`: make the selected source the default.
- `w`: enter the shared source wizard.
- `L`: open the language selector and re-render the interface immediately.
- `?`: show help.
- `Esc`: close the current overlay or return to the prior pane.
- `q`: quit when no blocking restore transaction is active.

The application stores the last Agent, pane, source, and per-Agent provider
cursor. Returning to an Agent restores its previous cursor and scroll offset.

Status is never color-only:

```text
› selected/viewed item
● current exclusive provider
○ available provider
◉ additive provider set
✓ success
! warning
× failure or unavailable Agent
```

Routine success feedback is appended to Activity instead of opening a modal.
Errors remain visible until acknowledged. A partial-apply result shows every
failed Agent and offers retry without re-downloading the snapshot.

The Sources view displays source name, type, default status, safe endpoint,
remote root/profile, connection result, current manifest metadata, and last
sync result. Passwords and secrets are completely hidden; access-key identifiers
are partially masked.

## 13. Internationalization

All application-owned text is available in Simplified Chinese and English:

- CLI help, validation, first-run guidance, summaries, and exit messages.
- Wizard labels, forms, confirmations, source tests, and warnings.
- TUI tabs, pane titles, key hints, empty states, progress, and result views.
- Transport, protocol, restore, provider, MCP, Skills, and terminal errors.
- Activity entries and retry feedback.

Language resolution precedence is:

```text
--lang zh|en
→ CC_SWITCHY_LANG
→ config.toml language
→ LC_ALL / LC_MESSAGES / LANG
→ English fallback
```

Chinese locale values beginning with `zh` select `zh-CN`; all other unknown or
unsupported locales fall back to `en-US`. `language = "auto"` repeats system
locale detection on each launch.

The implementation uses a small compile-time message catalog with a typed
message-key enum. It does not add a runtime localization dependency. Both
catalogs must contain the same keys and placeholder names. User-supplied source,
Agent, provider, path, and remote error values are inserted as data and never
interpreted as message templates.

Changing the language from the TUI or Wizard re-renders the active screen
without restarting. Persisted activity records store message keys and arguments
where possible so current-session entries can also be re-rendered.

## 14. Error Model

Errors carry:

- Stable category and stage.
- User-facing message.
- Optional source and Agent identity.
- Retryability.
- Redacted technical cause for verbose logs.

Primary categories are configuration, lock contention, transport,
authentication, remote-empty, protocol incompatibility, integrity, archive,
database validation, backup, restore, rollback, provider projection, MCP
projection, Skills projection, terminal, and cancellation.

Restore failure before local replacement returns exit code `1` and leaves local
state untouched. Restore or rollback failure after replacement returns exit
code `1` with the durable backup path. Projection warnings return exit code `2`
and keep the successfully restored database and Skills.

## 15. Security and Hardening

- Config and state files use restrictive Unix permissions.
- Temporary and backup paths are created beneath owned directories with unique
  names and atomic rename where supported.
- Remote sizes are checked before allocation and during streaming.
- ZIP extraction rejects traversal and unsafe links.
- SQL is executed only after the CC Switch export header check and only in an
  isolated temporary database before replacement.
- Logs use a redaction layer before rendering or persistence.
- No shell is invoked for restore, projection, or transport operations.
- Provider configuration values are treated as untrusted structured data.
- S3 canonicalization and WebDAV URL joining have dedicated test vectors.

The upstream snapshot itself is not encrypted by the CC Switch protocol.
Documentation must state that `db.sql` can contain provider API keys and users
must secure their WebDAV/S3 storage.

## 16. Rust Architecture

The implementation uses focused modules and a thin binary entry point:

```text
src/
├── main.rs
├── lib.rs
├── cli.rs
├── error.rs
├── i18n.rs
├── progress.rs
├── commands/
│   ├── mod.rs
│   ├── sync.rs
│   ├── tui.rs
│   └── wizard.rs
├── config/
│   ├── mod.rs
│   ├── model.rs
│   └── store.rs
├── remote/
│   ├── mod.rs
│   ├── protocol.rs
│   ├── webdav.rs
│   └── s3.rs
├── restore/
│   ├── mod.rs
│   ├── archive.rs
│   ├── backup.rs
│   ├── database.rs
│   └── service.rs
├── agent/
│   ├── mod.rs
│   ├── paths.rs
│   ├── provider.rs
│   ├── mcp.rs
│   └── skills.rs
└── tui/
    ├── mod.rs
    ├── app.rs
    ├── event.rs
    ├── keymap.rs
    └── view.rs
```

The remote client is an enum with read-only async methods rather than a
Tauri-bound service or an object-safe async trait. CLI, wizard, and TUI all call
the same application services and consume the same progress events.

Primary dependencies are kept MUSL-compatible: Clap, Ratatui, Crossterm,
Tokio, Reqwest with Rustls and no default TLS features, Rusqlite with bundled
SQLite, Serde/TOML/JSON, ZIP with a pure-Rust deflate backend, SHA-256/HMAC,
Chrono, URL handling, temporary files, and a cross-platform file lock.

## 17. Testing Strategy

Unit tests cover:

- Config normalization, uniqueness, default-source rules, atomic persistence,
  and secret masking.
- Language precedence, locale detection, catalog completeness, placeholder
  parity, and CLI/TUI language switching.
- Manifest parsing, compatibility, limits, snapshot hash validation, and
  WebDAV legacy fallback.
- WebDAV URL joining and S3 SigV4 canonical requests.
- Retry classification and progress event ordering.
- ZIP traversal, entry-count, size-limit, and link handling.
- SQL header, temporary import, required state, table preservation, and
  rollback behavior.
- Provider ordering and exclusive/additive switching rules.
- Per-platform Agent path resolution.
- TUI reducer, keymap, focus movement, remembered cursor state, and rendering
  via Ratatui `TestBackend`.

Integration tests use committed fixtures generated in CC Switch-compatible
format and isolated temporary home directories. They verify:

- First-run guidance.
- Wizard CRUD and default-source changes.
- WebDAV and S3 read-only sync against mock servers.
- Full snapshot restore and local table preservation.
- Skills rollback after forced database failure.
- Provider/MCP/Skills projections for each supported Agent.
- Partial projection warnings and exit code `2`.
- Concurrent sync lock rejection.
- Cancellation before and during the restore boundary.

No test may read or write the developer's real home directory.

## 18. CI and Release

GitHub Actions run on pushes and pull requests:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- Linux GNU build
- A representative MUSL build to catch static-link regressions

Version tags build and archive:

```text
x86_64-unknown-linux-gnu
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
x86_64-apple-darwin
aarch64-apple-darwin
x86_64-pc-windows-msvc
```

Linux MUSL builds use Rustls and bundled SQLite so no runtime OpenSSL or system
SQLite is required. Release artifacts include the binary, README/license files,
and SHA-256 checksums.

## 19. Compatibility Attribution

CC Switch and cc-switchy are MIT licensed. Any substantial adapted source keeps
the upstream copyright notice. The repository includes a third-party notice
describing the adapted CC Switch protocol, database, provider, and Skills
behavior.

## 20. Acceptance Criteria

The first version is complete when:

1. `cc-switchy` opens the TUI and restores its last UI context.
2. An unconfigured invocation directs the user to `cc-switchy --wizard`.
3. The wizard can list, create, inspect, edit, delete, test, and select a default
   WebDAV or S3 source.
4. `cc-switchy --sync` always downloads and applies the current snapshot from
   the default source.
5. `--source <name>` selects another source without modifying the default.
6. Real CC Switch v2 WebDAV and S3 fixtures pass protocol and restore tests.
7. Invalid, oversized, or tampered remote data cannot change local state.
8. Restore failure rolls back Skills and reports the durable backup path.
9. Restored providers, MCP, and Skills are projected with CC Switch-compatible
   exclusive/additive behavior.
10. The TUI switches Agents independently from switching providers and reports
    every action immediately.
11. CLI and TUI show stage and byte progress with secret-safe errors.
12. CLI, Wizard, TUI, progress, and errors are complete in Simplified Chinese
    and English, with automatic locale selection and explicit override.
13. All tests, formatting, Clippy, cross-platform builds, and required MUSL
    release builds pass.
