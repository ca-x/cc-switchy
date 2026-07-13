-- CC Switch SQLite 导出
-- fixture for cc-switchy compatibility tests
PRAGMA foreign_keys=OFF;
PRAGMA user_version=12;
BEGIN TRANSACTION;
CREATE TABLE providers (
    id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    name TEXT NOT NULL,
    settings_config TEXT NOT NULL,
    website_url TEXT,
    category TEXT,
    created_at INTEGER,
    sort_index INTEGER,
    notes TEXT,
    icon TEXT,
    icon_color TEXT,
    meta TEXT NOT NULL DEFAULT '{}',
    is_current BOOLEAN NOT NULL DEFAULT 0,
    in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
    PRIMARY KEY (id, app_type)
);
CREATE TABLE mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    server_config TEXT NOT NULL,
    description TEXT,
    homepage TEXT,
    docs TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    enabled_claude BOOLEAN NOT NULL DEFAULT 0,
    enabled_codex BOOLEAN NOT NULL DEFAULT 0,
    enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
    enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
    enabled_hermes BOOLEAN NOT NULL DEFAULT 0
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE skills (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    directory TEXT NOT NULL,
    repo_owner TEXT,
    repo_name TEXT,
    repo_branch TEXT DEFAULT 'main',
    readme_url TEXT,
    enabled_claude BOOLEAN NOT NULL DEFAULT 0,
    enabled_codex BOOLEAN NOT NULL DEFAULT 0,
    enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
    enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
    enabled_hermes BOOLEAN NOT NULL DEFAULT 0,
    installed_at INTEGER NOT NULL DEFAULT 0,
    content_hash TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY, model TEXT NOT NULL);
CREATE TABLE stream_check_logs (id INTEGER PRIMARY KEY, message TEXT NOT NULL);
CREATE TABLE proxy_live_backup (app_type TEXT PRIMARY KEY, original_config TEXT NOT NULL, backed_up_at TEXT NOT NULL);
CREATE TABLE usage_daily_rollups (date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL, PRIMARY KEY (date, app_type, provider_id, model));
CREATE TABLE provider_health (provider_id TEXT NOT NULL, app_type TEXT NOT NULL, is_healthy INTEGER NOT NULL DEFAULT 1, PRIMARY KEY (provider_id, app_type));
INSERT INTO providers (id, app_type, name, settings_config, created_at, sort_index, meta, is_current)
VALUES
    ('remote-provider', 'codex', 'Remote Provider', '{"api_key":"fixture-only"}', 1, 10, '{}', 1),
    ('additive-provider', 'opencode', 'Additive Provider', '{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://fixture.example/v1"}}', 2, 20, '{}', 0);
INSERT INTO mcp_servers (
    id, name, server_config, tags,
    enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_hermes
)
VALUES (
    'fixture-mcp', 'Fixture MCP',
    '{"type":"stdio","command":"fixture-mcp","args":["--stdio"]}',
    '["fixture"]', 0, 1, 0, 1, 0
);
INSERT INTO skills (
    id, name, directory, enabled_claude, enabled_codex, enabled_gemini,
    enabled_opencode, enabled_hermes, installed_at, updated_at
)
VALUES
    ('demo', 'Demo', 'demo', 0, 1, 0, 1, 0, 1, 1),
    ('disabled-demo', 'Disabled Demo', 'disabled-demo', 0, 0, 0, 0, 0, 1, 1);
COMMIT;
PRAGMA foreign_keys=ON;
