//! Minimal CC Switch schema repair needed for db-v5/db-v6 restore compatibility.

use rusqlite::Connection;

use crate::AppError;

pub(crate) fn migrate_and_validate(connection: &Connection) -> Result<(), AppError> {
    create_core_tables(connection)?;
    migrate_legacy_columns(connection)?;
    create_indexes(connection)?;
    validate_required_shape(connection)?;
    validate_basic_state(connection)?;
    validate_integrity(connection)
}

fn create_core_tables(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS providers (
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
            CREATE TABLE IF NOT EXISTS mcp_servers (
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
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT
            );
            CREATE TABLE IF NOT EXISTS skills (
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
            CREATE TABLE IF NOT EXISTS proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                model TEXT NOT NULL DEFAULT '',
                input_token_semantics INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS stream_check_logs (
                id INTEGER PRIMARY KEY,
                message TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS proxy_live_backup (
                app_type TEXT PRIMARY KEY,
                original_config TEXT NOT NULL,
                backed_up_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS usage_daily_rollups (
                date TEXT NOT NULL,
                app_type TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                input_token_semantics INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, app_type, provider_id, model)
            );",
        )
        .map_err(database_error)
}

fn migrate_legacy_columns(connection: &Connection) -> Result<(), AppError> {
    for (table, column, declaration) in [
        ("providers", "website_url", "TEXT"),
        ("providers", "category", "TEXT"),
        ("providers", "created_at", "INTEGER"),
        ("providers", "sort_index", "INTEGER"),
        ("providers", "notes", "TEXT"),
        ("providers", "icon", "TEXT"),
        ("providers", "icon_color", "TEXT"),
        ("providers", "meta", "TEXT NOT NULL DEFAULT '{}'"),
        ("providers", "is_current", "BOOLEAN NOT NULL DEFAULT 0"),
        (
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        ),
        ("mcp_servers", "description", "TEXT"),
        ("mcp_servers", "homepage", "TEXT"),
        ("mcp_servers", "docs", "TEXT"),
        ("mcp_servers", "tags", "TEXT NOT NULL DEFAULT '[]'"),
        (
            "mcp_servers",
            "enabled_claude",
            "BOOLEAN NOT NULL DEFAULT 0",
        ),
        ("mcp_servers", "enabled_codex", "BOOLEAN NOT NULL DEFAULT 0"),
        (
            "mcp_servers",
            "enabled_gemini",
            "BOOLEAN NOT NULL DEFAULT 0",
        ),
        (
            "mcp_servers",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        ),
        (
            "mcp_servers",
            "enabled_hermes",
            "BOOLEAN NOT NULL DEFAULT 0",
        ),
        ("skills", "description", "TEXT"),
        ("skills", "repo_owner", "TEXT"),
        ("skills", "repo_name", "TEXT"),
        ("skills", "repo_branch", "TEXT DEFAULT 'main'"),
        ("skills", "readme_url", "TEXT"),
        ("skills", "enabled_claude", "BOOLEAN NOT NULL DEFAULT 0"),
        ("skills", "enabled_codex", "BOOLEAN NOT NULL DEFAULT 0"),
        ("skills", "enabled_gemini", "BOOLEAN NOT NULL DEFAULT 0"),
        ("skills", "enabled_opencode", "BOOLEAN NOT NULL DEFAULT 0"),
        ("skills", "enabled_hermes", "BOOLEAN NOT NULL DEFAULT 0"),
        ("skills", "installed_at", "INTEGER NOT NULL DEFAULT 0"),
        ("skills", "content_hash", "TEXT"),
        ("skills", "updated_at", "INTEGER NOT NULL DEFAULT 0"),
        (
            "proxy_request_logs",
            "input_token_semantics",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "usage_daily_rollups",
            "input_token_semantics",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        add_column_if_missing(connection, table, column, declaration)?;
    }
    Ok(())
}

fn create_indexes(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_providers_app_sort
             ON providers(app_type, sort_index, created_at, id);",
        )
        .map_err(database_error)
}

fn validate_required_shape(connection: &Connection) -> Result<(), AppError> {
    for (table, columns) in [
        (
            "providers",
            &[
                "id",
                "app_type",
                "name",
                "settings_config",
                "meta",
                "is_current",
            ][..],
        ),
        (
            "mcp_servers",
            &["id", "name", "server_config", "enabled_claude"][..],
        ),
        ("settings", &["key", "value"][..]),
        ("skills", &["id", "name", "directory"][..]),
    ] {
        if !table_exists(connection, table)? {
            return Err(AppError::DatabaseValidation(format!(
                "required table {table} is missing"
            )));
        }
        for column in columns {
            if !column_exists(connection, table, column)? {
                return Err(AppError::DatabaseValidation(format!(
                    "required column {table}.{column} is missing"
                )));
            }
        }
    }
    Ok(())
}

fn validate_basic_state(connection: &Connection) -> Result<(), AppError> {
    let provider_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
        .map_err(database_error)?;
    let mcp_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
        .map_err(database_error)?;
    if provider_count == 0 && mcp_count == 0 {
        return Err(AppError::DatabaseValidation(
            "snapshot contains neither providers nor MCP servers".to_string(),
        ));
    }
    Ok(())
}

fn validate_integrity(connection: &Connection) -> Result<(), AppError> {
    let result: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(database_error)?;
    if result != "ok" {
        return Err(AppError::DatabaseValidation(format!(
            "SQLite quick_check failed: {result}"
        )));
    }
    Ok(())
}

pub(crate) fn table_exists(connection: &Connection, table: &str) -> Result<bool, AppError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(database_error)
}

pub(crate) fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, AppError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(database_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(database_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    Ok(table_columns(connection, table)?
        .iter()
        .any(|candidate| candidate == column))
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<(), AppError> {
    if !column_exists(connection, table, column)? {
        connection
            .execute(
                &format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {declaration}"),
                [],
            )
            .map_err(database_error)?;
    }
    Ok(())
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
