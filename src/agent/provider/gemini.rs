//! Gemini live projection adapted from CC Switch (MIT).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::{atomic_write, with_file_rollback, write_json};
use crate::agent::{Agent, AgentPaths};
use crate::AppError;

pub(crate) fn managed_paths(paths: &AgentPaths) -> Result<Vec<PathBuf>, AppError> {
    let directory = paths.config_dir(Agent::Gemini)?;
    Ok(vec![
        directory.join(".env"),
        directory.join("settings.json"),
    ])
}

pub(crate) fn write(paths: &AgentPaths, settings: &Value) -> Result<(), AppError> {
    let object = settings.as_object().ok_or_else(|| {
        AppError::Restore("Gemini provider settings must be a JSON object".to_string())
    })?;
    let env = env_map(object.get("env"))?;
    let files = managed_paths(paths)?;
    let config = merged_config(&files[1], object.get("config"))?;
    let env_text = env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");

    with_file_rollback(&files, || {
        atomic_write(&files[0], env_text.as_bytes())?;
        if let Some(config) = &config {
            write_json(&files[1], config)?;
        }
        Ok(())
    })
}

pub(crate) fn read(paths: &AgentPaths) -> Result<Option<Value>, AppError> {
    let files = managed_paths(paths)?;
    if !files.iter().any(|path| path.is_file()) {
        return Ok(None);
    }
    let mut env = Map::new();
    if files[0].is_file() {
        let content =
            fs::read_to_string(&files[0]).map_err(|error| AppError::io(&files[0], error))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                env.insert(
                    key.trim().to_string(),
                    Value::String(value.trim().to_string()),
                );
            }
        }
    }
    let config = if files[1].is_file() {
        let bytes = fs::read(&files[1]).map_err(|error| AppError::io(&files[1], error))?;
        serde_json::from_slice(&bytes).map_err(|error| {
            AppError::Restore(format!("invalid {}: {error}", files[1].display()))
        })?
    } else {
        Value::Null
    };
    Ok(Some(json!({"env": env, "config": config})))
}

fn env_map(value: Option<&Value>) -> Result<BTreeMap<String, String>, AppError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value.as_object().ok_or_else(|| {
        AppError::Restore("Gemini provider env must be a JSON object".to_string())
    })?;
    let mut result = BTreeMap::new();
    for (key, value) in object {
        if let Some(value) = value.as_str() {
            result.insert(key.clone(), value.to_string());
        }
    }
    Ok(result)
}

fn merged_config(
    path: &std::path::Path,
    config: Option<&Value>,
) -> Result<Option<Value>, AppError> {
    let Some(config) = config else {
        return Ok(None);
    };
    if config.is_null() {
        return Ok(None);
    }
    let incoming = config.as_object().ok_or_else(|| {
        AppError::Restore("Gemini provider config must be an object or null".to_string())
    })?;
    let mut result = if path.is_file() {
        let bytes = fs::read(path).map_err(|error| AppError::io(path, error))?;
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| AppError::Restore(format!("invalid {}: {error}", path.display())))?
    } else {
        json!({})
    };
    let object = result.as_object_mut().ok_or_else(|| {
        AppError::Restore(format!("{} must contain a JSON object", path.display()))
    })?;
    for (key, value) in incoming {
        object.insert(key.clone(), value.clone());
    }
    Ok(Some(result))
}
