//! Minimal Claude Desktop projection boundary adapted from CC Switch (MIT).

use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use serde_json::{json, Value};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::write_json;
use crate::agent::{Agent, AgentPaths, Provider};
use crate::AppError;

const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157210";

pub(crate) fn managed_paths(paths: &AgentPaths) -> Result<Vec<PathBuf>, AppError> {
    let directory = paths.config_dir(Agent::ClaudeDesktop)?;
    Ok(vec![
        directory.join("claude_desktop_config.json"),
        directory
            .join("configLibrary")
            .join(format!("{PROFILE_ID}.json")),
    ])
}

pub(crate) fn write(paths: &AgentPaths, provider: &Provider) -> Result<Option<String>, AppError> {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (paths, provider);
        Ok(Some(
            "Claude Desktop live configuration is unsupported on this platform".to_string(),
        ))
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mode = provider
            .meta
            .get("claudeDesktopMode")
            .and_then(Value::as_str)
            .unwrap_or("direct");
        if mode == "proxy" {
            return Ok(Some(
                "Claude Desktop proxy mode requires the CC Switch proxy runtime and was skipped"
                    .to_string(),
            ));
        }

        let paths = managed_paths(paths)?;
        if provider.category.as_deref() == Some("official") || mode == "official" {
            write_json(&paths[0], &json!({"deploymentMode": "1p"}))?;
            if paths[1].exists() {
                std::fs::remove_file(&paths[1]).map_err(|error| AppError::io(&paths[1], error))?;
            }
            return Ok(None);
        }

        let profile = json!({
            "id": PROFILE_ID,
            "name": "CC Switch",
            "deploymentMode": "3p",
            "providerId": provider.id,
            "settings": provider.settings_config,
        });
        write_json(
            &paths[0],
            &json!({"deploymentMode": "3p", "profileId": PROFILE_ID}),
        )?;
        write_json(&paths[1], &profile)?;
        Ok(None)
    }
}
