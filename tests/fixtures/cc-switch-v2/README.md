# CC Switch v2 compatibility fixture

This synthetic fixture is pinned to CC Switch `v3.17.0` at commit
`3d176b98cc0bfd151a42882e88ab59b62083b92f`.

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
- the local-only tables used by restore-preservation tests;
- the v13 `input_token_semantics` columns while retaining sync protocol db-v6.

Verification commands:

```bash
git -C /path/to/cc-switch checkout v3.17.0
cargo test --test protocol_compat committed_fixture -- --nocapture
sha256sum tests/fixtures/cc-switch-v2/{manifest.json,db.sql,skills.zip}
```

Pinned file hashes:

```text
0c6a5a7538311cf9ff5a226be7bb2106e88f9ff12d27463d38679de9db5dee44  manifest.json
fc96f37bcfdb68090621afd38b2c6ed092f1f3de6b9beb69c37abf8c6d61767d  db.sql
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
