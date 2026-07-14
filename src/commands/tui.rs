use std::collections::HashMap;
use std::io::{self, Stdout};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::sync::{SyncRequest, SyncService};
use crate::agent::{
    Agent, AgentPaths, AgentRepository, DeviceSettings, McpProjector, ProviderProjector,
    SkillProjector,
};
use crate::config::{ConfigStore, SourceCatalog};
use crate::progress::{NoopProgress, ProgressEvent, ProgressSink};
use crate::remote::RemoteClient;
use crate::tui::event::ActivityStatus;
use crate::tui::keymap;
use crate::tui::{render, App, PersistedUiState, TuiCommand, ViewProvider, ViewSkill, ViewSource};
use crate::{AppError, AppPaths, Language, MessageArgs, MessageKey, Translator};

pub type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter() -> Result<Self, AppError> {
        install_panic_restore();
        enable_raw_mode().map_err(|error| AppError::io("terminal", error))?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(AppError::io("terminal", error));
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

pub async fn run(
    paths: AppPaths,
    language: Language,
    source_override: Option<String>,
) -> Result<(), AppError> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|error| AppError::io("terminal", error))?;
    run_with_terminal(&mut terminal, paths, language, source_override).await
}

pub async fn run_with_terminal(
    terminal: &mut CrosstermTerminal,
    paths: AppPaths,
    language: Language,
    source_override: Option<String>,
) -> Result<(), AppError> {
    let mut persisted = PersistedUiState::load(&paths.state_file);
    if let Some(source) = source_override {
        persisted.selected_source = Some(source);
    }
    let mut app = load_app(&paths, language, persisted)?;
    let (sender, mut receiver) = mpsc::unbounded_channel::<RuntimeMessage>();
    let mut active_cancel: Option<CancellationToken> = None;
    let mut active_source_test = false;
    let mut active_started: Option<Instant> = None;
    let mut dirty = true;
    let mut last_draw = Instant::now();

    loop {
        while let Ok(message) = receiver.try_recv() {
            handle_message(
                &paths,
                &mut app,
                message,
                &mut active_cancel,
                &mut active_source_test,
                &mut active_started,
            )?;
            dirty = true;
        }
        if let Some(started) =
            active_started.filter(|_| last_draw.elapsed() >= Duration::from_millis(100))
        {
            app.progress.elapsed = started.elapsed();
            dirty = true;
        }
        if dirty {
            terminal
                .draw(|frame| render(frame, &app))
                .map_err(|error| AppError::io("terminal", error))?;
            dirty = false;
            last_draw = Instant::now();
        }

        if event::poll(Duration::from_millis(50))
            .map_err(|error| AppError::io("terminal", error))?
        {
            if let Event::Key(key) =
                event::read().map_err(|error| AppError::io("terminal", error))?
            {
                if let Some(action) = keymap::action_for(&app, key) {
                    app.update(action);
                    dirty = true;
                }
            }
        }

        while let Some(command) = app.pop_command() {
            dirty = true;
            match command {
                TuiCommand::Quit if active_cancel.is_some() => app.push_activity(
                    ActivityStatus::Warning,
                    text(app.language, MessageKey::ActivitySyncActive, []),
                ),
                TuiCommand::Quit => {
                    app.persisted().save(&paths.state_file)?;
                    return Ok(());
                }
                TuiCommand::PersistUi => app.persisted().save(&paths.state_file)?,
                TuiCommand::SyncSource { source }
                    if active_cancel.is_none() && !active_source_test =>
                {
                    let cancellation = CancellationToken::new();
                    active_started = Some(Instant::now());
                    active_cancel = Some(cancellation.clone());
                    app.progress.active = true;
                    app.set_source_status(
                        &source,
                        text(
                            app.language,
                            MessageKey::ProgressConnecting,
                            [("source", source.clone())],
                        ),
                    );
                    spawn_sync(paths.clone(), source, cancellation, sender.clone());
                }
                TuiCommand::SyncSource { .. } => app.push_activity(
                    ActivityStatus::Warning,
                    text(app.language, MessageKey::ActivityOperationRunning, []),
                ),
                TuiCommand::CancelActive => {
                    if let Some(cancellation) = &active_cancel {
                        cancellation.cancel();
                        app.push_activity(
                            ActivityStatus::Info,
                            text(app.language, MessageKey::ActivityCancelRequested, []),
                        );
                    }
                }
                TuiCommand::SwitchProvider { agent, provider_id } => {
                    spawn_provider_action(paths.clone(), agent, Some(provider_id), sender.clone());
                }
                TuiCommand::ReapplyProviders { agent } => {
                    spawn_provider_action(paths.clone(), agent, None, sender.clone());
                }
                TuiCommand::TestSource { source }
                    if active_cancel.is_none() && !active_source_test =>
                {
                    active_source_test = true;
                    app.set_source_status(
                        &source,
                        text(app.language, MessageKey::WizardTesting, []),
                    );
                    spawn_source_test(paths.clone(), source, sender.clone());
                }
                TuiCommand::TestSource { .. } => app.push_activity(
                    ActivityStatus::Warning,
                    text(app.language, MessageKey::ActivityOperationRunning, []),
                ),
                TuiCommand::MakeDefault { source } => {
                    let mut catalog =
                        SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
                    catalog.set_default(&source)?;
                    app = reload_app(&paths, app)?;
                    app.push_activity(
                        ActivityStatus::Success,
                        text(
                            app.language,
                            MessageKey::ActivityDefaultChanged,
                            [("source", source)],
                        ),
                    );
                }
                TuiCommand::ChangeLanguage(language) => {
                    let mut catalog =
                        SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
                    catalog.set_language(language)?;
                    app.language = language;
                    app.persisted().save(&paths.state_file)?;
                }
                TuiCommand::OpenWizard if active_cancel.is_none() => {
                    super::wizard::run_embedded(terminal, paths.clone(), app.language).await?;
                    app = reload_app(&paths, app)?;
                }
                TuiCommand::OpenWizard => app.push_activity(
                    ActivityStatus::Warning,
                    text(app.language, MessageKey::ActivityWizardBlocked, []),
                ),
                TuiCommand::RetryWarnings => {
                    spawn_projection_retry(paths.clone(), sender.clone());
                }
            }
        }
    }
}

enum RuntimeMessage {
    Progress(ProgressEvent),
    SyncFinished {
        source: String,
        result: Result<super::sync::SyncOutcome, AppError>,
    },
    ActionFinished(Result<ActionSuccess, AppError>),
    SourceTestFinished {
        source: String,
        result: Result<SourceTestStatus, AppError>,
    },
}

enum ActionSuccess {
    Switched { agent: Agent, provider: String },
    Reapplied { agent: Agent },
    RetryComplete,
}

enum SourceTestStatus {
    Snapshot(String),
    Empty,
}

#[derive(Clone)]
struct RuntimeProgress {
    sender: mpsc::UnboundedSender<RuntimeMessage>,
}

impl ProgressSink for RuntimeProgress {
    fn emit(&self, event: ProgressEvent) {
        let _ = self.sender.send(RuntimeMessage::Progress(event));
    }
}

fn spawn_sync(
    paths: AppPaths,
    source: String,
    cancellation: CancellationToken,
    sender: mpsc::UnboundedSender<RuntimeMessage>,
) {
    tokio::spawn(async move {
        let result = async {
            let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
            let progress: Arc<dyn ProgressSink> = Arc::new(RuntimeProgress {
                sender: sender.clone(),
            });
            let mut service = SyncService {
                paths,
                catalog,
                progress,
                cancellation,
            };
            service
                .run(SyncRequest {
                    source_name: Some(source.clone()),
                })
                .await
        }
        .await;
        let _ = sender.send(RuntimeMessage::SyncFinished { source, result });
    });
}

fn spawn_provider_action(
    paths: AppPaths,
    agent: Agent,
    provider_id: Option<String>,
    sender: mpsc::UnboundedSender<RuntimeMessage>,
) {
    tokio::task::spawn_blocking(move || {
        let progress: Arc<dyn ProgressSink> = Arc::new(RuntimeProgress {
            sender: sender.clone(),
        });
        let result = (|| {
            let database = paths.cc_switch_dir.join("cc-switch.db");
            let settings_path = paths.cc_switch_dir.join("settings.json");
            let mut settings = DeviceSettings::load(&settings_path)?;
            let agent_paths = AgentPaths::from_settings(&paths.home, &settings);
            let mut repo = AgentRepository::open(&database)?;
            let mut projector =
                ProviderProjector::new(&mut repo, &mut settings, &agent_paths, progress);
            match provider_id {
                Some(provider_id) => {
                    projector.switch_exclusive(agent, &provider_id)?;
                    Ok(ActionSuccess::Switched {
                        agent,
                        provider: provider_id,
                    })
                }
                None => {
                    projector.project_agent(agent)?;
                    Ok(ActionSuccess::Reapplied { agent })
                }
            }
        })();
        let _ = sender.send(RuntimeMessage::ActionFinished(result));
    });
}

fn spawn_projection_retry(paths: AppPaths, sender: mpsc::UnboundedSender<RuntimeMessage>) {
    tokio::task::spawn_blocking(move || {
        let progress: Arc<dyn ProgressSink> = Arc::new(RuntimeProgress {
            sender: sender.clone(),
        });
        let result = (|| {
            let database = paths.cc_switch_dir.join("cc-switch.db");
            let settings_path = paths.cc_switch_dir.join("settings.json");
            let mut settings = DeviceSettings::load(&settings_path)?;
            let agent_paths = AgentPaths::from_settings(&paths.home, &settings);
            let mut repo = AgentRepository::open(&database)?;
            let mut report = ProviderProjector::new(
                &mut repo,
                &mut settings,
                &agent_paths,
                Arc::clone(&progress),
            )
            .project_all();
            report
                .merge(McpProjector::new(&repo, &agent_paths, Arc::clone(&progress)).project_all());
            report
                .merge(SkillProjector::new(&repo, &settings, &agent_paths, progress).project_all());
            if report.warnings.is_empty() {
                Ok(ActionSuccess::RetryComplete)
            } else {
                Err(AppError::Restore(format!(
                    "projection retry completed with {} warnings",
                    report.warnings.len()
                )))
            }
        })();
        let _ = sender.send(RuntimeMessage::ActionFinished(result));
    });
}

fn spawn_source_test(
    paths: AppPaths,
    source_name: String,
    sender: mpsc::UnboundedSender<RuntimeMessage>,
) {
    tokio::spawn(async move {
        let result = async {
            let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file))?;
            let source = catalog.resolve(Some(&source_name))?.clone();
            let progress: Arc<dyn ProgressSink> = Arc::new(NoopProgress);
            let remote = RemoteClient::new(source, progress)?;
            match remote.test_connection().await? {
                Some(manifest) => Ok(SourceTestStatus::Snapshot(
                    short_id(manifest.snapshot_id()).to_string(),
                )),
                None => Ok(SourceTestStatus::Empty),
            }
        }
        .await;
        let _ = sender.send(RuntimeMessage::SourceTestFinished {
            source: source_name,
            result,
        });
    });
}

fn handle_message(
    paths: &AppPaths,
    app: &mut App,
    message: RuntimeMessage,
    active_cancel: &mut Option<CancellationToken>,
    active_source_test: &mut bool,
    active_started: &mut Option<Instant>,
) -> Result<(), AppError> {
    match message {
        RuntimeMessage::Progress(event) => {
            app.apply_progress(
                event,
                active_started
                    .map(|started| started.elapsed())
                    .unwrap_or_default(),
            );
        }
        RuntimeMessage::SyncFinished { source, result } => {
            *active_cancel = None;
            *active_started = None;
            match result {
                Ok(outcome) => {
                    let warnings = outcome.projection.warnings.len();
                    let status = format!(
                        "{} · {}",
                        text(
                            app.language,
                            MessageKey::ActivitySnapshot,
                            [("snapshot", short_id(&outcome.snapshot_id).to_string())],
                        ),
                        text(
                            app.language,
                            MessageKey::ActivitySyncFinished,
                            [("warnings", warnings.to_string())],
                        )
                    );
                    *app = reload_app(paths, std::mem::replace(app, empty_app(app.language)))?;
                    app.set_source_status(&source, status);
                    app.push_activity(
                        if warnings == 0 {
                            ActivityStatus::Success
                        } else {
                            ActivityStatus::Warning
                        },
                        text(
                            app.language,
                            MessageKey::ActivitySyncFinished,
                            [("warnings", warnings.to_string())],
                        ),
                    );
                }
                Err(error) => {
                    let detail = error.to_string();
                    app.set_source_status(&source, format!("× {detail}"));
                    app.push_activity(ActivityStatus::Error, detail);
                }
            }
        }
        RuntimeMessage::ActionFinished(result) => match result {
            Ok(success) => {
                *app = reload_app(paths, std::mem::replace(app, empty_app(app.language)))?;
                let message = match success {
                    ActionSuccess::Switched { agent, provider } => text(
                        app.language,
                        MessageKey::ActivitySwitched,
                        [("agent", agent.to_string()), ("provider", provider)],
                    ),
                    ActionSuccess::Reapplied { agent } => text(
                        app.language,
                        MessageKey::ActivityReapplied,
                        [("agent", agent.to_string())],
                    ),
                    ActionSuccess::RetryComplete => {
                        text(app.language, MessageKey::ActivityRetryComplete, [])
                    }
                };
                app.push_activity(ActivityStatus::Success, message);
            }
            Err(error) => app.push_activity(ActivityStatus::Error, error.to_string()),
        },
        RuntimeMessage::SourceTestFinished { source, result } => {
            *active_source_test = false;
            let status = match result {
                Ok(SourceTestStatus::Snapshot(snapshot)) => text(
                    app.language,
                    MessageKey::ActivitySnapshot,
                    [("snapshot", snapshot)],
                ),
                Ok(SourceTestStatus::Empty) => {
                    text(app.language, MessageKey::ActivityConnectedEmpty, [])
                }
                Err(error) => format!("× {error}"),
            };
            app.set_source_status(&source, status.clone());
            app.push_activity(ActivityStatus::Info, format!("{source}: {status}"));
        }
    }
    Ok(())
}

fn load_app(
    paths: &AppPaths,
    language: Language,
    persisted: PersistedUiState,
) -> Result<App, AppError> {
    let catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    let default_source = catalog.config().default_source.as_deref();
    let sources = catalog
        .config()
        .sources
        .iter()
        .cloned()
        .map(|config| ViewSource {
            is_default: default_source == Some(config.name.as_str()),
            config,
            status: None,
        })
        .collect::<Vec<_>>();
    let mut providers = HashMap::new();
    let mut skills = HashMap::new();
    let database = paths.cc_switch_dir.join("cc-switch.db");
    if database.is_file() {
        let repo = AgentRepository::open(&database)?;
        let settings = DeviceSettings::load(&paths.cc_switch_dir.join("settings.json"))?;
        let installed = repo.installed_skills()?;
        for agent in Agent::ALL {
            let database_current = repo.database_current_provider(agent)?;
            let effective_current = settings
                .current_provider(agent)
                .filter(|id| repo.provider(agent, id).ok().flatten().is_some())
                .map(str::to_string)
                .or(database_current);
            providers.insert(
                agent,
                repo.providers(agent)?
                    .into_iter()
                    .map(|provider| ViewProvider {
                        is_current: effective_current.as_deref() == Some(provider.id.as_str()),
                        id: provider.id,
                        name: provider.name,
                        category: provider.category,
                        managed: provider.meta.live_config_managed(),
                    })
                    .collect(),
            );
            skills.insert(
                agent,
                installed
                    .iter()
                    .map(|skill| ViewSkill {
                        directory: skill.directory.clone(),
                        name: skill.name.clone(),
                        enabled: skill.enabled_for(agent),
                    })
                    .collect(),
            );
        }
    }
    Ok(App::new(language, providers, skills, sources, persisted))
}

fn reload_app(paths: &AppPaths, old: App) -> Result<App, AppError> {
    let persisted = old.persisted();
    let language = old.language;
    let mut app = load_app(paths, language, persisted)?;
    app.preserve_source_statuses_from(&old);
    app.progress = old.progress;
    Ok(app)
}

fn empty_app(language: Language) -> App {
    App::new(
        language,
        HashMap::new(),
        HashMap::new(),
        Vec::new(),
        PersistedUiState::default(),
    )
}

fn short_id(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

fn text<const N: usize>(
    language: Language,
    key: MessageKey,
    values: [(&'static str, String); N],
) -> String {
    let mut args = MessageArgs::default();
    for (name, value) in values {
        args.0.insert(name, value);
    }
    Translator::new(language).text(key, &args)
}

fn install_panic_restore() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            previous(info);
        }));
    });
}

fn restore_terminal() {
    let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SourceConfig, SourceKind, WebDavConfig};
    use tempfile::TempDir;

    fn paths_with_source(home: &TempDir, name: &str) -> AppPaths {
        let paths = AppPaths::from_home(home.path());
        let mut catalog =
            SourceCatalog::load(ConfigStore::new(paths.config_file.clone())).expect("catalog");
        catalog
            .add(SourceConfig {
                name: name.to_string(),
                remote_root: "cc-switch-sync".to_string(),
                profile: "default".to_string(),
                kind: SourceKind::WebDav {
                    webdav: WebDavConfig {
                        base_url: "https://dav.example.test".to_string(),
                        username: "user".to_string(),
                        password: "secret".to_string(),
                    },
                },
            })
            .expect("add source");
        paths
    }

    #[test]
    fn source_test_completion_finishes_test_without_changing_global_progress() {
        let home = TempDir::new().expect("home");
        let paths = paths_with_source(&home, "home");
        let mut app = load_app(&paths, Language::EnUs, PersistedUiState::default()).expect("app");
        app.progress.active = true;
        let mut cancel = None;
        let mut active_source_test = true;
        let mut started = None;

        handle_message(
            &paths,
            &mut app,
            RuntimeMessage::SourceTestFinished {
                source: "home".to_string(),
                result: Ok(SourceTestStatus::Snapshot("abc123".to_string())),
            },
            &mut cancel,
            &mut active_source_test,
            &mut started,
        )
        .expect("message");

        assert!(!active_source_test);
        assert!(app.progress.active);
        assert_eq!(app.sources[0].status.as_deref(), Some("✓ Snapshot abc123"));
    }

    #[test]
    fn sync_completion_reloads_data_and_keeps_result_on_the_source() {
        let home = TempDir::new().expect("home");
        let paths = paths_with_source(&home, "home");
        let mut app = load_app(&paths, Language::EnUs, PersistedUiState::default()).expect("app");
        app.progress.active = true;
        let mut cancel = Some(CancellationToken::new());
        let mut active_source_test = false;
        let mut started = Some(Instant::now());

        handle_message(
            &paths,
            &mut app,
            RuntimeMessage::SyncFinished {
                source: "home".to_string(),
                result: Ok(super::super::sync::SyncOutcome {
                    source_name: "home".to_string(),
                    snapshot_id: "abcdef1234567890".to_string(),
                    backup_dir: paths.backups_dir.join("backup"),
                    projection: crate::agent::ProjectionReport::default(),
                    duration: Duration::from_secs(1),
                }),
            },
            &mut cancel,
            &mut active_source_test,
            &mut started,
        )
        .expect("message");

        assert!(cancel.is_none());
        assert!(started.is_none());
        assert_eq!(
            app.sources[0].status.as_deref(),
            Some("✓ Snapshot abcdef123456 · Sync finished with 0 warning(s).")
        );
    }

    #[test]
    fn sync_failure_is_visible_on_the_source() {
        let home = TempDir::new().expect("home");
        let paths = paths_with_source(&home, "home");
        let mut app = load_app(&paths, Language::EnUs, PersistedUiState::default()).expect("app");
        let mut cancel = Some(CancellationToken::new());
        let mut active_source_test = false;
        let mut started = Some(Instant::now());

        handle_message(
            &paths,
            &mut app,
            RuntimeMessage::SyncFinished {
                source: "home".to_string(),
                result: Err(AppError::Cancelled),
            },
            &mut cancel,
            &mut active_source_test,
            &mut started,
        )
        .expect("message");

        assert_eq!(
            app.sources[0].status.as_deref(),
            Some("× synchronization was cancelled")
        );
    }
}
