use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::event::ActivityStatus;
use super::{App, FocusPane, MainView};
use crate::{Language, MessageArgs, MessageKey, Translator};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < 60 || area.height < 18 {
        render_resize(frame, app.language, area);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);
    render_tabs(frame, app, rows[0]);
    match app.view {
        MainView::Providers => render_providers(frame, app, rows[1]),
        MainView::Skills => render_skills(frame, app, rows[1]),
        MainView::Activity => render_activity(frame, app, rows[1]),
        MainView::Sources => render_sources(frame, app, rows[1]),
    }
    render_footer(frame, app, rows[2]);
}

fn render_tabs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let tabs = [
        (MainView::Providers, "1", MessageKey::TuiProviders),
        (MainView::Skills, "2", MessageKey::TuiSkills),
        (MainView::Activity, "3", MessageKey::TuiActivity),
        (MainView::Sources, "4", MessageKey::TuiSources),
    ];
    let mut spans = vec![Span::styled(
        " cc-switchy  ",
        Style::default()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD),
    )];
    for (view, key, label) in tabs {
        let style = if app.view == view {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(
            format!("  {key} {}  ", translator.text(label, &args)),
            style,
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_providers(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width >= 120 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(24),
                Constraint::Percentage(42),
                Constraint::Min(32),
            ])
            .split(area);
        render_agents(frame, app, columns[0]);
        render_provider_list(frame, app, columns[1]);
        render_provider_details(frame, app, columns[2]);
    } else if area.width >= 80 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(40)])
            .split(area);
        render_agents(frame, app, columns[0]);
        render_provider_list(frame, app, columns[1]);
    } else {
        match app.focus {
            FocusPane::Agents => render_agents(frame, app, area),
            FocusPane::Details => render_provider_details(frame, app, area),
            FocusPane::List | FocusPane::Activity => render_provider_list(frame, app, area),
        }
    }
}

fn render_agents(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let items = app
        .agents
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let selected = index == app.selected_agent;
            let supported = app
                .providers
                .get(agent)
                .is_some_and(|providers| !providers.is_empty());
            ListItem::new(format!(
                "{} {} {}",
                if selected { "›" } else { " " },
                if supported { "✓" } else { "×" },
                agent
            ))
            .style(selection_style(selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &translator.text(MessageKey::TuiAgents, &MessageArgs::default()),
            app.focus == FocusPane::Agents,
        )),
        area,
    );
}

fn render_provider_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let agent = app.selected_agent();
    let cursor = app
        .provider_cursors
        .get(&agent)
        .copied()
        .unwrap_or_default();
    let providers = app
        .providers
        .get(&agent)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if providers.is_empty() {
        frame.render_widget(
            Paragraph::new(translator.text(MessageKey::TuiNoProviders, &MessageArgs::default()))
                .alignment(Alignment::Center)
                .block(pane_block(
                    &translator.text(MessageKey::TuiProviders, &MessageArgs::default()),
                    app.focus == FocusPane::List,
                )),
            area,
        );
        return;
    }
    let items = providers
        .iter()
        .enumerate()
        .skip(cursor.scroll)
        .map(|(index, provider)| {
            let selected = index == cursor.selected;
            let glyph = if agent.is_additive() {
                "◉"
            } else if provider.is_current {
                "●"
            } else {
                "○"
            };
            let managed = if provider.managed {
                String::new()
            } else {
                format!(
                    "  × {}",
                    translator.text(MessageKey::TuiUnmanaged, &MessageArgs::default())
                )
            };
            ListItem::new(format!(
                "{} {glyph} {}{managed}",
                if selected { "›" } else { " " },
                provider.name
            ))
            .style(selection_style(selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &format!(
                "{} · {agent}",
                translator.text(MessageKey::TuiProviders, &MessageArgs::default())
            ),
            app.focus == FocusPane::List,
        )),
        area,
    );
}

fn render_provider_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let text = app.selected_provider().map_or_else(
        || translator.text(MessageKey::TuiNoProviders, &args),
        |provider| {
            format!(
                "{}\n\nID\n{}\n\n{}\n{}\n\n{}\n{}",
                provider.name,
                provider.id,
                translator.text(MessageKey::TuiCategory, &args),
                provider.category.as_deref().unwrap_or("—"),
                translator.text(MessageKey::TuiStatus, &args),
                if app.selected_agent().is_additive() {
                    format!("◉ {}", translator.text(MessageKey::TuiAdditiveSet, &args))
                } else if provider.is_current {
                    format!("● {}", translator.text(MessageKey::TuiCurrent, &args))
                } else {
                    format!("○ {}", translator.text(MessageKey::TuiAvailable, &args))
                }
            )
        },
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(pane_block(
                &translator.text(MessageKey::TuiDetails, &MessageArgs::default()),
                app.focus == FocusPane::Details,
            )),
        area,
    );
}

fn render_skills(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = if area.width >= 80 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(30)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1)])
            .split(area)
    };
    if columns.len() > 1 {
        render_agents(frame, app, columns[0]);
    }
    let target = *columns.last().expect("Skills layout");
    let skills = app
        .skills
        .get(&app.selected_agent())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let items = skills
        .iter()
        .map(|skill| {
            ListItem::new(format!(
                "{} {}  {}",
                if skill.enabled { "✓" } else { "○" },
                skill.name,
                skill.directory
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &Translator::new(app.language).text(MessageKey::TuiSkills, &MessageArgs::default()),
            app.focus == FocusPane::List,
        )),
        target,
    );
}

fn render_activity(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(area);
    let mut progress = vec![Line::from(vec![
        Span::styled(
            format!("{}  ", translator.text(MessageKey::TuiStage, &args)),
            Style::default().fg(MUTED),
        ),
        Span::raw(if app.progress.stage.is_empty() {
            translator.text(MessageKey::TuiReady, &args)
        } else {
            app.progress.stage.clone()
        }),
    ])];
    progress.push(Line::from(format!(
        "{}  {:.1}s",
        translator.text(MessageKey::TuiElapsed, &args),
        app.progress.elapsed.as_secs_f64()
    )));
    for (artifact, (downloaded, total)) in &app.progress.downloads {
        let percent = downloaded
            .saturating_mul(100)
            .checked_div(*total)
            .unwrap_or(100);
        progress.push(Line::from(format!(
            "{artifact}  {downloaded}/{total} {}  {percent}%",
            translator.text(MessageKey::TuiBytes, &args)
        )));
    }
    if !app.progress.failed_agents.is_empty() {
        progress.push(Line::from(format!(
            "! {}: {} · r {}",
            translator.text(MessageKey::TuiFailedAgents, &args),
            app.progress.failed_agents.join(", "),
            translator.text(MessageKey::TuiRetry, &args)
        )));
    }
    frame.render_widget(
        Paragraph::new(progress).block(pane_block(
            &translator.text(MessageKey::TuiProgress, &args),
            true,
        )),
        rows[0],
    );
    let log = app
        .progress
        .log
        .iter()
        .map(|entry| {
            let (glyph, color) = match entry.status {
                ActivityStatus::Info => ("·", Color::Gray),
                ActivityStatus::Success => ("✓", Color::Green),
                ActivityStatus::Warning => ("!", Color::Yellow),
                ActivityStatus::Error => ("×", Color::Red),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::raw(&entry.text),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(log).block(pane_block(
            &translator.text(MessageKey::TuiActivityLog, &args),
            false,
        )),
        rows[1],
    );
}

fn render_sources(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    if app.sources.is_empty() {
        frame.render_widget(
            Paragraph::new(translator.text(MessageKey::TuiNoSources, &MessageArgs::default()))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(pane_block(
                    &translator.text(MessageKey::TuiSources, &MessageArgs::default()),
                    true,
                )),
            area,
        );
        return;
    }
    if area.width >= 80 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area);
        render_source_list(frame, app, columns[0]);
        render_source_details(frame, app, columns[1]);
    } else if app.focus == FocusPane::Details {
        render_source_details(frame, app, area);
    } else {
        render_source_list(frame, app, area);
    }
}

fn render_source_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let items = app
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let selected = index == app.selected_source;
            ListItem::new(format!(
                "{} {}  {:7} {}",
                if selected { "›" } else { " " },
                source.config.name,
                source.kind_label(),
                if source.is_default {
                    translator.text(MessageKey::TuiDefault, &MessageArgs::default())
                } else {
                    String::new()
                }
            ))
            .style(selection_style(selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &translator.text(MessageKey::TuiSources, &MessageArgs::default()),
            app.focus == FocusPane::List,
        )),
        area,
    );
}

fn render_source_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let text = app.selected_source().map_or_else(String::new, |source| {
        format!(
            "{}\n\n{}\n{}\n\n{}\n{}\n\n{}\n{}/v2/db-v6/{}\n\n{}\n{}",
            source.config.name,
            translator.text(MessageKey::TuiType, &args),
            source.kind_label(),
            translator.text(MessageKey::TuiEndpoint, &args),
            source.safe_endpoint(),
            translator.text(MessageKey::TuiRemotePath, &args),
            source.config.remote_root,
            source.config.profile,
            translator.text(MessageKey::TuiStatus, &args),
            source
                .status
                .clone()
                .unwrap_or_else(|| translator.text(MessageKey::TuiNotTested, &args))
        )
    });
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(pane_block(
                &translator.text(MessageKey::TuiDetails, &args),
                app.focus == FocusPane::Details,
            )),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let active = if app.progress.active {
        format!("  ◌ {}", translator.text(MessageKey::TuiWorking, &args))
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(format!(
            " {}{active}",
            translator.text(MessageKey::TuiFooter, &args)
        ))
        .style(Style::default().fg(MUTED)),
        area,
    );
}

fn render_resize(frame: &mut Frame<'_>, language: Language, area: Rect) {
    let translator = Translator::new(language);
    frame.render_widget(
        Paragraph::new(translator.text(MessageKey::TuiResize, &MessageArgs::default()))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("cc-switchy")),
        area,
    );
}

fn pane_block(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        })
}

fn selection_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}
