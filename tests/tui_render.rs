use std::collections::HashMap;
use std::time::Duration;

use cc_switchy::agent::Agent;
use cc_switchy::config::{
    ConfigStore, S3Config, SourceCatalog, SourceConfig, SourceKind, WebDavConfig,
};
use cc_switchy::progress::ProgressEvent;
use cc_switchy::tui::keymap;
use cc_switchy::tui::wizard;
use cc_switchy::tui::{
    render, App, CursorState, FocusPane, MainView, PersistedUiState, TuiCommand, ViewProvider,
    ViewSkill, ViewSource, WizardAction, WizardCommand, WizardMode, WizardState,
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

fn draw_wizard_with_cursor(
    state: &WizardState,
    width: u16,
    height: u16,
) -> (String, bool, ratatui::layout::Position) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| cc_switchy::tui::wizard::render(frame, state))
        .expect("draw wizard");
    let text = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (
        text,
        terminal.backend().cursor_visible(),
        terminal.backend().cursor_position(),
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn drain(app: &mut App) {
    while app.pop_command().is_some() {}
}

#[test]
fn wizard_form_treats_command_letters_as_text() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    for character in "qjkaextmwsL".chars() {
        let action = wizard::action_for_key(&state, key(KeyCode::Char(character)))
            .expect("form input action");
        state.update(action);
    }

    assert_eq!(state.form_values()[0], "qjkaextmwsL");
    assert_eq!(state.mode, WizardMode::EditWebDav);
    assert!(state.pop_command().is_none());
}

#[test]
fn wizard_exit_and_back_keys_follow_mode() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    let q = wizard::action_for_key(&state, key(KeyCode::Char('q'))).expect("q input");
    state.update(q);
    assert_eq!(state.form_values()[0], "q");

    let escape = wizard::action_for_key(&state, key(KeyCode::Esc)).expect("escape");
    state.update(escape);
    assert_eq!(state.mode, WizardMode::List);

    state.update(WizardAction::Add);
    let quit = wizard::action_for_key(&state, key(KeyCode::Char('q'))).expect("quit");
    state.update(quit);
    assert!(matches!(state.pop_command(), Some(WizardCommand::Exit)));
}

#[test]
fn control_c_exits_from_a_wizard_form() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    let control_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

    let action = wizard::action_for_key(&state, control_c).expect("Ctrl+C");
    state.update(action);

    assert!(matches!(state.pop_command(), Some(WizardCommand::Exit)));
}

#[test]
fn wizard_keeps_form_values_when_catalog_mutation_fails() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    for character in "duplicate".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "https://dav.example.test".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "user".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "secret".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    state.update(WizardAction::NextField);
    let before = state.form_values();

    state.update(WizardAction::Confirm);
    assert!(matches!(state.pop_command(), Some(WizardCommand::Add(_))));
    state.mutation_failed("source already exists".to_string());

    assert_eq!(state.mode, WizardMode::EditWebDav);
    assert_eq!(state.form_values(), before);
    assert!(state
        .status
        .as_deref()
        .unwrap_or_default()
        .contains("already exists"));
}

#[test]
fn wizard_clears_form_only_after_catalog_mutation_succeeds() {
    let source = SourceConfig {
        name: "home".to_string(),
        remote_root: "cc-switch-sync".to_string(),
        profile: "default".to_string(),
        kind: SourceKind::WebDav {
            webdav: WebDavConfig {
                base_url: "https://dav.example.test".to_string(),
                username: "user".to_string(),
                password: "secret".to_string(),
            },
        },
    };
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    state.mutation_succeeded(vec![source], Some("home".to_string()));

    assert_eq!(state.mode, WizardMode::List);
    assert!(state.form_values().is_empty());
    assert_eq!(state.status.as_deref(), Some("✓ Saved"));
}

#[test]
fn wizard_empty_field_has_a_visible_marker_and_cursor() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    let (rendered, cursor_visible, cursor) = draw_wizard_with_cursor(&state, 100, 30);

    assert!(rendered.contains("› Name"));
    assert!(cursor_visible);
    assert!(cursor.x > 14);
    assert_eq!(cursor.y, 4);
}

#[test]
fn wizard_footer_matches_the_current_mode() {
    let list = WizardState::new(Language::EnUs, Vec::new(), None);
    assert!(draw_wizard(&list, 100, 30).contains("a add"));

    let mut form = WizardState::new(Language::ZhCn, Vec::new(), None);
    form.update(WizardAction::Add);
    form.update(WizardAction::Confirm);
    let rendered = draw_wizard(&form, 100, 30);
    let compact = rendered.replace(' ', "");
    assert!(compact.contains("输入"));
    assert!(!compact.contains("a添加"));
}

#[test]
fn responsive_views_always_render_the_focused_pane() {
    let mut app = app(Language::EnUs, true);
    app.focus = FocusPane::Details;
    let providers = draw(&app, 100, 30);
    assert!(providers.contains("› Details"));

    app.view = MainView::Skills;
    app.focus = FocusPane::Agents;
    let agents = draw(&app, 70, 24);
    assert!(agents.contains("› Agents"));

    app.focus = FocusPane::List;
    let skills = draw(&app, 70, 24);
    assert!(skills.contains("› Skills"));
}

#[test]
fn wizard_form_error_remains_visible_with_contextual_help() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    state.mutation_failed("source already exists".to_string());

    let rendered = draw_wizard(&state, 70, 24);

    assert!(rendered.contains("source already exists"));
    assert!(rendered.contains("Ctrl+C exit"));
}

#[test]
fn wizard_secret_field_stays_masked_while_showing_the_cursor() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    for _ in 0..3 {
        state.update(WizardAction::NextField);
    }
    for character in "qsecret".chars() {
        state.update(WizardAction::Input(character));
    }

    let (rendered, cursor_visible, _) = draw_wizard_with_cursor(&state, 100, 30);

    assert!(!rendered.contains("qsecret"));
    assert!(rendered.contains("••••••••"));
    assert!(cursor_visible);
}

#[test]
fn main_footer_only_advertises_actions_for_the_current_view() {
    let mut app = app(Language::EnUs, true);
    let providers = draw(&app, 100, 30);
    assert!(providers.contains("Enter apply"));

    app.view = MainView::Skills;
    let skills = draw(&app, 100, 30);
    assert!(!skills.contains("Enter apply"));
    assert!(skills.contains("q quit"));

    app.view = MainView::Sources;
    let sources = draw(&app, 100, 30);
    assert!(sources.contains("t test"));
    assert!(sources.contains("m default"));

    app.view = MainView::Activity;
    let activity = draw(&app, 100, 30);
    assert!(!activity.contains("r retry"));
    app.progress.retry_available = true;
    let retry = draw(&app, 100, 30);
    assert!(retry.contains("r retry"));
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
