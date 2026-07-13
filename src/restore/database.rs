//! Transactional preparation of CC Switch SQL exports.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OpenFlags};
use tempfile::NamedTempFile;

use super::schema::{migrate_and_validate, table_columns, table_exists};
use crate::AppError;

const SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";
const LOCAL_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
];

pub struct PreparedDatabase {
    pub file: NamedTempFile,
}

pub fn prepare_database(
    sql_path: &Path,
    existing_db: Option<&Path>,
) -> Result<PreparedDatabase, AppError> {
    let sql = fs::read_to_string(sql_path).map_err(|error| AppError::io(sql_path, error))?;
    let sql = sql.trim_start_matches('\u{feff}');
    if !sql.trim_start().starts_with(SQL_EXPORT_HEADER) {
        return Err(AppError::InvalidSqlExport);
    }

    let file = NamedTempFile::new().map_err(|error| AppError::io("temporary database", error))?;
    let mut connection = Connection::open(file.path()).map_err(database_error)?;
    install_import_authorizer(&connection)?;
    let import_result = connection.execute_batch(sql).map_err(database_error);
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .map_err(database_error)?;
    import_result?;
    if !connection.is_autocommit() {
        return Err(AppError::DatabaseValidation(
            "SQL export left a transaction open".to_string(),
        ));
    }

    migrate_and_validate(&connection)?;
    if let Some(existing_db) = existing_db.filter(|path| path.exists()) {
        preserve_local_tables(existing_db, &mut connection)?;
    }
    migrate_and_validate(&connection)?;
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(database_error)?;
    drop(connection);

    Ok(PreparedDatabase { file })
}

fn install_import_authorizer(connection: &Connection) -> Result<(), AppError> {
    connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Attach { .. }
            | AuthAction::Detach { .. }
            | AuthAction::CreateTempIndex { .. }
            | AuthAction::CreateTempTable { .. }
            | AuthAction::CreateTempTrigger { .. }
            | AuthAction::CreateTempView { .. }
            | AuthAction::CreateTrigger { .. }
            | AuthAction::CreateVtable { .. }
            | AuthAction::DropTempIndex { .. }
            | AuthAction::DropTempTable { .. }
            | AuthAction::DropTempTrigger { .. }
            | AuthAction::DropTempView { .. }
            | AuthAction::DropTrigger { .. }
            | AuthAction::DropVtable { .. }
            | AuthAction::Unknown { .. } => Authorization::Deny,
            AuthAction::Function { function_name }
                if function_name.eq_ignore_ascii_case("load_extension") =>
            {
                Authorization::Deny
            }
            AuthAction::Pragma { pragma_name, .. }
                if matches!(
                    pragma_name.to_ascii_lowercase().as_str(),
                    "writable_schema" | "temp_store_directory" | "data_store_directory"
                ) =>
            {
                Authorization::Deny
            }
            _ => Authorization::Allow,
        }))
        .map_err(database_error)
}

fn preserve_local_tables(existing_db: &Path, target: &mut Connection) -> Result<(), AppError> {
    let source = Connection::open_with_flags(existing_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(database_error)?;
    let transaction = target.transaction().map_err(database_error)?;

    for table in LOCAL_TABLES {
        if !table_exists(&source, table)? || !table_exists(&transaction, table)? {
            continue;
        }
        let source_columns = table_columns(&source, table)?;
        let target_columns = table_columns(&transaction, table)?;
        let target_set = target_columns
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let columns = source_columns
            .into_iter()
            .filter(|column| target_set.contains(column.as_str()))
            .collect::<Vec<_>>();
        if columns.is_empty() {
            continue;
        }

        transaction
            .execute(&format!("DELETE FROM {}", quote_identifier(table)), [])
            .map_err(database_error)?;
        let quoted_columns = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let select_sql = format!("SELECT {quoted_columns} FROM {}", quote_identifier(table));
        let placeholders = (1..=columns.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!(
            "INSERT INTO {} ({quoted_columns}) VALUES ({placeholders})",
            quote_identifier(table)
        );
        let mut statement = source.prepare(&select_sql).map_err(database_error)?;
        let mut rows = statement.query([]).map_err(database_error)?;
        while let Some(row) = rows.next().map_err(database_error)? {
            let values = (0..columns.len())
                .map(|index| row.get::<_, Value>(index))
                .collect::<Result<Vec<_>, _>>()
                .map_err(database_error)?;
            transaction
                .execute(&insert_sql, params_from_iter(values.iter()))
                .map_err(database_error)?;
        }
    }

    transaction.commit().map_err(database_error)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
