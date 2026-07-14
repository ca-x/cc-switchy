//! Managed Skill projection adapted from CC Switch's Skill synchronization.
//!
//! Upstream reference: CC Switch commit
//! c6197ae32450cd70e2bf03b35e3f5f53ac12044c (MIT).

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    Agent, AgentPaths, AgentRepository, DeviceSettings, InstalledSkill, ProjectionReport,
    ProjectionStage, ProjectionWarning, SkillSyncMethod,
};
use crate::progress::{ProgressEvent, ProgressSink};
use crate::{AppError, MessageKey};

const COPY_MARKER: &str = ".cc-switchy-managed";

pub struct SkillProjector<'a> {
    repo: &'a AgentRepository,
    settings: &'a DeviceSettings,
    paths: &'a AgentPaths,
    progress: Arc<dyn ProgressSink>,
}

impl<'a> SkillProjector<'a> {
    pub fn new(
        repo: &'a AgentRepository,
        settings: &'a DeviceSettings,
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

    pub fn project_all(&self) -> ProjectionReport {
        let mut report = ProjectionReport::default();
        for agent in Agent::ALL {
            if !agent.supports_skills() {
                report.skipped_agents.push(agent);
                continue;
            }
            match self.project_agent(agent) {
                Ok(()) => report.applied_agents.push(agent),
                Err(error) => {
                    report.skipped_agents.push(agent);
                    let message = error.to_string();
                    self.progress.emit(ProgressEvent::Warning {
                        stage: "skills".to_string(),
                        agent: Some(agent.to_string()),
                        message_key: MessageKey::UnexpectedError,
                        detail: message.clone(),
                    });
                    report.warnings.push(ProjectionWarning {
                        stage: ProjectionStage::Skills,
                        agent: Some(agent),
                        message,
                    });
                }
            }
        }
        report
    }

    pub fn project_agent(&self, agent: Agent) -> Result<(), AppError> {
        if !agent.supports_skills() {
            return Err(AppError::UnsupportedAgentFeature {
                agent: agent.to_string(),
                feature: "Skills",
            });
        }
        let skills = self.repo.installed_skills()?;
        let ssot = self.settings.skills_ssot(self.paths.home());
        let app_dir = self.paths.skills_dir(agent)?;
        fs::create_dir_all(&app_dir).map_err(|error| AppError::io(&app_dir, error))?;
        self.cleanup_managed_targets(&app_dir, &ssot, &skills, agent)?;

        let enabled = skills
            .iter()
            .filter(|skill| skill.enabled_for(agent))
            .collect::<Vec<_>>();
        let total = enabled.len();
        let mut errors = Vec::new();
        for (index, skill) in enabled.into_iter().enumerate() {
            if let Err(error) = self.sync_skill(&ssot, &app_dir, skill) {
                errors.push(format!("{}: {error}", skill.directory));
            }
            let name = if skill.name.trim().is_empty() {
                skill.directory.as_str()
            } else {
                skill.name.trim()
            };
            self.progress.emit(ProgressEvent::ApplyingSkills {
                agent: format!("{agent} · {name}"),
                completed: index + 1,
                total,
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::Restore(format!(
                "one or more Skills could not be projected: {}",
                errors.join("; ")
            )))
        }
    }

    fn cleanup_managed_targets(
        &self,
        app_dir: &Path,
        ssot: &Path,
        skills: &[InstalledSkill],
        agent: Agent,
    ) -> Result<(), AppError> {
        let indexed = skills
            .iter()
            .map(|skill| (skill.directory.to_lowercase(), skill))
            .collect::<HashMap<_, _>>();
        for entry in fs::read_dir(app_dir).map_err(|error| AppError::io(app_dir, error))? {
            let entry = entry.map_err(|error| AppError::io(app_dir, error))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if let Some(skill) = indexed.get(&name.to_lowercase()) {
                if !skill.enabled_for(agent) {
                    remove_path(&path)?;
                }
                continue;
            }
            if is_symlink_to_ssot(&path, ssot) || is_managed_copy(&path) {
                remove_path(&path)?;
            }
        }
        Ok(())
    }

    fn sync_skill(
        &self,
        ssot: &Path,
        app_dir: &Path,
        skill: &InstalledSkill,
    ) -> Result<(), AppError> {
        validate_directory_name(&skill.directory)?;
        let source = ssot.join(&skill.directory);
        validate_source(ssot, &source)?;
        let destination = app_dir.join(&skill.directory);
        match self.settings.skill_sync_method() {
            SkillSyncMethod::Auto => {
                if destination.exists() && !is_symlink(&destination) {
                    replace_with_copy(&source, &destination, &skill.directory)
                } else {
                    if is_symlink(&destination) {
                        remove_path(&destination)?;
                    }
                    match create_symlink(&source, &destination) {
                        Ok(()) => Ok(()),
                        Err(_) => replace_with_copy(&source, &destination, &skill.directory),
                    }
                }
            }
            SkillSyncMethod::Symlink => {
                if destination.exists() || is_symlink(&destination) {
                    remove_path(&destination)?;
                }
                create_symlink(&source, &destination)
            }
            SkillSyncMethod::Copy => replace_with_copy(&source, &destination, &skill.directory),
        }
    }
}

fn validate_directory_name(directory: &str) -> Result<(), AppError> {
    let path = Path::new(directory);
    let mut components = path.components();
    let valid = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !directory.trim().is_empty();
    if valid {
        Ok(())
    } else {
        Err(AppError::DatabaseValidation(format!(
            "unsafe Skill directory name: {directory}"
        )))
    }
}

fn validate_source(ssot: &Path, source: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| AppError::io(source, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::Restore(format!(
            "Skill source must be a real directory inside the SSOT: {}",
            source.display()
        )));
    }
    if !source.join("SKILL.md").is_file() {
        return Err(AppError::Restore(format!(
            "Skill source is missing SKILL.md: {}",
            source.display()
        )));
    }
    let canonical_ssot = ssot
        .canonicalize()
        .map_err(|error| AppError::io(ssot, error))?;
    let canonical_source = source
        .canonicalize()
        .map_err(|error| AppError::io(source, error))?;
    if !canonical_source.starts_with(&canonical_ssot) {
        return Err(AppError::Restore(format!(
            "Skill source escapes the SSOT: {}",
            source.display()
        )));
    }
    validate_source_tree(&canonical_ssot, &canonical_source)
}

fn validate_source_tree(ssot: &Path, directory: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(directory).map_err(|error| AppError::io(directory, error))? {
        let entry = entry.map_err(|error| AppError::io(directory, error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| AppError::io(&path, error))?;
        if metadata.file_type().is_symlink() {
            let target = path
                .canonicalize()
                .map_err(|error| AppError::io(&path, error))?;
            if !target.starts_with(ssot) {
                return Err(AppError::Restore(format!(
                    "Skill source symlink escapes the SSOT: {}",
                    path.display()
                )));
            }
        } else if metadata.is_dir() {
            validate_source_tree(ssot, &path)?;
        }
    }
    Ok(())
}

fn replace_with_copy(source: &Path, destination: &Path, name: &str) -> Result<(), AppError> {
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Restore(format!("{} has no parent", destination.display())))?;
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()));
    if temporary.exists() || is_symlink(&temporary) {
        remove_path(&temporary)?;
    }
    if let Err(error) = copy_directory(source, &temporary) {
        let _ = remove_path(&temporary);
        return Err(error);
    }
    fs::write(temporary.join(COPY_MARKER), b"cc-switchy\n")
        .map_err(|error| AppError::io(temporary.join(COPY_MARKER), error))?;
    if destination.exists() || is_symlink(destination) {
        remove_path(destination)?;
    }
    fs::rename(&temporary, destination).map_err(|error| {
        let _ = remove_path(&temporary);
        AppError::io(destination, error)
    })
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), AppError> {
    fs::create_dir_all(destination).map_err(|error| AppError::io(destination, error))?;
    for entry in fs::read_dir(source).map_err(|error| AppError::io(source, error))? {
        let entry = entry.map_err(|error| AppError::io(source, error))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::io(&source_path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::Restore(format!(
                "copy mode does not follow Skill source symlinks: {}",
                source_path.display()
            )));
        }
        if metadata.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| AppError::io(&destination_path, error))?;
            let _ = fs::set_permissions(&destination_path, metadata.permissions());
        }
    }
    Ok(())
}

fn is_managed_copy(path: &Path) -> bool {
    path.is_dir() && path.join(COPY_MARKER).is_file()
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_symlink_to_ssot(path: &Path, ssot: &Path) -> bool {
    if !is_symlink(path) {
        return false;
    }
    let Ok(target) = fs::read_link(path) else {
        return false;
    };
    if target.is_absolute() && target.starts_with(ssot) {
        return true;
    }
    let resolved = path
        .parent()
        .map(|parent| parent.join(&target))
        .unwrap_or(target);
    let canonical_ssot = ssot.canonicalize().unwrap_or_else(|_| ssot.to_path_buf());
    let canonical_target = resolved.canonicalize().unwrap_or(resolved);
    canonical_target.starts_with(canonical_ssot)
}

#[cfg(unix)]
fn create_symlink(source: &Path, destination: &Path) -> Result<(), AppError> {
    if std::env::var_os("CC_SWITCHY_TEST_FORCE_SYMLINK_FAILURE").is_some() {
        return Err(AppError::Restore("forced symlink failure".to_string()));
    }
    std::os::unix::fs::symlink(source, destination)
        .map_err(|error| AppError::io(destination, error))
}

#[cfg(windows)]
fn create_symlink(source: &Path, destination: &Path) -> Result<(), AppError> {
    if std::env::var_os("CC_SWITCHY_TEST_FORCE_SYMLINK_FAILURE").is_some() {
        return Err(AppError::Restore("forced symlink failure".to_string()));
    }
    std::os::windows::fs::symlink_dir(source, destination)
        .map_err(|error| AppError::io(destination, error))
}

fn remove_path(path: &Path) -> Result<(), AppError> {
    if is_symlink(path) {
        #[cfg(unix)]
        fs::remove_file(path).map_err(|error| AppError::io(path, error))?;
        #[cfg(windows)]
        fs::remove_dir(path).map_err(|error| AppError::io(path, error))?;
    } else if path.is_dir() {
        fs::remove_dir_all(path).map_err(|error| AppError::io(path, error))?;
    } else if path.exists() {
        fs::remove_file(path).map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}
