use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, MainView, TuiAction};
use crate::Language;

pub fn action_for(app: &App, key: KeyEvent) -> Option<TuiAction> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(if app.progress.active {
            TuiAction::CancelActive
        } else {
            TuiAction::Quit
        });
    }
    match key.code {
        KeyCode::Char('q') => Some(TuiAction::Quit),
        KeyCode::Esc if app.progress.active => Some(TuiAction::CancelActive),
        KeyCode::Char('1') => Some(TuiAction::SwitchView(MainView::Providers)),
        KeyCode::Char('2') => Some(TuiAction::SwitchView(MainView::Skills)),
        KeyCode::Char('3') => Some(TuiAction::SwitchView(MainView::Activity)),
        KeyCode::Char('4') => Some(TuiAction::SwitchView(MainView::Sources)),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiAction::Move(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiAction::Move(-1)),
        KeyCode::Tab => Some(TuiAction::FocusNext),
        KeyCode::BackTab => Some(TuiAction::FocusPrevious),
        KeyCode::Left | KeyCode::Char('h') => Some(TuiAction::FocusPrevious),
        KeyCode::Right | KeyCode::Char('l') => Some(TuiAction::FocusNext),
        KeyCode::Char('[') => Some(TuiAction::PreviousAgent),
        KeyCode::Char(']') => Some(TuiAction::NextAgent),
        KeyCode::Enter if app.view == MainView::Providers => {
            let agent = app.selected_agent();
            if agent.is_additive() {
                Some(TuiAction::ReapplyProviders { agent })
            } else {
                app.selected_provider()
                    .map(|provider| TuiAction::SwitchProvider {
                        agent,
                        provider_id: provider.id.clone(),
                    })
            }
        }
        KeyCode::Char('s') => app
            .selected_source()
            .filter(|_| app.view == MainView::Sources)
            .map(|source| source.config.name.clone())
            .or_else(|| app.default_source_name().map(str::to_string))
            .map(|source| TuiAction::SyncSource { source }),
        KeyCode::Char('t') if app.view == MainView::Sources => {
            app.selected_source().map(|source| TuiAction::TestSource {
                source: source.config.name.clone(),
            })
        }
        KeyCode::Char('m') if app.view == MainView::Sources => {
            app.selected_source().map(|source| TuiAction::MakeDefault {
                source: source.config.name.clone(),
            })
        }
        KeyCode::Char('w') => Some(TuiAction::OpenWizard),
        KeyCode::Char('L') => Some(TuiAction::ChangeLanguage(match app.language {
            Language::ZhCn => Language::EnUs,
            Language::Auto | Language::EnUs => Language::ZhCn,
        })),
        KeyCode::Char('r') if app.progress.retry_available => Some(TuiAction::RetryWarnings),
        _ => None,
    }
}
