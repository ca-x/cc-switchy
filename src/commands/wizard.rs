use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use super::tui::{CrosstermTerminal, TerminalGuard};
use crate::config::{ConfigStore, SourceCatalog};
use crate::progress::NoopProgress;
use crate::remote::RemoteClient;
use crate::tui::wizard::{self, WizardCommand};
use crate::tui::WizardState;
use crate::{AppError, AppPaths, Language};

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
    let mut state = WizardState::new(
        language,
        catalog.config().sources.clone(),
        catalog.config().default_source.clone(),
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
                if let Some(action) = wizard::action_for_key(key) {
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
                    mutate_catalog(&paths, &mut state, |catalog| catalog.add(source))?;
                }
                WizardCommand::Update { original, source } => {
                    mutate_catalog(&paths, &mut state, |catalog| {
                        catalog.update(&original, source)
                    })?;
                }
                WizardCommand::Delete { name, replacement } => {
                    mutate_catalog(&paths, &mut state, |catalog| {
                        catalog.delete(&name, replacement.as_deref())
                    })?;
                }
                WizardCommand::MakeDefault(name) => {
                    mutate_catalog(&paths, &mut state, |catalog| catalog.set_default(&name))?;
                }
                WizardCommand::ChangeLanguage(language) => {
                    mutate_catalog(&paths, &mut state, |catalog| catalog.set_language(language))?;
                    state.language = language;
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

fn mutate_catalog(
    paths: &AppPaths,
    state: &mut WizardState,
    mutation: impl FnOnce(&mut SourceCatalog) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    match mutation(&mut catalog) {
        Ok(()) => {
            state.update_sources(
                catalog.config().sources.clone(),
                catalog.config().default_source.clone(),
            );
            state.set_status("✓ Saved");
            Ok(())
        }
        Err(error) => {
            state.set_status(format!("× {error}"));
            Ok(())
        }
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
