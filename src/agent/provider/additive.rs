//! Additive JSON provider projection adapted from CC Switch (MIT).

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::write_json;
use crate::agent::{Agent, AgentPaths, Provider};
use crate::AppError;

pub(crate) fn write(
    paths: &AgentPaths,
    agent: Agent,
    providers: &[Provider],
) -> Result<(), AppError> {
    let path = config_path(paths, agent)?;
    let mut root = read_root(&path, agent)?;
    let provider_map = match agent {
        Agent::OpenCode => ensure_object(&mut root)?
            .entry("provider".to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| AppError::Restore("OpenCode provider must be an object".to_string()))?,
        Agent::OpenClaw => {
            let models = ensure_object(&mut root)?
                .entry("models".to_string())
                .or_insert_with(|| json!({"mode": "merge", "providers": {}}));
            let models = models.as_object_mut().ok_or_else(|| {
                AppError::Restore("OpenClaw models must be an object".to_string())
            })?;
            models
                .entry("mode".to_string())
                .or_insert_with(|| Value::String("merge".to_string()));
            models
                .entry("providers".to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| {
                    AppError::Restore("OpenClaw models.providers must be an object".to_string())
                })?
        }
        _ => unreachable!("JSON additive Agent set is exhaustive"),
    };

    for provider in providers {
        let fragment = provider_fragment(agent, provider)?;
        provider_map.insert(provider.id.clone(), fragment);
    }
    write_json(&path, &root)
}

pub(crate) fn config_path(paths: &AgentPaths, agent: Agent) -> Result<PathBuf, AppError> {
    let file = match agent {
        Agent::OpenCode => "opencode.json",
        Agent::OpenClaw => "openclaw.json",
        _ => {
            return Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "additive JSON provider projection",
            });
        }
    };
    Ok(paths.config_dir(agent)?.join(file))
}

fn read_root(path: &std::path::Path, agent: Agent) -> Result<Value, AppError> {
    if !path.is_file() {
        return Ok(match agent {
            Agent::OpenCode => json!({"$schema": "https://opencode.ai/config.json"}),
            Agent::OpenClaw => json!({"models": {"mode": "merge", "providers": {}}}),
            _ => json!({}),
        });
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

fn provider_fragment(agent: Agent, provider: &Provider) -> Result<Value, AppError> {
    let settings = &provider.settings_config;
    if !settings.is_object() {
        return Err(AppError::Restore(format!(
            "{} provider {} settings must be an object",
            agent, provider.id
        )));
    }
    let fragment = match agent {
        Agent::OpenCode => settings
            .get("provider")
            .and_then(|providers| providers.get(&provider.id))
            .cloned()
            .unwrap_or_else(|| settings.clone()),
        Agent::OpenClaw => settings
            .pointer(&format!(
                "/models/providers/{}",
                escape_pointer(&provider.id)
            ))
            .cloned()
            .unwrap_or_else(|| settings.clone()),
        _ => settings.clone(),
    };
    Ok(fragment)
}

fn ensure_object(value: &mut Value) -> Result<&mut Map<String, Value>, AppError> {
    value
        .as_object_mut()
        .ok_or_else(|| AppError::Restore("live config root must be an object".to_string()))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
