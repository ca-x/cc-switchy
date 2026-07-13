use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{Agent, AgentFlags, InstalledSkill, McpServer, Provider, ProviderMeta};
use crate::AppError;

pub struct AgentRepository {
    connection: Connection,
}

impl AgentRepository {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path).map_err(database_error)?;
        Ok(Self { connection })
    }

    pub fn providers(&self, agent: Agent) -> Result<Vec<Provider>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, settings_config, website_url, category, created_at,
                        sort_index, notes, icon, icon_color, meta, is_current
                 FROM providers
                 WHERE app_type = ?1
                 ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([agent.db_key()], |row| provider_from_row(row, agent))
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn provider(&self, agent: Agent, id: &str) -> Result<Option<Provider>, AppError> {
        self.connection
            .query_row(
                "SELECT id, name, settings_config, website_url, category, created_at,
                        sort_index, notes, icon, icon_color, meta, is_current
                 FROM providers
                 WHERE app_type = ?1 AND id = ?2",
                params![agent.db_key(), id],
                |row| provider_from_row(row, agent),
            )
            .optional()
            .map_err(database_error)
    }

    pub fn database_current_provider(&self, agent: Agent) -> Result<Option<String>, AppError> {
        self.connection
            .query_row(
                "SELECT id FROM providers
                 WHERE app_type = ?1 AND is_current = 1
                 ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC
                 LIMIT 1",
                [agent.db_key()],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)
    }

    pub fn set_database_current_provider(
        &mut self,
        agent: Agent,
        id: &str,
    ) -> Result<(), AppError> {
        let exists: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE app_type=?1 AND id=?2)",
                params![agent.db_key(), id],
                |row| row.get(0),
            )
            .map_err(database_error)?;
        if !exists {
            return Err(AppError::DatabaseValidation(format!(
                "provider {id} does not exist for {agent}"
            )));
        }

        let transaction = self.connection.transaction().map_err(database_error)?;
        transaction
            .execute(
                "UPDATE providers SET is_current=0 WHERE app_type=?1",
                [agent.db_key()],
            )
            .map_err(database_error)?;
        let affected = transaction
            .execute(
                "UPDATE providers SET is_current=1 WHERE app_type=?1 AND id=?2",
                params![agent.db_key(), id],
            )
            .map_err(database_error)?;
        if affected != 1 {
            return Err(AppError::DatabaseValidation(format!(
                "provider {id} could not become current for {agent}"
            )));
        }
        transaction.commit().map_err(database_error)
    }

    pub fn mcp_servers(&self) -> Result<Vec<McpServer>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, server_config, description, homepage, docs, tags,
                        enabled_claude, enabled_codex, enabled_gemini, enabled_opencode,
                        enabled_hermes
                 FROM mcp_servers
                 ORDER BY name ASC, id ASC",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| {
                let server_json: String = row.get(2)?;
                let tags_json: String = row.get(6)?;
                Ok(McpServer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    server: serde_json::from_str(&server_json).unwrap_or_default(),
                    description: row.get(3)?,
                    homepage: row.get(4)?,
                    docs: row.get(5)?,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    apps: AgentFlags {
                        claude: row.get(7)?,
                        codex: row.get(8)?,
                        gemini: row.get(9)?,
                        opencode: row.get(10)?,
                        hermes: row.get(11)?,
                    },
                })
            })
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn installed_skills(&self) -> Result<Vec<InstalledSkill>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini,
                        enabled_opencode, enabled_hermes, installed_at, content_hash, updated_at
                 FROM skills
                 ORDER BY name ASC, id ASC",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(InstalledSkill {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    directory: row.get(3)?,
                    repo_owner: row.get(4)?,
                    repo_name: row.get(5)?,
                    repo_branch: row.get(6)?,
                    readme_url: row.get(7)?,
                    apps: AgentFlags {
                        claude: row.get(8)?,
                        codex: row.get(9)?,
                        gemini: row.get(10)?,
                        opencode: row.get(11)?,
                        hermes: row.get(12)?,
                    },
                    installed_at: row.get(13)?,
                    content_hash: row.get(14)?,
                    updated_at: row.get(15)?,
                })
            })
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, AppError> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(database_error)
    }
}

fn provider_from_row(row: &Row<'_>, agent: Agent) -> rusqlite::Result<Provider> {
    let settings_json: String = row.get(2)?;
    let meta_json: String = row.get(10)?;
    Ok(Provider {
        id: row.get(0)?,
        app: agent,
        name: row.get(1)?,
        settings_config: serde_json::from_str(&settings_json).unwrap_or_default(),
        website_url: row.get(3)?,
        category: row.get(4)?,
        created_at: row.get(5)?,
        sort_index: row.get(6)?,
        notes: row.get(7)?,
        icon: row.get(8)?,
        icon_color: row.get(9)?,
        meta: serde_json::from_str::<ProviderMeta>(&meta_json).unwrap_or_default(),
        is_current: row.get(11)?,
    })
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
