//! MCP projection adapted from CC Switch's per-Agent MCP adapters.
//!
//! Upstream reference: CC Switch commit
//! c6197ae32450cd70e2bf03b35e3f5f53ac12044c (MIT).

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Map, Value};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use toml_edit::{Array as TomlArray, DocumentMut, Item, Table, Value as TomlValue};

use super::provider::{atomic_write, write_json};
use super::{
    Agent, AgentPaths, AgentRepository, McpServer, ProjectionReport, ProjectionStage,
    ProjectionWarning,
};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::{AppError, MessageKey};

pub struct McpProjector<'a> {
    repo: &'a AgentRepository,
    paths: &'a AgentPaths,
    progress: Arc<dyn ProgressSink>,
}

impl<'a> McpProjector<'a> {
    pub fn new(
        repo: &'a AgentRepository,
        paths: &'a AgentPaths,
        progress: Arc<dyn ProgressSink>,
    ) -> Self {
        Self {
            repo,
            paths,
            progress,
        }
    }

    pub fn project_all(&self) -> ProjectionReport {
        let mut report = ProjectionReport::default();
        for agent in Agent::ALL {
            if !agent.supports_mcp() {
                report.skipped_agents.push(agent);
                continue;
            }
            match self.project_agent(agent) {
                Ok(()) => report.applied_agents.push(agent),
                Err(error) => {
                    report.skipped_agents.push(agent);
                    let message = error.to_string();
                    self.progress.emit(ProgressEvent::Warning {
                        stage: "mcp".to_string(),
                        agent: Some(agent.to_string()),
                        message_key: MessageKey::UnexpectedError,
                        detail: message.clone(),
                    });
                    report.warnings.push(ProjectionWarning {
                        stage: ProjectionStage::Mcp,
                        agent: Some(agent),
                        message,
                    });
                }
            }
        }
        report
    }

    pub fn project_agent(&self, agent: Agent) -> Result<(), AppError> {
        if !agent.supports_mcp() {
            return Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "MCP",
            });
        }
        self.progress.emit(ProgressEvent::ApplyingMcp {
            agent: agent.to_string(),
        });
        let servers = self.repo.mcp_servers()?;
        let known_ids = servers
            .iter()
            .map(|server| server.id.as_str())
            .collect::<HashSet<_>>();
        let enabled = servers
            .iter()
            .filter(|server| server.enabled_for(agent))
            .collect::<Vec<_>>();

        match agent {
            Agent::Claude => self.project_json(
                &self.paths.home().join(".claude.json"),
                "mcpServers",
                &known_ids,
                &enabled,
                identity_spec,
            ),
            Agent::Codex => self.project_codex(&known_ids, &enabled),
            Agent::Gemini => self.project_json(
                &self.paths.config_dir(Agent::Gemini)?.join("settings.json"),
                "mcpServers",
                &known_ids,
                &enabled,
                identity_spec,
            ),
            Agent::OpenCode => self.project_json(
                &self
                    .paths
                    .config_dir(Agent::OpenCode)?
                    .join("opencode.json"),
                "mcp",
                &known_ids,
                &enabled,
                to_opencode,
            ),
            Agent::Hermes => self.project_hermes(&known_ids, &enabled),
            Agent::ClaudeDesktop | Agent::OpenClaw => Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "MCP",
            }),
        }
    }

    fn project_json(
        &self,
        path: &Path,
        section: &str,
        known_ids: &HashSet<&str>,
        enabled: &[&McpServer],
        convert: fn(&Value) -> Result<Value, AppError>,
    ) -> Result<(), AppError> {
        let mut root = read_json_object(path)?;
        let object = root.as_object_mut().expect("validated JSON object");
        let servers = object
            .entry(section.to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| {
                AppError::Restore(format!(
                    "{}.{} must be a JSON object",
                    path.display(),
                    section
                ))
            })?;
        servers.retain(|id, _| !known_ids.contains(id.as_str()));
        for server in enabled {
            let validated = validate_spec(&server.server)?;
            servers.insert(server.id.clone(), convert(&validated)?);
        }
        write_json(path, &root)
    }

    fn project_codex(
        &self,
        known_ids: &HashSet<&str>,
        enabled: &[&McpServer],
    ) -> Result<(), AppError> {
        let path = self.paths.config_dir(Agent::Codex)?.join("config.toml");
        let text = if path.is_file() {
            fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?
        } else {
            String::new()
        };
        let mut document = if text.trim().is_empty() {
            DocumentMut::new()
        } else {
            text.parse::<DocumentMut>().map_err(|error| {
                AppError::Restore(format!("invalid {}: {error}", path.display()))
            })?
        };

        if let Some(mcp) = document.get_mut("mcp").and_then(Item::as_table_like_mut) {
            mcp.remove("servers");
        }
        if !document.contains_key("mcp_servers") {
            document["mcp_servers"] = Item::Table(Table::new());
        }
        let servers = document["mcp_servers"].as_table_mut().ok_or_else(|| {
            AppError::Restore("Codex mcp_servers must be a TOML table".to_string())
        })?;
        servers.retain(|id, _| !known_ids.contains(id));
        for server in enabled {
            let validated = validate_spec(&server.server)?;
            servers.insert(&server.id, Item::Table(to_codex_table(&validated)?));
        }
        if servers.is_empty() {
            document.as_table_mut().remove("mcp_servers");
        }
        atomic_write(&path, document.to_string().as_bytes())
    }

    fn project_hermes(
        &self,
        known_ids: &HashSet<&str>,
        enabled: &[&McpServer],
    ) -> Result<(), AppError> {
        let path = self.paths.config_dir(Agent::Hermes)?.join("config.yaml");
        let mut root = if path.is_file() {
            let text = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
            if text.trim().is_empty() {
                YamlValue::Mapping(YamlMapping::new())
            } else {
                serde_yaml::from_str(&text).map_err(|error| {
                    AppError::Restore(format!("invalid {}: {error}", path.display()))
                })?
            }
        } else {
            YamlValue::Mapping(YamlMapping::new())
        };
        let root_map = root.as_mapping_mut().ok_or_else(|| {
            AppError::Restore(format!("{} must contain a YAML mapping", path.display()))
        })?;
        let section_key = YamlValue::String("mcp_servers".to_string());
        let servers = root_map
            .entry(section_key)
            .or_insert_with(|| YamlValue::Mapping(YamlMapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| AppError::Restore("Hermes mcp_servers must be a mapping".to_string()))?;
        servers.retain(|key, _| key.as_str().is_none_or(|id| !known_ids.contains(id)));
        for server in enabled {
            let validated = validate_spec(&server.server)?;
            let converted = to_hermes(&validated)?;
            let key = YamlValue::String(server.id.clone());
            let mut incoming = serde_yaml::to_value(converted)
                .map_err(|error| AppError::Restore(error.to_string()))?;
            if let (Some(existing), Some(incoming_map)) = (
                servers.get(&key).and_then(YamlValue::as_mapping),
                incoming.as_mapping_mut(),
            ) {
                for field in [
                    "timeout",
                    "connect_timeout",
                    "tools",
                    "sampling",
                    "roots",
                    "auth",
                ] {
                    let field = YamlValue::String(field.to_string());
                    if let Some(value) = existing.get(&field) {
                        incoming_map.entry(field).or_insert_with(|| value.clone());
                    }
                }
            }
            servers.insert(key, incoming);
        }
        let bytes = serde_yaml::to_string(&root)
            .map_err(|error| AppError::Restore(error.to_string()))?
            .into_bytes();
        atomic_write(&path, &bytes)
    }
}

fn read_json_object(path: &Path) -> Result<Value, AppError> {
    if !path.is_file() {
        return Ok(json!({}));
    }
    let bytes = fs::read(path).map_err(|error| AppError::io(path, error))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Restore(format!("invalid {}: {error}", path.display())))?;
    if !value.is_object() {
        return Err(AppError::Restore(format!(
            "{} must contain a JSON object",
            path.display()
        )));
    }
    Ok(value)
}

fn validate_spec(spec: &Value) -> Result<Value, AppError> {
    let object = spec.as_object().ok_or_else(|| {
        AppError::DatabaseValidation("MCP server spec must be a JSON object".to_string())
    })?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    match kind {
        "stdio" => {
            let valid_command = object
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|command| !command.trim().is_empty());
            if !valid_command {
                return Err(AppError::DatabaseValidation(
                    "stdio MCP server requires a command".to_string(),
                ));
            }
        }
        "http" | "sse" => {
            let valid_url = object
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"));
            if !valid_url {
                return Err(AppError::DatabaseValidation(
                    "remote MCP server requires an HTTP(S) URL".to_string(),
                ));
            }
        }
        other => {
            return Err(AppError::DatabaseValidation(format!(
                "unsupported MCP server type {other}"
            )));
        }
    }
    Ok(spec.clone())
}

fn identity_spec(spec: &Value) -> Result<Value, AppError> {
    Ok(spec.clone())
}

fn to_opencode(spec: &Value) -> Result<Value, AppError> {
    let object = spec.as_object().expect("validated MCP spec");
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    match kind {
        "stdio" => {
            let mut command = vec![object.get("command").cloned().expect("validated command")];
            if let Some(args) = object.get("args").and_then(Value::as_array) {
                command.extend(args.iter().cloned());
            }
            let mut result = json!({"type": "local", "command": command, "enabled": true});
            if let Some(env) = object.get("env").filter(|value| value.is_object()) {
                result["environment"] = env.clone();
            }
            Ok(result)
        }
        "http" | "sse" => {
            let mut result = json!({
                "type": "remote",
                "url": object.get("url").cloned().expect("validated URL"),
                "enabled": true
            });
            if let Some(headers) = object.get("headers").filter(|value| value.is_object()) {
                result["headers"] = headers.clone();
            }
            Ok(result)
        }
        _ => unreachable!("validated MCP type"),
    }
}

fn to_hermes(spec: &Value) -> Result<Value, AppError> {
    let object = spec.as_object().expect("validated MCP spec");
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    let mut result = Map::new();
    match kind {
        "stdio" => {
            for key in ["command", "args", "env", "cwd"] {
                if let Some(value) = object.get(key) {
                    result.insert(key.to_string(), value.clone());
                }
            }
        }
        "http" | "sse" => {
            for key in ["url", "headers"] {
                if let Some(value) = object.get(key) {
                    result.insert(key.to_string(), value.clone());
                }
            }
        }
        _ => unreachable!("validated MCP type"),
    }
    result.insert("enabled".to_string(), Value::Bool(true));
    Ok(Value::Object(result))
}

fn to_codex_table(spec: &Value) -> Result<Table, AppError> {
    let object = spec.as_object().expect("validated MCP spec");
    let mut table = Table::new();
    for (key, value) in object {
        let key = if key == "headers" {
            "http_headers"
        } else {
            key
        };
        table.insert(key, json_to_toml_item(value)?);
    }
    Ok(table)
}

fn json_to_toml_item(value: &Value) -> Result<Item, AppError> {
    match value {
        Value::String(value) => Ok(Item::Value(TomlValue::from(value.as_str()))),
        Value::Bool(value) => Ok(Item::Value(TomlValue::from(*value))),
        Value::Number(value) if value.is_i64() => Ok(Item::Value(TomlValue::from(
            value.as_i64().expect("checked integer"),
        ))),
        Value::Number(value) if value.is_u64() => {
            let value = i64::try_from(value.as_u64().expect("checked integer")).map_err(|_| {
                AppError::DatabaseValidation("MCP integer exceeds TOML range".to_string())
            })?;
            Ok(Item::Value(TomlValue::from(value)))
        }
        Value::Number(value) => Ok(Item::Value(TomlValue::from(
            value.as_f64().expect("JSON number"),
        ))),
        Value::Array(values) => {
            let mut array = TomlArray::new();
            for value in values {
                let item = json_to_toml_item(value)?;
                let value = item.into_value().map_err(|_| {
                    AppError::DatabaseValidation(
                        "MCP arrays may contain only scalar values".to_string(),
                    )
                })?;
                array.push(value);
            }
            Ok(Item::Value(TomlValue::Array(array)))
        }
        Value::Object(values) => {
            let mut table = Table::new();
            for (key, value) in values {
                table.insert(key, json_to_toml_item(value)?);
            }
            Ok(Item::Table(table))
        }
        Value::Null => Err(AppError::DatabaseValidation(
            "MCP TOML values cannot be null".to_string(),
        )),
    }
}
