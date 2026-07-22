# CC Switch v2 compatibility fixture

This synthetic fixture is pinned to CC Switch `v3.18.0` at commit
`a377d79303bc1e592d2783d559ca5bd6b8ba1417`.

Upstream references used to verify it:

- `src-tauri/src/services/sync_protocol.rs` for protocol v2, db-v6 artifact
  names, SHA-256 metadata, and `snapshotId` construction;
- `src-tauri/src/database/backup.rs` for the SQL export header and synchronized
  database shape;
- `src-tauri/src/services/webdav_sync/archive.rs` for the Skills ZIP shape.

The fixture intentionally contains:

- exclusive Codex and Grok Build providers plus an additive OpenCode provider;
- one stdio MCP server enabled for Codex, Grok Build, and OpenCode;
- one enabled Skill and one disabled installed Skill;
- a Skills archive containing the enabled `demo/SKILL.md` source;
- the local-only tables used by restore-preservation tests;
- the v16 user version and Grok enablement columns while retaining sync protocol db-v6.

Verification commands:

```bash
git -C /path/to/cc-switch checkout a377d79303bc1e592d2783d559ca5bd6b8ba1417
cargo test --test protocol_compat committed_fixture -- --nocapture
sha256sum tests/fixtures/cc-switch-v2/{manifest.json,db.sql,skills.zip}
```

Pinned file hashes:

```text
c98cb624c79b0c7ef172d712b8b04d3deec124a2da6e6e30a8328eebf2e94c2b  manifest.json
afe943269c7740f8784a674a6869ae32230085759c8c90fa7faf843bde159d86  db.sql
f356667d67f458c786ccd16d582c5d4ca7ecc0b0bbd10268b56f6720cb45c0de  skills.zip
```

To refresh manifest metadata after an intentional artifact change, compute the
artifact hashes and then compute the snapshot ID exactly as upstream does:

```bash
db_hash=$(sha256sum tests/fixtures/cc-switch-v2/db.sql | cut -d' ' -f1)
skills_hash=$(sha256sum tests/fixtures/cc-switch-v2/skills.zip | cut -d' ' -f1)
printf 'db.sql:%s|skills.zip:%s' "$db_hash" "$skills_hash" | sha256sum
```

Update the manifest sizes, hashes, and `snapshotId` together. The committed
protocol test rejects any mismatch.
