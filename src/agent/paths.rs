use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{Agent, DeviceSettings};
use crate::AppError;

#[derive(Clone)]
pub struct AgentPaths {
    home: PathBuf,
    overrides: HashMap<Agent, PathBuf>,
}

impl AgentPaths {
    pub fn from_settings(home: &Path, settings: &DeviceSettings) -> Self {
        let overrides = Agent::ALL
            .into_iter()
            .filter_map(|agent| {
                settings
                    .config_override(agent, home)
                    .map(|path| (agent, path))
            })
            .collect();
        Self {
            home: home.to_path_buf(),
            overrides,
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config_dir(&self, agent: Agent) -> Result<PathBuf, AppError> {
        if let Some(path) = self.overrides.get(&agent) {
            return Ok(path.clone());
        }
        match agent {
            Agent::Claude => Ok(self.home.join(".claude")),
            Agent::Codex => Ok(self.home.join(".codex")),
            Agent::Gemini => Ok(self.home.join(".gemini")),
            Agent::OpenCode => Ok(self.home.join(".config/opencode")),
            Agent::OpenClaw => Ok(self.home.join(".openclaw")),
            Agent::Hermes => Ok(default_hermes_dir(&self.home)),
            Agent::ClaudeDesktop => claude_desktop_dir(&self.home),
        }
    }

    pub fn skills_dir(&self, agent: Agent) -> Result<PathBuf, AppError> {
        if !agent.supports_skills() {
            return Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "Skills",
            });
        }
        Ok(self.config_dir(agent)?.join("skills"))
    }
}

fn default_hermes_dir(home: &Path) -> PathBuf {
    if let Some(value) = std::env::var_os("HERMES_HOME") {
        if !value.to_string_lossy().trim().is_empty() {
            return PathBuf::from(value);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.to_string_lossy().trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"));
        return local.join("hermes");
    }

    #[cfg(not(target_os = "windows"))]
    home.join(".hermes")
}

#[cfg(target_os = "macos")]
fn claude_desktop_dir(home: &Path) -> Result<PathBuf, AppError> {
    Ok(home.join("Library/Application Support/Claude-3p"))
}

#[cfg(target_os = "windows")]
fn claude_desktop_dir(home: &Path) -> Result<PathBuf, AppError> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"));
    Ok(local.join("Claude-3p"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn claude_desktop_dir(_home: &Path) -> Result<PathBuf, AppError> {
    Err(AppError::UnsupportedAgentFeature {
        agent: Agent::ClaudeDesktop.to_string(),
        feature: "live configuration",
    })
}
