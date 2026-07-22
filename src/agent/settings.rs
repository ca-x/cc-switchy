use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{Agent, AgentPaths, AgentRepository, Provider, SkillSyncMethod};
use crate::AppError;

#[derive(Clone, Default)]
pub struct DeviceSettings {
    values: Map<String, Value>,
    loaded_from: Option<PathBuf>,
}

impl DeviceSettings {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self {
                values: Map::new(),
                loaded_from: Some(path.to_path_buf()),
            });
        }
        let bytes = fs::read(path).map_err(|error| AppError::io(path, error))?;
        let values = serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| {
                AppError::Restore(format!("failed to parse {}: {error}", path.display()))
            })?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                AppError::Restore(format!("{} must contain a JSON object", path.display()))
            })?;
        Ok(Self {
            values,
            loaded_from: Some(path.to_path_buf()),
        })
    }

    pub fn current_provider(&self, agent: Agent) -> Option<&str> {
        self.values
            .get(current_provider_key(agent))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn set_current_provider(&mut self, agent: Agent, id: Option<&str>) {
        let key = current_provider_key(agent).to_string();
        match id.map(str::trim).filter(|value| !value.is_empty()) {
            Some(id) => {
                self.values.insert(key, Value::String(id.to_string()));
            }
            None => {
                self.values.remove(&key);
            }
        }
    }

    pub fn config_override(&self, agent: Agent, home: &Path) -> Option<PathBuf> {
        let key = config_dir_key(agent)?;
        let raw = self.values.get(key)?.as_str()?.trim();
        if raw.is_empty() {
            return None;
        }
        Some(expand_home(raw, home))
    }

    pub fn config_dir(&self, agent: Agent, home: &Path) -> Result<PathBuf, AppError> {
        AgentPaths::from_settings(home, self).config_dir(agent)
    }

    pub fn skills_ssot(&self, home: &Path) -> PathBuf {
        match self
            .values
            .get("skillStorageLocation")
            .and_then(Value::as_str)
        {
            Some("unified") => home.join(".agents/skills"),
            _ => home.join(".cc-switch/skills"),
        }
    }

    pub fn skill_sync_method(&self) -> SkillSyncMethod {
        match self.values.get("skillSyncMethod").and_then(Value::as_str) {
            Some("symlink") => SkillSyncMethod::Symlink,
            Some("copy") => SkillSyncMethod::Copy,
            _ => SkillSyncMethod::Auto,
        }
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), AppError> {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::Restore("settings path has no parent".to_string()))?;
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        let bytes = serde_json::to_vec_pretty(&self.values)
            .map_err(|error| AppError::Restore(error.to_string()))?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| AppError::io(parent, error))?;
        temporary
            .write_all(&bytes)
            .map_err(|error| AppError::io(temporary.path(), error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| AppError::io(temporary.path(), error))?;
        set_private_file(temporary.path())?;
        temporary
            .persist(path)
            .map_err(|error| AppError::io(path, error.error))?;
        set_private_file(path)
    }

    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    pub fn loaded_from(&self) -> Option<&Path> {
        self.loaded_from.as_deref()
    }
}

pub fn effective_current_provider(
    repo: &AgentRepository,
    settings: &mut DeviceSettings,
    agent: Agent,
) -> Result<Option<Provider>, AppError> {
    if let Some(local_id) = settings.current_provider(agent).map(str::to_string) {
        if let Some(provider) = repo.provider(agent, &local_id)? {
            return Ok(Some(provider));
        }
        settings.set_current_provider(agent, None);
        if let Some(path) = settings.loaded_from.as_deref() {
            settings.save_atomic(path)?;
        }
    }

    let Some(database_id) = repo.database_current_provider(agent)? else {
        return Ok(None);
    };
    repo.provider(agent, &database_id)
}

fn current_provider_key(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "currentProviderClaude",
        Agent::ClaudeDesktop => "currentProviderClaudeDesktop",
        Agent::Codex => "currentProviderCodex",
        Agent::Gemini => "currentProviderGemini",
        Agent::GrokBuild => "currentProviderGrokbuild",
        Agent::OpenCode => "currentProviderOpencode",
        Agent::OpenClaw => "currentProviderOpenclaw",
        Agent::Hermes => "currentProviderHermes",
    }
}

fn config_dir_key(agent: Agent) -> Option<&'static str> {
    match agent {
        Agent::Claude => Some("claudeConfigDir"),
        Agent::Codex => Some("codexConfigDir"),
        Agent::Gemini => Some("geminiConfigDir"),
        Agent::GrokBuild => Some("grokConfigDir"),
        Agent::OpenCode => Some("opencodeConfigDir"),
        Agent::OpenClaw => Some("openclawConfigDir"),
        Agent::Hermes => Some("hermesConfigDir"),
        Agent::ClaudeDesktop => None,
    }
}

fn expand_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), AppError> {
    Ok(())
}
