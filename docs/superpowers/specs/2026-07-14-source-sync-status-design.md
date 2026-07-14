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

Application reloads preserve transient source status by exact source name.
Statuses are copied only to sources still present after reload; deleted or
renamed sources do not retain stale state. This applies consistently after sync,
provider actions, default-source changes, and returning from the wizard.

### Skill restore progress

Each existing `ApplyingSkills` progress event includes the current Skill display
name together with the agent label and the existing completed/total counter. The
CLI and TUI therefore render progress such as `Codex · using-superpowers 12/76`
without changing the public event shape. If a display name is blank, the Skill
directory name is used as the stable fallback.

## Error handling and safety

The Sources view reuses the same error values already written to Activity. No
credentials or raw request headers are added to status messages. A failed sync
does not reload restored data and leaves the source marked with the failure.

## Test seams

The regression tests exercise two observable boundaries:

1. application reload state: a source status survives reload by exact name while
   unrelated, removed, or renamed sources do not inherit it;
2. rendered Sources view: testing, synchronized, and failed states are visible
   in source details using localized text.
3. Skill projection progress: emitted progress identifies the current agent,
   Skill name, completed count, and total count, including the directory-name
   fallback for a blank display name.

The full Rust suite remains the compatibility gate. A PTY smoke test against the
configured source verifies that `t` reaches a terminal result without leaving a
false global working indicator and that `s` leaves a visible synchronized state.

## Release

This is a backward-compatible TUI bug fix. If implementation introduces no
public API change, bump `0.2.0` to `0.2.1`, create annotated tag `v0.2.1`, push
`main` and the tag, then verify CI and all release assets.
