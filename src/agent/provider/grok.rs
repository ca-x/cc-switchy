use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, Table};

use super::atomic_write;
use crate::agent::{Agent, AgentPaths, Provider};
use crate::AppError;

pub(crate) fn managed_paths(paths: &AgentPaths) -> Result<Vec<PathBuf>, AppError> {
    Ok(vec![config_path(paths)?])
}

pub(crate) fn read(paths: &AgentPaths) -> Result<Option<Value>, AppError> {
    let path = config_path(paths)?;
    if !path.is_file() {
        return Ok(None);
    }
    let config = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    validate_syntax(&config)?;
    Ok(Some(json!({ "config": config })))
}

pub(crate) fn write(
    paths: &AgentPaths,
    provider: &Provider,
    settings: &Value,
) -> Result<(), AppError> {
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::Restore("Grok Build provider settings require a config string".to_string())
        })?;
    if provider.category.as_deref() == Some("official") {
        validate_syntax(config)?;
    } else {
        validate_provider_config(config)?;
    }
    atomic_write(&config_path(paths)?, config.as_bytes())
}

pub(crate) fn strip_mcp_servers(settings: &mut Value) -> Result<(), AppError> {
    let Some(config) = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(());
    };
    if !config.contains("mcp") {
        return Ok(());
    }
    let mut document = if config.trim().is_empty() {
        DocumentMut::new()
    } else {
        config.parse::<DocumentMut>().map_err(|error| {
            AppError::Restore(format!("invalid Grok Build config.toml: {error}"))
        })?
    };
    let mut changed = document.as_table_mut().remove("mcp_servers").is_some();
    if let Some(mcp) = document.get_mut("mcp").and_then(Item::as_table_like_mut) {
        changed |= mcp.remove("servers").is_some();
        if mcp.is_empty() {
            document.as_table_mut().remove("mcp");
        }
    }
    if changed {
        settings
            .as_object_mut()
            .expect("provider settings already contained config")
            .insert("config".to_string(), Value::String(document.to_string()));
    }
    Ok(())
}

pub(crate) fn capture_unknown_mcp_servers(
    paths: &AgentPaths,
    known_ids: &HashSet<String>,
) -> Result<Table, AppError> {
    let path = config_path(paths)?;
    if !path.is_file() {
        return Ok(Table::new());
    }
    let config = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let document = if config.trim().is_empty() {
        DocumentMut::new()
    } else {
        config.parse::<DocumentMut>().map_err(|error| {
            AppError::Restore(format!("invalid Grok Build config.toml: {error}"))
        })?
    };
    let Some(item) = document.get("mcp_servers") else {
        return Ok(Table::new());
    };
    let servers = item.as_table().ok_or_else(|| {
        AppError::Restore("Grok Build mcp_servers must be a TOML table".to_string())
    })?;
    let mut unknown = Table::new();
    for (id, server) in servers {
        if !known_ids.contains(id) {
            unknown.insert(id, server.clone());
        }
    }
    Ok(unknown)
}

pub(crate) fn restore_unknown_mcp_servers(
    paths: &AgentPaths,
    unknown: &Table,
) -> Result<(), AppError> {
    if unknown.is_empty() {
        return Ok(());
    }
    let path = config_path(paths)?;
    let config = if path.is_file() {
        fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?
    } else {
        String::new()
    };
    let mut document = if config.trim().is_empty() {
        DocumentMut::new()
    } else {
        config.parse::<DocumentMut>().map_err(|error| {
            AppError::Restore(format!("invalid Grok Build config.toml: {error}"))
        })?
    };
    if !document.contains_key("mcp_servers") {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = document["mcp_servers"].as_table_mut().ok_or_else(|| {
        AppError::Restore("Grok Build mcp_servers must be a TOML table".to_string())
    })?;
    for (id, server) in unknown {
        servers.insert(id, server.clone());
    }
    atomic_write(&path, document.to_string().as_bytes())
}

fn config_path(paths: &AgentPaths) -> Result<PathBuf, AppError> {
    Ok(paths.config_dir(Agent::GrokBuild)?.join("config.toml"))
}

fn validate_syntax(config: &str) -> Result<(), AppError> {
    if config.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Value>(config)
        .map(|_| ())
        .map_err(|error| AppError::Restore(format!("invalid Grok Build config.toml: {error}")))
}

fn validate_provider_config(config: &str) -> Result<(), AppError> {
    let document = toml::from_str::<toml::Value>(config)
        .map_err(|error| AppError::Restore(format!("invalid Grok Build config.toml: {error}")))?;
    let root = document.as_table().ok_or_else(|| {
        AppError::Restore("Grok Build config.toml must contain a table".to_string())
    })?;
    let models = root
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| AppError::Restore("Grok Build config is missing [models]".to_string()))?;
    let default_model = required_string(models, "default")?;
    let selected = root
        .get("model")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get(default_model))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::Restore(format!(
                "Grok Build config is missing [model.\"{default_model}\"]"
            ))
        })?;
    for field in ["model", "base_url", "name", "api_backend"] {
        required_string(selected, field)?;
    }
    let has_credentials = ["api_key", "env_key"].into_iter().any(|field| {
        selected
            .get(field)
            .and_then(toml::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    });
    if !has_credentials {
        return Err(AppError::Restore(
            "Grok Build config requires api_key or env_key".to_string(),
        ));
    }
    let valid_context = selected
        .get("context_window")
        .and_then(toml::Value::as_integer)
        .is_some_and(|value| value > 0);
    if !valid_context {
        return Err(AppError::Restore(
            "Grok Build context_window must be a positive integer".to_string(),
        ));
    }
    Ok(())
}

fn required_string<'a>(table: &'a toml::value::Table, field: &str) -> Result<&'a str, AppError> {
    table
        .get(field)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::Restore(format!(
                "Grok Build config requires a non-empty {field} field"
            ))
        })
}
