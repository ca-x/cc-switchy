# Source Sync Status Repair

## Context

The configured S3 source is healthy: both the connection test and a complete
download-and-restore run succeed with the installed `cc-switchy 0.2.0` binary.
The misleading behavior is in the TUI state flow:

- source tests send ordinary progress events, which mark the global progress
  model active, but test completion never clears that active state;
- a successful source test is stored only in the in-memory `ViewSource.status`;
- sync completion reloads the application and recreates every source with a
  `None` status, so the Sources view returns to “Not tested” even after success;
- the final sync result is visible only in Activity, not beside the source that
  was synchronized.

This makes successful download operations look unfinished or failed.

## Goal

Make the Sources view accurately communicate the lifecycle of a remote test and
a remote-to-local synchronization without forcing the user to switch views.

## Non-goals

- No upload, merge, or remote deletion support.
- No persistence of transient test results across process restarts.
- No changes to the remote protocol, credentials, restore transaction, or
  synchronization semantics.
- No automatic navigation to the Activity view.
- No change to the public `ProgressEvent` variant shape.

## Considered approaches

1. Preserve per-source status and update it through test/sync lifecycle events.
   This is the selected approach because it keeps the user in context and
   changes only presentation state.
2. Automatically switch to Activity whenever an operation starts. This exposes
   progress but interrupts source management and still does not explain the
   source status when the user returns.
3. Add a global modal or toast. This adds a second feedback system and more
   dismissal/focus behavior than the defect requires.

## Design

### Test lifecycle

When a test starts, the selected source immediately displays the localized
“Testing…” state. The test client uses a no-op progress sink because its
intermediate transport stages are not a cancellable global sync operation. On
completion, the source displays either the snapshot identifier, the connected
but empty state, or the redacted error. The final result is also appended to the
Activity log.

### Sync lifecycle

When synchronization starts, the requested source immediately displays a
localized “Synchronizing…” state. `SyncFinished` carries the requested source
name even when the operation fails. On success, the source displays the short
snapshot identifier and warning count. On failure, it displays the existing
redacted error text. Detailed stages and warnings remain in Activity.

### Reload behavior

Application reloads preserve transient source status only when both the source
name and complete normalized `SourceConfig` still match. Deleted, renamed, or
reconfigured same-name sources do not inherit stale state. Source tests carry
the exact tested configuration and discard late results when that configuration
is no longer present. The wizard is unavailable while a source test is active.

### Skill restore progress

`ProgressSink` gains a provided `emit_skill` method. Its default implementation
forwards the existing `ApplyingSkills` event with the original agent identity,
so external consumers retain the existing structured contract. Built-in CLI and
TUI sinks override the method and render `Codex · using-superpowers 12/76`
through internal display state. If a display name is blank, the directory name
is used. Remote-derived labels replace control characters, are bounded to 80
characters plus an ellipsis, and cannot inject terminal control sequences or
forged redirected-output lines.

## Error handling and safety

All public CLI/TUI error rendering reuses credential-safe error values. TOML
parse failures use a generic public message instead of parser source lines that
may contain secrets.
No credentials or raw request headers are added to status messages. Every
`SyncFinished` result clears the global active state, including failures before
`SyncService::run`. A failed sync does not reload restored data and leaves the
source marked with the failure.

At widths below 80 columns, the source list renders the per-source status below
each source row. At the minimum supported height, the details pane removes blank
separator lines so the status remains visible.

## Test seams

The regression tests exercise four observable boundaries:

1. application reload state: a source status survives only for the exact same
   configuration, and stale same-name test results are discarded;
2. rendered Sources view: testing, synchronized, and failed states are visible
   in source details using localized text.
3. Skill projection progress: emitted progress identifies the current agent,
   Skill name, completed count, and total count, including the directory-name
   fallback for a blank display name and control-character sanitization;
4. responsive status rendering at `70×24` and `100×18`, early sync failure
   teardown, exact-config sync completion, and configuration parse-error
   redaction at both TUI and binary boundaries.

The full Rust suite remains the compatibility gate. A PTY smoke test against the
configured source verifies that `t` reaches a terminal result without leaving a
false global working indicator and that `s` leaves a visible synchronized state.

## CC Switch 3.17.0 compatibility

CC Switch 3.17.0 keeps the remote wire contract at protocol v2/db-v6 but raises
SQLite `user_version` from 12 to 13. The schema change only adds
`input_token_semantics INTEGER NOT NULL DEFAULT 0` to `proxy_request_logs` and
`usage_daily_rollups`, both local-only tables whose rows are excluded from sync
exports and restored from the receiving device.

cc-switchy adds these columns to its minimal table creation and repair path. It
does not implement usage accounting or proxy behavior. The purpose is solely to
keep v13 rows intact when a current device restores an older snapshot whose
table definitions do not yet contain the columns. Sync manifests, paths, and
database compatibility checks remain unchanged at db-v6.

## Release

This is a backward-compatible TUI bug fix. If implementation introduces no
public API change, bump `0.2.0` to `0.2.1`, create annotated tag `v0.2.1`, push
`main` and the tag, then verify CI and all release assets.
