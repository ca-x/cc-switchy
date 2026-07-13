//! Hermes YAML provider projection adapted from CC Switch (MIT).

use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Value};

use super::atomic_write;
use crate::agent::{Agent, AgentPaths, Provider};
use crate::AppError;

pub(crate) fn write(paths: &AgentPaths, providers: &[Provider]) -> Result<(), AppError> {
    let path = config_path(paths)?;
    let mut root = read_root(&path)?;
    let mapping = root.as_mapping_mut().ok_or_else(|| {
        AppError::Restore(format!("{} must contain a YAML mapping", path.display()))
    })?;
    let key = Value::String("custom_providers".to_string());
    let sequence = mapping
        .entry(key)
        .or_insert_with(|| Value::Sequence(Vec::new()))
        .as_sequence_mut()
        .ok_or_else(|| {
            AppError::Restore("Hermes custom_providers must be a sequence".to_string())
        })?;

    for provider in providers {
        let mut incoming = serde_yaml::to_value(&provider.settings_config)
            .map_err(|error| AppError::Restore(error.to_string()))?;
        let incoming_map = incoming.as_mapping_mut().ok_or_else(|| {
            AppError::Restore(format!(
                "Hermes provider {} settings must be an object",
                provider.id
            ))
        })?;
        incoming_map.insert(
            Value::String("name".to_string()),
            Value::String(provider.id.clone()),
        );

        if let Some(existing) = sequence
            .iter_mut()
            .find(|item| item.get("name").and_then(Value::as_str) == Some(provider.id.as_str()))
        {
            if let (Some(existing_map), Some(incoming_map)) =
                (existing.as_mapping(), incoming.as_mapping_mut())
            {
                for (key, value) in existing_map {
                    incoming_map
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            *existing = incoming;
        } else {
            sequence.push(incoming);
        }
    }

    let bytes = serde_yaml::to_string(&root)
        .map_err(|error| AppError::Restore(error.to_string()))?
        .into_bytes();
    atomic_write(&path, &bytes)
}

pub(crate) fn config_path(paths: &AgentPaths) -> Result<PathBuf, AppError> {
    Ok(paths.config_dir(Agent::Hermes)?.join("config.yaml"))
}

fn read_root(path: &std::path::Path) -> Result<Value, AppError> {
    if !path.is_file() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    let content = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if content.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    serde_yaml::from_str(&content)
        .map_err(|error| AppError::Restore(format!("invalid {}: {error}", path.display())))
}
