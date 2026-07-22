use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{Agent, AgentFlags, InstalledSkill, McpServer, Provider, ProviderMeta};
use crate::AppError;

pub struct AgentRepository {
    connection: Connection,
    mcp_has_grok_flag: bool,
    skills_have_grok_flag: bool,
}

impl AgentRepository {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path).map_err(database_error)?;
        let mcp_has_grok_flag = has_column(&connection, "mcp_servers", "enabled_grokbuild")?;
        let skills_have_grok_flag = has_column(&connection, "skills", "enabled_grokbuild")?;
        Ok(Self {
            connection,
            mcp_has_grok_flag,
            skills_have_grok_flag,
        })
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

    pub fn restore_database_current_provider(
        &mut self,
        agent: Agent,
        id: Option<&str>,
    ) -> Result<(), AppError> {
        if let Some(id) = id {
            return self.set_database_current_provider(agent, id);
        }

        self.connection
            .execute(
                "UPDATE providers SET is_current=0 WHERE app_type=?1",
                [agent.db_key()],
            )
            .map(|_| ())
            .map_err(database_error)
    }

    pub fn update_provider_settings(
        &mut self,
        agent: Agent,
        id: &str,
        settings: &serde_json::Value,
    ) -> Result<(), AppError> {
        let serialized = serde_json::to_string(settings)
            .map_err(|error| AppError::DatabaseValidation(error.to_string()))?;
        let affected = self
            .connection
            .execute(
                "UPDATE providers SET settings_config=?1 WHERE app_type=?2 AND id=?3",
                params![serialized, agent.db_key(), id],
            )
            .map_err(database_error)?;
        if affected != 1 {
            return Err(AppError::DatabaseValidation(format!(
                "provider {id} does not exist for {agent}"
            )));
        }
        Ok(())
    }

    pub fn mcp_servers(&self) -> Result<Vec<McpServer>, AppError> {
        let grok_flag = if self.mcp_has_grok_flag {
            "enabled_grokbuild"
        } else {
            "0 AS enabled_grokbuild"
        };
        let query = format!(
            "SELECT id, name, server_config, description, homepage, docs, tags,
                    enabled_claude, enabled_codex, enabled_gemini, {grok_flag},
                    enabled_opencode, enabled_hermes
             FROM mcp_servers
             ORDER BY name ASC, id ASC"
        );
        let mut statement = self.connection.prepare(&query).map_err(database_error)?;
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
                        grokbuild: row.get(10)?,
                        opencode: row.get(11)?,
                        hermes: row.get(12)?,
                    },
                })
            })
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn installed_skills(&self) -> Result<Vec<InstalledSkill>, AppError> {
        let grok_flag = if self.skills_have_grok_flag {
            "enabled_grokbuild"
        } else {
            "0 AS enabled_grokbuild"
        };
        let query = format!(
            "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                    readme_url, enabled_claude, enabled_codex, enabled_gemini,
                    {grok_flag}, enabled_opencode, enabled_hermes,
                    installed_at, content_hash, updated_at
             FROM skills
             ORDER BY name ASC, id ASC"
        );
        let mut statement = self.connection.prepare(&query).map_err(database_error)?;
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
                        grokbuild: row.get(11)?,
                        opencode: row.get(12)?,
                        hermes: row.get(13)?,
                    },
                    installed_at: row.get(14)?,
                    content_hash: row.get(15)?,
                    updated_at: row.get(16)?,
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

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
            params![table, column],
            |row| row.get(0),
        )
        .map_err(database_error)
}
