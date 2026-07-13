use std::collections::HashMap;
use std::time::Duration;

use cc_switchy::agent::Agent;
use cc_switchy::config::{
    ConfigStore, S3Config, SourceCatalog, SourceConfig, SourceKind, WebDavConfig,
};
use cc_switchy::progress::ProgressEvent;
use cc_switchy::tui::keymap;
use cc_switchy::tui::{
    render, App, CursorState, FocusPane, MainView, PersistedUiState, TuiCommand, ViewProvider,
    ViewSkill, ViewSource, WizardAction, WizardCommand, WizardState,
};
use cc_switchy::{Language, MessageKey};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;

fn app(language: Language, with_sources: bool) -> App {
    let mut providers = HashMap::new();
    providers.insert(
        Agent::Claude,
        vec![
            ViewProvider {
                id: "claude-a".to_string(),
                name: "Claude A".to_string(),
                category: Some("custom".to_string()),
                is_current: true,
                managed: true,
            },
            ViewProvider {
                id: "claude-b".to_string(),
                name: "Claude B".to_string(),
                category: None,
                is_current: false,
                managed: true,
            },
        ],
    );
    providers.insert(
        Agent::Codex,
        vec![ViewProvider {
            id: "codex-a".to_string(),
            name: "Codex A".to_string(),
            category: Some("official".to_string()),
            is_current: true,
            managed: true,
        }],
    );
    providers.insert(
        Agent::OpenCode,
        vec![ViewProvider {
            id: "open-managed".to_string(),
            name: "Open Managed".to_string(),
            category: None,
            is_current: false,
            managed: true,
        }],
    );
    let mut skills = HashMap::new();
    skills.insert(
        Agent::Codex,
        vec![ViewSkill {
            directory: "demo".to_string(),
            name: "Demo Skill".to_string(),
            enabled: true,
        }],
    );
    let sources = if with_sources {
        vec![
            ViewSource {
                config: SourceConfig {
                    name: "home-webdav".to_string(),
                    remote_root: "cc-switch-sync".to_string(),
                    profile: "default".to_string(),
                    kind: SourceKind::WebDav {
                        webdav: WebDavConfig {
                            base_url: "https://dav.example.test/root?token=hidden".to_string(),
                            username: "user".to_string(),
                            password: "secret".to_string(),
                        },
                    },
                },
                is_default: true,
                status: Some("✓ Connected".to_string()),
            },
            ViewSource {
                config: SourceConfig {
                    name: "work-s3".to_string(),
                    remote_root: "cc-switch-sync".to_string(),
                    profile: "work".to_string(),
                    kind: SourceKind::S3 {
                        s3: S3Config {
                            region: "auto".to_string(),
                            bucket: "backup".to_string(),
                            endpoint: "https://r2.example.test?secret=value".to_string(),
                            access_key_id: "ACCESSKEY123".to_string(),
                            secret_access_key: "secret-key".to_string(),
                        },
                    },
                },
                is_default: false,
                status: None,
            },
        ]
    } else {
        Vec::new()
    };
    App::new(
        language,
        providers,
        skills,
        sources,
        PersistedUiState::default(),
    )
}

fn draw(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| render(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw_wizard(state: &WizardState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| cc_switchy::tui::wizard::render(frame, state))
        .expect("draw wizard");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn drain(app: &mut App) {
    while app.pop_command().is_some() {}
}

#[test]
fn no_source_empty_state_is_bilingual_and_actionable() {
    let mut english = app(Language::EnUs, false);
    english.view = MainView::Sources;
    let rendered = draw(&english, 100, 30);
    assert!(rendered.contains("Run: cc-switchy --wizard"));

    let mut chinese = app(Language::ZhCn, false);
    chinese.view = MainView::Sources;
    let rendered = draw(&chinese, 100, 30);
    assert!(rendered.contains("请"));
    assert!(rendered.contains("cc-switchy --wizard"));
}

#[test]
fn provider_layout_responds_without_hiding_navigation_context() {
    let app = app(Language::EnUs, true);
    let wide = draw(&app, 140, 36);
    assert!(wide.contains("Agents"));
    assert!(wide.contains("Providers"));
    assert!(wide.contains("Details"));

    let medium = draw(&app, 100, 30);
    assert!(medium.contains("Agents"));
    assert!(medium.contains("Providers"));

    let narrow = draw(&app, 70, 24);
    assert!(narrow.contains("Agents"));
    assert!(!narrow.contains("Claude A"));

    let tiny = draw(&app, 50, 15);
    assert!(tiny.contains("Terminal is too small"));
}

#[test]
fn chinese_tui_and_wizard_render_localized_system_labels() {
    let mut app = app(Language::ZhCn, true);
    let providers = draw(&app, 140, 36);
    let compact_providers = providers.replace(' ', "");
    assert!(compact_providers.contains("智能体"));
    assert!(compact_providers.contains("供应商"));
    assert!(compact_providers.contains("详情"));
    assert!(!providers.contains("unmanaged"));

    app.view = MainView::Skills;
    assert!(draw(&app, 100, 30).replace(' ', "").contains("技能"));

    app.view = MainView::Sources;
    let sources = draw(&app, 100, 30);
    let compact_sources = sources.replace(' ', "");
    assert!(compact_sources.contains("同步源"));
    assert!(compact_sources.contains("默认"));
    assert!(!sources.contains("DEFAULT"));

    app.view = MainView::Activity;
    app.apply_progress(
        ProgressEvent::Downloading {
            artifact: "db.sql".to_string(),
            downloaded: 50,
            total: 100,
        },
        Duration::from_secs(1),
    );
    let activity = draw(&app, 100, 30);
    let compact_activity = activity.replace(' ', "");
    assert!(compact_activity.contains("正在下载db.sql"));
    assert!(compact_activity.contains("50/100字节"));

    let wizard = WizardState::new(
        Language::ZhCn,
        app.sources
            .iter()
            .map(|source| source.config.clone())
            .collect(),
        Some("home-webdav".to_string()),
    );
    let wizard_rendered = draw_wizard(&wizard, 100, 30);
    let compact_wizard = wizard_rendered.replace(' ', "");
    assert!(compact_wizard.contains("同步源向导"));
    assert!(compact_wizard.contains("默认"));
    assert!(!wizard_rendered.contains(" Sources "));
    assert!(!wizard_rendered.contains("DEFAULT"));
}

#[test]
fn agent_navigation_and_provider_actions_are_independent() {
    let mut app = app(Language::EnUs, true);
    app.focus = FocusPane::List;
    app.provider_cursors.insert(
        Agent::Claude,
        CursorState {
            selected: 1,
            scroll: 1,
        },
    );
    let action = keymap::action_for(&app, key(KeyCode::Char(']'))).expect("next Agent");
    app.update(action);
    assert_eq!(app.selected_agent(), Agent::Codex);
    assert!(app.providers[&Agent::Claude][0].is_current);
    drain(&mut app);
    app.update(cc_switchy::tui::TuiAction::PreviousAgent);
    assert_eq!(app.selected_agent(), Agent::Claude);
    assert_eq!(app.provider_cursors[&Agent::Claude].selected, 1);
    drain(&mut app);

    let action = keymap::action_for(&app, key(KeyCode::Enter)).expect("switch provider");
    app.update(action);
    assert_eq!(
        app.pop_command(),
        Some(TuiCommand::SwitchProvider {
            agent: Agent::Claude,
            provider_id: "claude-b".to_string(),
        })
    );

    app.selected_agent = app
        .agents
        .iter()
        .position(|agent| *agent == Agent::OpenCode)
        .expect("OpenCode");
    let action = keymap::action_for(&app, key(KeyCode::Enter)).expect("reapply additive");
    app.update(action);
    assert_eq!(
        app.pop_command(),
        Some(TuiCommand::ReapplyProviders {
            agent: Agent::OpenCode,
        })
    );
}

#[test]
fn sources_shortcuts_queue_session_sync_default_change_and_language() {
    let mut app = app(Language::EnUs, true);
    app.update(keymap::action_for(&app, key(KeyCode::Char('4'))).expect("Sources"));
    assert_eq!(app.view, MainView::Sources);
    drain(&mut app);
    app.update(cc_switchy::tui::TuiAction::Move(1));
    drain(&mut app);

    app.update(keymap::action_for(&app, key(KeyCode::Char('s'))).expect("sync source"));
    assert_eq!(
        app.pop_command(),
        Some(TuiCommand::SyncSource {
            source: "work-s3".to_string(),
        })
    );
    assert_eq!(app.default_source_name(), Some("home-webdav"));

    app.update(keymap::action_for(&app, key(KeyCode::Char('m'))).expect("make default"));
    assert_eq!(
        app.pop_command(),
        Some(TuiCommand::MakeDefault {
            source: "work-s3".to_string(),
        })
    );

    app.update(keymap::action_for(&app, key(KeyCode::Char('L'))).expect("language"));
    assert_eq!(app.language, Language::ZhCn);
    assert_eq!(
        app.pop_command(),
        Some(TuiCommand::ChangeLanguage(Language::ZhCn))
    );
}

#[test]
fn activity_view_shows_stage_bytes_elapsed_log_warnings_and_retry() {
    let mut app = app(Language::EnUs, true);
    app.view = MainView::Activity;
    app.apply_progress(
        ProgressEvent::Downloading {
            artifact: "db.sql".to_string(),
            downloaded: 50,
            total: 100,
        },
        Duration::from_millis(1250),
    );
    app.apply_progress(
        ProgressEvent::Warning {
            stage: "provider".to_string(),
            agent: Some("Codex".to_string()),
            message_key: MessageKey::UnexpectedError,
            detail: "invalid config".to_string(),
        },
        Duration::from_secs(2),
    );

    let rendered = draw(&app, 100, 30);
    assert!(rendered.contains("Downloading db.sql"));
    assert!(rendered.contains("50/100 bytes"));
    assert!(rendered.contains("Elapsed  2.0s"));
    assert!(rendered.contains("Failed Agents: Codex"));
    assert!(rendered.contains("r retry"));
    assert!(rendered.contains("invalid config"));
}

#[test]
fn wizard_crud_mutates_catalog_immediately_and_masks_credentials() {
    let home = TempDir::new().expect("home");
    let paths = cc_switchy::AppPaths::from_home(home.path());
    let mut catalog =
        SourceCatalog::load(ConfigStore::new(paths.config_file.clone())).expect("catalog");
    let mut wizard = WizardState::new(Language::EnUs, Vec::new(), None);
    wizard.update(WizardAction::Add);
    wizard.update(WizardAction::Confirm);
    for character in "home".chars() {
        wizard.update(WizardAction::Input(character));
    }
    wizard.update(WizardAction::NextField);
    for character in "https://dav.example.test".chars() {
        wizard.update(WizardAction::Input(character));
    }
    wizard.update(WizardAction::NextField);
    for character in "user".chars() {
        wizard.update(WizardAction::Input(character));
    }
    wizard.update(WizardAction::NextField);
    for character in "super-secret".chars() {
        wizard.update(WizardAction::Input(character));
    }
    wizard.update(WizardAction::NextField);
    wizard.update(WizardAction::NextField);
    wizard.update(WizardAction::Confirm);
    let source = match wizard.pop_command().expect("add command") {
        WizardCommand::Add(source) => source,
        _ => panic!("expected Add"),
    };
    catalog.add(source).expect("add source");
    assert_eq!(catalog.config().default_source.as_deref(), Some("home"));

    catalog
        .add(SourceConfig {
            name: "work".to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            kind: SourceKind::S3 {
                s3: S3Config {
                    region: "auto".to_string(),
                    bucket: "backup".to_string(),
                    endpoint: String::new(),
                    access_key_id: "ACCESSKEY123".to_string(),
                    secret_access_key: "s3-secret".to_string(),
                },
            },
        })
        .expect("add work");
    wizard.update_sources(
        catalog.config().sources.clone(),
        catalog.config().default_source.clone(),
    );
    wizard.update(WizardAction::Details);
    let rendered = draw_wizard(&wizard, 100, 30);
    assert!(!rendered.contains("super-secret"));
    assert!(rendered.contains("••••••••"));
    wizard.update(WizardAction::Cancel);
    wizard.update(WizardAction::Delete);
    wizard.update(WizardAction::Confirm);
    wizard.update(WizardAction::Confirm);
    let (name, replacement) = match wizard.pop_command().expect("delete command") {
        WizardCommand::Delete { name, replacement } => (name, replacement),
        _ => panic!("expected Delete"),
    };
    catalog
        .delete(&name, replacement.as_deref())
        .expect("delete default");
    assert_eq!(catalog.config().default_source.as_deref(), Some("work"));
}

#[test]
fn ui_state_preserves_sync_state_and_corruption_falls_back_to_defaults() {
    let home = TempDir::new().expect("home");
    let path = home.path().join("state.json");
    std::fs::write(&path, r#"{"lastSync":{"snapshotId":"abc"}}"#).expect("sync state");
    let mut state = PersistedUiState {
        view: MainView::Sources,
        focus: FocusPane::Details,
        agent: Some(Agent::Codex),
        selected_source: Some("work".to_string()),
        ..PersistedUiState::default()
    };
    state.provider_cursors.insert(
        Agent::Codex,
        CursorState {
            selected: 3,
            scroll: 2,
        },
    );
    state.save(&path).expect("save UI state");
    let root: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("state bytes")).expect("state JSON");
    assert_eq!(
        root.pointer("/lastSync/snapshotId")
            .and_then(|v| v.as_str()),
        Some("abc")
    );
    let loaded = PersistedUiState::load(&path);
    assert_eq!(loaded.agent, Some(Agent::Codex));
    assert_eq!(loaded.provider_cursors[&Agent::Codex].selected, 3);

    std::fs::write(&path, "{").expect("corrupt state");
    let fallback = PersistedUiState::load(&path);
    assert_eq!(fallback.view, MainView::Providers);
    assert_eq!(fallback.agent, None);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        state.save(&path).expect("replace corrupt state");
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
