# CC Switch v2 compatibility fixture

This synthetic fixture is pinned to the behavior of CC Switch commit
`c6197ae32450cd70e2bf03b35e3f5f53ac12044c`.

Upstream references used to verify it:

- `src-tauri/src/services/sync_protocol.rs` for protocol v2, db-v6 artifact
  names, SHA-256 metadata, and `snapshotId` construction;
- `src-tauri/src/database/backup.rs` for the SQL export header and synchronized
  database shape;
- `src-tauri/src/services/webdav_sync/archive.rs` for the Skills ZIP shape.

The fixture intentionally contains:

- an exclusive Codex provider and an additive OpenCode provider;
- one stdio MCP server enabled for Codex and OpenCode;
- one enabled Skill and one disabled installed Skill;
- a Skills archive containing the enabled `demo/SKILL.md` source;
- the local-only tables used by restore-preservation tests.

Verification commands:

```bash
git -C /home/czyt/code/others/cc-switch checkout c6197ae32450cd70e2bf03b35e3f5f53ac12044c
cargo test --test protocol_compat committed_fixture -- --nocapture
sha256sum tests/fixtures/cc-switch-v2/{manifest.json,db.sql,skills.zip}
```

Pinned file hashes:

```text
9de563b90e16e1119b605959738da683e4709a5af687cdfe3f1139029625ef7e  manifest.json
dced6f193af3fc6394c600d86f55a178a4b3decca0af618e8e63af8343add65d  db.sql
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
