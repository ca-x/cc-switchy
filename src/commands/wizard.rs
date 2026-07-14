use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use super::tui::{CrosstermTerminal, TerminalGuard};
use crate::config::{BackupConfig, ConfigStore, SourceCatalog, SourceConfig};
use crate::progress::NoopProgress;
use crate::remote::RemoteClient;
use crate::tui::wizard::{self, WizardCommand};
use crate::tui::WizardState;
use crate::{AppError, AppPaths, Language, MessageArgs, MessageKey, Translator};

pub async fn run(paths: AppPaths, language: Language) -> Result<(), AppError> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|error| AppError::io("terminal", error))?;
    run_embedded(&mut terminal, paths, language).await
}

pub async fn run_embedded(
    terminal: &mut CrosstermTerminal,
    paths: AppPaths,
    language: Language,
) -> Result<(), AppError> {
    let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    let mut state = WizardState::new_with_backup(
        language,
        catalog.config().sources.clone(),
        catalog.config().default_source.clone(),
        catalog.config().backup.clone(),
    );
    let mut dirty = true;

    loop {
        if dirty {
            terminal
                .draw(|frame| wizard::render(frame, &state))
                .map_err(|error| AppError::io("terminal", error))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(100))
            .map_err(|error| AppError::io("terminal", error))?
        {
            if let Event::Key(key) =
                event::read().map_err(|error| AppError::io("terminal", error))?
            {
                if let Some(action) = wizard::action_for_key(&state, key) {
                    state.update(action);
                    dirty = true;
                }
            }
        }

        while let Some(command) = state.pop_command() {
            dirty = true;
            match command {
                WizardCommand::Exit => return Ok(()),
                WizardCommand::Add(source) => {
                    let result = mutate_catalog(&paths, |catalog| catalog.add(source));
                    finish_catalog_mutation(&mut state, result, true);
                }
                WizardCommand::Update { original, source } => {
                    let result =
                        mutate_catalog(&paths, |catalog| catalog.update(&original, source));
                    finish_catalog_mutation(&mut state, result, true);
                }
                WizardCommand::Delete { name, replacement } => {
                    let result = mutate_catalog(&paths, |catalog| {
                        catalog.delete(&name, replacement.as_deref())
                    });
                    finish_catalog_mutation(&mut state, result, false);
                }
                WizardCommand::MakeDefault(name) => {
                    let result = mutate_catalog(&paths, |catalog| catalog.set_default(&name));
                    finish_catalog_mutation(&mut state, result, false);
                }
                WizardCommand::ChangeLanguage(language) => {
                    let result = mutate_catalog(&paths, |catalog| catalog.set_language(language));
                    if result.is_ok() {
                        state.language = language;
                    }
                    finish_catalog_mutation(&mut state, result, false);
                }
                WizardCommand::ChangeBackup(backup) => {
                    let result = mutate_backup_config(&paths, backup);
                    match result {
                        Ok(backup) => state.backup_mutation_succeeded(backup),
                        Err(error) => state.backup_mutation_failed(error.to_string()),
                    }
                }
                WizardCommand::Test(name) => {
                    let result = test_source(&paths, &name).await;
                    match result {
                        Ok(status) => state.set_status(status),
                        Err(error) => state.set_status(format!("× {error}")),
                    }
                }
            }
        }
    }
}

fn mutate_backup_config(paths: &AppPaths, backup: BackupConfig) -> Result<BackupConfig, AppError> {
    let mut catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    catalog.set_backup_config(backup)?;
    Ok(catalog.config().backup.clone())
}

fn mutate_catalog(
    paths: &AppPaths,
    mutation: impl FnOnce(&mut SourceCatalog) -> Result<(), AppError>,
) -> Result<(Vec<SourceConfig>, Option<String>), AppError> {
    let mut catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    mutation(&mut catalog)?;
    Ok((
        catalog.config().sources.clone(),
        catalog.config().default_source.clone(),
    ))
}

fn finish_catalog_mutation(
    state: &mut WizardState,
    result: Result<(Vec<SourceConfig>, Option<String>), AppError>,
    close_form: bool,
) {
    match result {
        Ok((sources, default_source)) if close_form => {
            state.mutation_succeeded(sources, default_source);
        }
        Ok((sources, default_source)) => {
            state.update_sources(sources, default_source);
            state.set_status(
                Translator::new(state.language)
                    .text(MessageKey::WizardSaved, &MessageArgs::default()),
            );
        }
        Err(error) => state.mutation_failed(error.to_string()),
    }
}

async fn test_source(paths: &AppPaths, name: &str) -> Result<String, AppError> {
    let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    let source = catalog.resolve(Some(name))?.clone();
    let remote = RemoteClient::new(source, Arc::new(NoopProgress))?;
    match remote.test_connection().await? {
        Some(manifest) => Ok(format!(
            "✓ Snapshot {} · {}",
            short_id(manifest.snapshot_id()),
            manifest.manifest.created_at
        )),
        None => Ok("! Connected, no snapshot available".to_string()),
    }
}

fn short_id(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}
