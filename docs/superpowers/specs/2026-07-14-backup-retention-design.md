# Configurable Backup Retention Design

**Date:** 2026-07-14

## Goal

Prevent `~/.cc-switchy/backups` from growing without bound while leaving the
backup decision under user control. Backups are enabled by default and retain
the newest 10 completed backups. Users can disable backup creation entirely or
set a different global maximum.

## Configuration contract

`config.toml` gains a global backup section:

```toml
[backup]
enabled = true
max_count = 10
```

- `enabled = true` creates a durable pre-restore backup and keeps rollback
  behavior.
- `enabled = false` creates no backup. Restore failures cannot roll database or
  Skills state back, and the UI must state that consequence before saving.
- `max_count = 10` is the default.
- `max_count = 0` means unlimited retention.
- A positive `max_count` is the maximum number of recognized completed backup
  directories retained globally across all synchronization sources.
- `max_count` remains stored while backups are disabled and becomes active
  again when backups are re-enabled.

Existing configuration files remain valid through Serde defaults. The config
format version remains `1` because no existing field changes meaning.

## User interface

The source-management wizard gains a global Backup Settings entry, available
from the list screen with a documented shortcut. The screen contains:

1. an enabled/disabled toggle;
2. a numeric maximum field;
3. explanatory text that `0` means unlimited;
4. a warning that disabling backups also disables rollback.

Changing `enabled` from true to false requires explicit confirmation. Saving
uses the existing atomic `ConfigStore` path, so a persistence failure leaves
the previous policy active and keeps the form values visible.

The README configuration example and bilingual help describe the same
semantics. Users may also edit the TOML directly.

## Restore behavior

### Backups enabled

The restore flow keeps its current safety order:

1. continue under the process-wide sync lock already acquired before download;
2. validate and prepare the downloaded database and Skills;
3. create the complete durable backup and `metadata.json`;
4. enforce retention before changing live database or Skills state;
5. restore Skills and database, using the current backup for rollback on
   failure.

Retention includes the newly created current backup. For example, with
`max_count = 10`, creating backup 11 removes the oldest recognized backup and
leaves 10 before live state is modified.

If retention cleanup fails, synchronization stops before live state mutation.
The newly created backup remains available, and the error identifies the path
that could not be removed.

### Backups disabled

The restore flow skips `LocalBackup::create` and performs no retention cleanup.
It applies prepared Skills and database state directly. If either operation
fails, the error explicitly says rollback was unavailable because backups were
disabled. No backup directory is reported in CLI output, TUI state, or saved
sync state.

## Safe retention rules

Cleanup operates only while holding the existing exclusive sync lock.

A deletion candidate must be a direct, real directory under
`~/.cc-switchy/backups`, have a valid cc-switchy timestamp directory name, and
contain parseable `metadata.json`. Symbolic links, files, malformed directories,
and unknown user-created directories are ignored.

Recognized backups are sorted by metadata creation time with the directory name
as a deterministic tiebreaker. The current backup is always excluded from the
deletion set. Cleanup deletes the oldest candidates until the recognized count
is at or below the configured positive maximum.

Changing the setting does not immediately delete data. Existing excess backups
are pruned during the next synchronization for which backups are enabled. This
keeps destructive cleanup inside the sync lock and immediately before a known
safe restore boundary.

## Data and API shape

Add a `BackupConfig` value to `AppConfig`, with defaults of `enabled = true`
and `max_count = 10`.

Restore and sync outcomes represent the durable backup path as optional. Saved
sync state also makes the backup path optional so disabled backups do not leave
a stale or nonexistent path. CLI and TUI rendering show a localized “backup
disabled / not created” result instead of an empty path.

These public Rust type changes require the next release to be `0.3.0` under the
project's existing SemVer policy.

## Error handling

- Invalid or negative TOML values fail configuration loading without exposing
  source lines that might contain credentials.
- Wizard input accepts only non-negative integers within the selected storage
  type's range.
- Cleanup errors abort before live mutation.
- When backups are disabled, restore errors never claim rollback was attempted.
- Unknown backup-directory contents are never deleted automatically.

## Testing

Tests cover:

- old config defaults to enabled with a maximum of 10;
- TOML and wizard round trips for enabled, disabled, positive counts, and zero;
- zero performs no pruning;
- positive limits retain the newest recognized backups, including the current
  backup;
- malformed directories, files, and symlinks are preserved;
- cleanup failure occurs before database or Skills mutation;
- enabled restore still rolls back on Skills and database failures;
- disabled restore creates no backup and reports rollback as unavailable;
- CLI, TUI, README, and persisted state do not display a nonexistent backup
  path when backup creation is disabled.

## Alternatives rejected

- **TOML-only configuration:** smaller implementation, but too difficult to
  discover in a keyboard-first application.
- **Per-source retention:** backups protect global local state and can contain
  changes involving several Agents, so source-specific limits create confusing
  deletion behavior.
- **`max_count = 0` disables backups:** rejected because zero now has the clear
  meaning “unlimited”; backup creation has a separate explicit switch.
- **Creating a temporary rollback backup while disabled:** rejected by the
  requested switch contract. Disabled means no backup is created and rollback
  is unavailable.
