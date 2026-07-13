//! Provider projection adapted from CC Switch's live provider writers.
//!
//! Upstream reference: CC Switch commit
//! c6197ae32450cd70e2bf03b35e3f5f53ac12044c (MIT).

mod additive;
mod claude;
mod claude_desktop;
mod codex;
mod gemini;
mod hermes;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, Item, TableLike};

use super::{
    effective_current_provider, Agent, AgentPaths, AgentRepository, DeviceSettings,
    ProjectionReport, ProjectionStage, ProjectionWarning, Provider,
};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::{AppError, MessageKey};

pub struct ProviderProjector<'a> {
    repo: &'a mut AgentRepository,
    settings: &'a mut DeviceSettings,
    paths: &'a AgentPaths,
    progress: Arc<dyn ProgressSink>,
}

enum ProjectOutcome {
    Applied,
    Skipped(Option<String>),
}

impl<'a> ProviderProjector<'a> {
    pub fn new(
        repo: &'a mut AgentRepository,
        settings: &'a mut DeviceSettings,
        paths: &'a AgentPaths,
        progress: Arc<dyn ProgressSink>,
    ) -> Self {
        Self {
            repo,
            settings,
            paths,
            progress,
        }
    }

    pub fn project_all(&mut self) -> ProjectionReport {
        let mut report = ProjectionReport::default();
        for agent in Agent::ALL {
            match self.project_agent_internal(agent) {
                Ok(ProjectOutcome::Applied) => report.applied_agents.push(agent),
                Ok(ProjectOutcome::Skipped(message)) => {
                    report.skipped_agents.push(agent);
                    if let Some(message) = message {
                        self.record_warning(&mut report, agent, message);
                    }
                }
                Err(error) => {
                    report.skipped_agents.push(agent);
                    self.record_warning(&mut report, agent, error.to_string());
                }
            }
        }
        report
    }

    pub fn project_agent(&mut self, agent: Agent) -> Result<(), AppError> {
        self.project_agent_internal(agent).map(|_| ())
    }

    pub fn switch_exclusive(&mut self, agent: Agent, provider_id: &str) -> Result<(), AppError> {
        if agent.is_additive() {
            return Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "exclusive provider switching",
            });
        }

        let target = self.repo.provider(agent, provider_id)?.ok_or_else(|| {
            AppError::DatabaseValidation(format!(
                "provider {provider_id} does not exist for {agent}"
            ))
        })?;
        let settings_before = self.settings.clone();
        let database_current_before = self.repo.database_current_provider(agent)?;
        let old_provider = settings_before
            .current_provider(agent)
            .and_then(|id| self.repo.provider(agent, id).transpose())
            .transpose()?
            .or_else(|| {
                database_current_before
                    .as_deref()
                    .and_then(|id| self.repo.provider(agent, id).ok().flatten())
            });
        let old_provider_settings = old_provider
            .as_ref()
            .map(|provider| (provider.id.clone(), provider.settings_config.clone()));
        let settings_path = self.settings_path();
        let mut managed_paths = managed_paths(self.paths, agent)?;
        managed_paths.push(settings_path.clone());
        let files_before = MultiFileBackup::capture(&managed_paths)?;

        let result = (|| {
            if old_provider.as_ref().is_some_and(|old| old.id != target.id) {
                if let Some(live) = read_live_settings(self.paths, agent)? {
                    let old_id = &old_provider.as_ref().expect("checked above").id;
                    self.repo.update_provider_settings(agent, old_id, &live)?;
                }
            }

            self.repo.set_database_current_provider(agent, &target.id)?;
            self.settings
                .set_current_provider(agent, Some(target.id.as_str()));
            self.settings.save_atomic(&settings_path)?;
            self.write_exclusive_provider(agent, &target)?;
            Ok(())
        })();

        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            if let Err(rollback) = files_before.restore() {
                rollback_errors.push(rollback.to_string());
            }
            *self.settings = settings_before;
            if let Some((id, settings)) = old_provider_settings {
                if let Err(rollback) = self.repo.update_provider_settings(agent, &id, &settings) {
                    rollback_errors.push(rollback.to_string());
                }
            }
            if let Err(rollback) = self
                .repo
                .restore_database_current_provider(agent, database_current_before.as_deref())
            {
                rollback_errors.push(rollback.to_string());
            }
            if rollback_errors.is_empty() {
                return Err(error);
            }
            return Err(AppError::Rollback(format!(
                "{error}; rollback errors: {}",
                rollback_errors.join("; ")
            )));
        }

        if agent.supports_mcp() {
            let projector =
                super::McpProjector::new(self.repo, self.paths, Arc::clone(&self.progress));
            if let Err(error) = projector.project_agent(agent) {
                // The provider switch is already committed. MCP is a follow-up
                // projection, so surface a structured Activity warning without
                // rolling a working provider back.
                self.progress.emit(ProgressEvent::Warning {
                    stage: "mcp".to_string(),
                    agent: Some(agent.to_string()),
                    message_key: MessageKey::UnexpectedError,
                    detail: error.to_string(),
                });
            }
        }

        Ok(())
    }

    fn project_agent_internal(&mut self, agent: Agent) -> Result<ProjectOutcome, AppError> {
        self.progress.emit(ProgressEvent::ApplyingProvider {
            agent: agent.to_string(),
        });

        if agent.is_additive() {
            let providers = self
                .repo
                .providers(agent)?
                .into_iter()
                .filter(|provider| provider.meta.live_config_managed())
                .collect::<Vec<_>>();
            if providers.is_empty() {
                return Ok(ProjectOutcome::Skipped(None));
            }
            match agent {
                Agent::OpenCode | Agent::OpenClaw => {
                    additive::write(self.paths, agent, &providers)?;
                }
                Agent::Hermes => hermes::write(self.paths, &providers)?,
                _ => unreachable!("additive Agent set is exhaustive"),
            }
            return Ok(ProjectOutcome::Applied);
        }

        let Some(provider) = effective_current_provider(self.repo, self.settings, agent)? else {
            return Ok(ProjectOutcome::Skipped(None));
        };
        if !provider.meta.live_config_managed() {
            return Ok(ProjectOutcome::Skipped(None));
        }
        if agent == Agent::ClaudeDesktop {
            return match claude_desktop::write(self.paths, &provider)? {
                Some(warning) => Ok(ProjectOutcome::Skipped(Some(warning))),
                None => Ok(ProjectOutcome::Applied),
            };
        }

        self.write_exclusive_provider(agent, &provider)?;
        Ok(ProjectOutcome::Applied)
    }

    fn write_exclusive_provider(&self, agent: Agent, provider: &Provider) -> Result<(), AppError> {
        let effective = self.effective_settings(agent, provider)?;
        match agent {
            Agent::Claude => claude::write(self.paths, &effective),
            Agent::Codex => codex::write(self.paths, &effective),
            Agent::Gemini => gemini::write(self.paths, &effective),
            Agent::ClaudeDesktop => claude_desktop::write(self.paths, provider).map(|_| ()),
            Agent::OpenCode | Agent::OpenClaw | Agent::Hermes => {
                Err(AppError::UnsupportedAgentFeature {
                    agent: agent.to_string(),
                    feature: "exclusive provider switching",
                })
            }
        }
    }

    fn effective_settings(&self, agent: Agent, provider: &Provider) -> Result<Value, AppError> {
        if !provider.meta.common_config_enabled() {
            return Ok(provider.settings_config.clone());
        }
        let key = format!("common_config_{}", agent.db_key());
        let Some(snippet) = self.repo.setting(&key)? else {
            return Ok(provider.settings_config.clone());
        };
        if snippet.trim().is_empty() {
            return Ok(provider.settings_config.clone());
        }

        match agent {
            Agent::Claude => {
                let common: Value = serde_json::from_str(&snippet).map_err(|error| {
                    AppError::Restore(format!("invalid Claude common config: {error}"))
                })?;
                let mut settings = provider.settings_config.clone();
                json_deep_merge(&mut settings, &common);
                Ok(settings)
            }
            Agent::Codex => merge_codex_common(&provider.settings_config, &snippet),
            Agent::Gemini => {
                let common: Value = serde_json::from_str(&snippet).map_err(|error| {
                    AppError::Restore(format!("invalid Gemini common config: {error}"))
                })?;
                let mut settings = provider.settings_config.clone();
                let object = settings.as_object_mut().ok_or_else(|| {
                    AppError::Restore("Gemini provider settings must be an object".to_string())
                })?;
                let env = object
                    .entry("env".to_string())
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                json_deep_merge(env, &common);
                Ok(settings)
            }
            Agent::ClaudeDesktop | Agent::OpenCode | Agent::OpenClaw | Agent::Hermes => {
                Ok(provider.settings_config.clone())
            }
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.settings
            .loaded_from()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.paths.home().join(".cc-switch/settings.json"))
    }

    fn record_warning(&self, report: &mut ProjectionReport, agent: Agent, message: String) {
        self.progress.emit(ProgressEvent::Warning {
            stage: "provider".to_string(),
            agent: Some(agent.to_string()),
            message_key: MessageKey::UnexpectedError,
            detail: message.clone(),
        });
        report.warnings.push(ProjectionWarning {
            stage: ProjectionStage::Provider,
            agent: Some(agent),
            message,
        });
    }
}

fn merge_codex_common(settings: &Value, snippet: &str) -> Result<Value, AppError> {
    let mut result = settings.clone();
    let object = result.as_object_mut().ok_or_else(|| {
        AppError::Restore("Codex provider settings must be an object".to_string())
    })?;
    let config = object.get("config").and_then(Value::as_str).unwrap_or("");
    let mut target = if config.trim().is_empty() {
        DocumentMut::new()
    } else {
        config
            .parse::<DocumentMut>()
            .map_err(|error| AppError::Restore(format!("invalid Codex config.toml: {error}")))?
    };
    let common = snippet.parse::<DocumentMut>().map_err(|error| {
        AppError::Restore(format!("invalid Codex common config snippet: {error}"))
    })?;
    merge_toml_table(target.as_table_mut(), common.as_table());
    object.insert("config".to_string(), Value::String(target.to_string()));
    Ok(result)
}

fn merge_toml_table(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let (Some(target_table), Some(source_table)) =
        (target.as_table_like_mut(), source.as_table_like())
    {
        merge_toml_table(target_table, source_table);
    } else {
        *target = source.clone();
    }
}

fn json_deep_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(key) {
                    Some(existing) => json_deep_merge(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, source) => *target = source.clone(),
    }
}

fn managed_paths(paths: &AgentPaths, agent: Agent) -> Result<Vec<PathBuf>, AppError> {
    match agent {
        Agent::Claude => claude::managed_paths(paths),
        Agent::Codex => codex::managed_paths(paths),
        Agent::Gemini => gemini::managed_paths(paths),
        Agent::ClaudeDesktop => claude_desktop::managed_paths(paths),
        Agent::OpenCode | Agent::OpenClaw | Agent::Hermes => {
            Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "exclusive provider switching",
            })
        }
    }
}

fn read_live_settings(paths: &AgentPaths, agent: Agent) -> Result<Option<Value>, AppError> {
    match agent {
        Agent::Claude => claude::read(paths),
        Agent::Codex => codex::read(paths),
        Agent::Gemini => gemini::read(paths),
        Agent::ClaudeDesktop => Ok(None),
        Agent::OpenCode | Agent::OpenClaw | Agent::Hermes => Ok(None),
    }
}

pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|error| AppError::Restore(error.to_string()))?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Restore(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    let old_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|error| AppError::io(parent, error))?;
    temporary
        .write_all(bytes)
        .map_err(|error| AppError::io(temporary.path(), error))?;
    temporary
        .flush()
        .map_err(|error| AppError::io(temporary.path(), error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| AppError::io(temporary.path(), error))?;
    if let Some(permissions) = old_permissions {
        fs::set_permissions(temporary.path(), permissions)
            .map_err(|error| AppError::io(temporary.path(), error))?;
    }
    temporary
        .persist(path)
        .map_err(|error| AppError::io(path, error.error))?;
    sync_directory(parent)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), AppError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| AppError::io(path, error))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

pub(crate) struct MultiFileBackup {
    files: Vec<FileSnapshot>,
}

struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl MultiFileBackup {
    pub(crate) fn capture(paths: &[PathBuf]) -> Result<Self, AppError> {
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let content = if path.exists() {
                Some(fs::read(path).map_err(|error| AppError::io(path, error))?)
            } else {
                None
            };
            files.push(FileSnapshot {
                path: path.clone(),
                content,
            });
        }
        Ok(Self { files })
    }

    pub(crate) fn restore(&self) -> Result<(), AppError> {
        let mut errors = Vec::new();
        for snapshot in self.files.iter().rev() {
            let result = match &snapshot.content {
                Some(content) => atomic_write(&snapshot.path, content),
                None if snapshot.path.exists() => fs::remove_file(&snapshot.path)
                    .map_err(|error| AppError::io(&snapshot.path, error)),
                None => Ok(()),
            };
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::Rollback(errors.join("; ")))
        }
    }
}

pub(crate) fn with_file_rollback<T>(
    paths: &[PathBuf],
    operation: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let backup = MultiFileBackup::capture(paths)?;
    match operation() {
        Ok(value) => Ok(value),
        Err(error) => match backup.restore() {
            Ok(()) => Err(error),
            Err(rollback) => Err(AppError::Rollback(format!(
                "{error}; rollback failed: {rollback}"
            ))),
        },
    }
}
