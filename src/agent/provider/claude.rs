//! Claude live settings projection adapted from CC Switch (MIT).

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use super::write_json;
use crate::agent::{Agent, AgentPaths};
use crate::AppError;

pub(crate) fn managed_paths(paths: &AgentPaths) -> Result<Vec<PathBuf>, AppError> {
    Ok(vec![paths.config_dir(Agent::Claude)?.join("settings.json")])
}

pub(crate) fn write(paths: &AgentPaths, settings: &Value) -> Result<(), AppError> {
    let mut sanitized = settings.clone();
    let object = sanitized.as_object_mut().ok_or_else(|| {
        AppError::Restore("Claude provider settings must be a JSON object".to_string())
    })?;
    for key in [
        "api_format",
        "apiFormat",
        "openrouter_compat_mode",
        "openrouterCompatMode",
    ] {
        object.remove(key);
    }
    write_json(&managed_paths(paths)?[0], &sanitized)
}

pub(crate) fn read(paths: &AgentPaths) -> Result<Option<Value>, AppError> {
    let path = &managed_paths(paths)?[0];
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| AppError::io(path, error))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| AppError::Restore(format!("invalid {}: {error}", path.display())))
}
