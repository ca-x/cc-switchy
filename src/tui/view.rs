use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::event::ActivityStatus;
use super::{App, FocusPane, MainView};
use crate::{Language, MessageArgs, MessageKey, Translator};

const ACCENT: Color = Color::Cyan;
const SECONDARY: Color = Color::LightBlue;
const MUTED: Color = Color::DarkGray;
const SELECTED_BG: Color = Color::Rgb(27, 34, 41);

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < 60 || area.height < 18 {
        render_resize(frame, app.language, area);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);
    render_header(frame, app, rows[0]);
    match app.view {
        MainView::Providers => render_providers(frame, app, rows[1]),
        MainView::Sources => render_sources(frame, app, rows[1]),
        MainView::Skills => render_skills(frame, app, rows[1]),
        MainView::Activity => render_activity(frame, app, rows[1]),
    }
    render_footer(frame, app, rows[2]);
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let agent = app.selected_agent();
    let agent_name = truncate_to_width(&agent.to_string(), 18);
    let provider = if agent.is_additive() {
        translator.text(MessageKey::TuiAdditiveSet, &args)
    } else {
        app.current_provider()
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| "—".to_string())
    };
    let source = app.default_source_name().unwrap_or("—");
    let label_width = display_width(&translator.text(MessageKey::TuiAgent, &args))
        + display_width(&translator.text(MessageKey::TuiProvider, &args));
    let provider_width = usize::from(area.width)
        .saturating_sub(20 + label_width + display_width(&agent_name))
        .max(4);
    let provider = truncate_to_width(&provider, provider_width);
    let source_width = usize::from(area.width).saturating_sub(24).max(4);
    let source = truncate_to_width(source, source_width);
    let state = if app.progress.active {
        (
            "◌",
            translator.text(MessageKey::TuiWorkingShort, &args),
            Color::Yellow,
        )
    } else {
        (
            "✓",
            translator.text(MessageKey::TuiReady, &args),
            Color::Green,
        )
    };
    let primary_status = Line::from(vec![
        Span::styled(
            " cc-switchy ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}  ", translator.text(MessageKey::TuiAgent, &args)),
            Style::default().fg(MUTED),
        ),
        Span::styled(agent_name, Style::default().fg(ACCENT).bold()),
        Span::styled(
            format!("  {}  ", translator.text(MessageKey::TuiProvider, &args)),
            Style::default().fg(MUTED),
        ),
        Span::styled(provider, Style::default().fg(Color::White)),
    ]);
    let secondary_status = Line::from(vec![
        Span::styled(
            format!(" {}  ", translator.text(MessageKey::TuiSource, &args)),
            Style::default().fg(MUTED),
        ),
        Span::styled(source, Style::default().fg(SECONDARY)),
        Span::raw("  "),
        Span::styled(
            format!("{} {}", state.0, state.1),
            Style::default().fg(state.2),
        ),
    ]);
    let navigation = [
        (MainView::Providers, "1", MessageKey::TuiSwitch),
        (MainView::Sources, "2", MessageKey::TuiSync),
        (MainView::Skills, "3", MessageKey::TuiSkills),
        (MainView::Activity, "4", MessageKey::TuiActivity),
    ]
    .into_iter()
    .flat_map(|(view, key, label)| {
        let selected = app.view == view;
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        [
            Span::raw("  "),
            Span::styled(format!("{key} {}", translator.text(label, &args)), style),
        ]
    })
    .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(vec![
            primary_status,
            secondary_status,
            Line::from(navigation),
        ])
        .block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_providers(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(7)])
        .split(area);
    render_agent_strip(frame, app, rows[0]);
    if rows[1].width >= 120 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(rows[1]);
        render_provider_list(frame, app, columns[0]);
        render_provider_details(frame, app, columns[1]);
    } else if app.focus == FocusPane::Details {
        render_provider_details(frame, app, rows[1]);
    } else {
        render_provider_list(frame, app, rows[1]);
    }
}

fn render_agent_strip(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let (start, end) = visible_agent_range(app, area.width);
    let spans = app
        .agents
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .flat_map(|(index, agent)| {
            let selected = index == app.selected_agent;
            let available = app
                .providers
                .get(agent)
                .is_some_and(|providers| !providers.is_empty());
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if available {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(MUTED)
            };
            [
                Span::raw(" "),
                Span::styled(
                    format!("{} {agent} ", if available { "●" } else { "○" }),
                    style,
                ),
            ]
        })
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    if start > 0 {
        visible.push(Span::styled(" ‹", Style::default().fg(MUTED)));
    }
    visible.extend(spans);
    if end < app.agents.len() {
        visible.push(Span::styled(" ›", Style::default().fg(MUTED)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(visible)).block(pane_block(
            &translator.text(MessageKey::TuiAgents, &MessageArgs::default()),
            app.focus == FocusPane::Agents,
        )),
        area,
    );
}

fn render_provider_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
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
            Paragraph::new(translator.text(MessageKey::TuiNoProviders, &args))
                .alignment(Alignment::Center)
                .block(pane_block(
                    &translator.text(MessageKey::TuiProviders, &args),
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
            let state = if agent.is_additive() {
                translator.text(MessageKey::TuiAdditiveSet, &args)
            } else if provider.is_current {
                translator.text(MessageKey::TuiCurrent, &args)
            } else {
                translator.text(MessageKey::TuiAvailable, &args)
            };
            let unmanaged = if provider.managed {
                String::new()
            } else {
                format!(" · {}", translator.text(MessageKey::TuiUnmanaged, &args))
            };
            ListItem::new(format!(
                "{} {}  {}  · {state}{unmanaged}",
                if selected { "›" } else { " " },
                if agent.is_additive() || provider.is_current {
                    "●"
                } else {
                    "○"
                },
                provider.name
            ))
            .style(selection_style(selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &format!(
                "{} · {agent}",
                translator.text(MessageKey::TuiProviders, &args)
            ),
            app.focus == FocusPane::List,
        )),
        area,
    );
}

fn render_provider_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let lines = app.selected_provider().map_or_else(
        || {
            vec![Line::raw(
                translator.text(MessageKey::TuiNoProviders, &args),
            )]
        },
        |provider| {
            let status = if app.selected_agent().is_additive() {
                format!("● {}", translator.text(MessageKey::TuiAdditiveSet, &args))
            } else if provider.is_current {
                format!("● {}", translator.text(MessageKey::TuiCurrent, &args))
            } else {
                format!("○ {}", translator.text(MessageKey::TuiAvailable, &args))
            };
            vec![
                Line::styled(
                    provider.name.clone(),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::raw(""),
                detail_line(translator.text(MessageKey::TuiStatus, &args), status),
                detail_line("ID".to_string(), provider.id.clone()),
                detail_line(
                    translator.text(MessageKey::TuiCategory, &args),
                    provider.category.clone().unwrap_or_else(|| "—".to_string()),
                ),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(pane_block(
                &translator.text(MessageKey::TuiDetails, &args),
                app.focus == FocusPane::Details,
            )),
        area,
    );
}

fn render_skills(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(7)])
        .split(area);
    render_agent_strip(frame, app, rows[0]);
    render_skill_list(frame, app, rows[1]);
}

fn render_skill_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let skills = app
        .skills
        .get(&app.selected_agent())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let items = skills
        .iter()
        .map(|skill| {
            ListItem::new(format!(
                "{}  {}  · {}",
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
        area,
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
        let filled = usize::try_from(percent.min(100) / 10).unwrap_or(10);
        progress.push(Line::from(format!(
            "{artifact}  [{}{}]  {downloaded}/{total} {}  {percent}%",
            "■".repeat(filled),
            "·".repeat(10usize.saturating_sub(filled)),
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
                    &translator.text(MessageKey::TuiSync, &MessageArgs::default()),
                    true,
                )),
            area,
        );
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(7)])
        .split(area);
    render_sync_summary(frame, app, rows[0]);
    if rows[1].width >= 120 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(rows[1]);
        render_source_list(frame, app, columns[0], false);
        render_source_details(frame, app, columns[1]);
    } else if app.focus == FocusPane::Details {
        render_source_details(frame, app, rows[1]);
    } else {
        render_source_list(frame, app, rows[1], true);
    }
}

fn render_sync_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let source = app.selected_source();
    let status = source
        .and_then(|source| source.status.clone())
        .unwrap_or_else(|| translator.text(MessageKey::TuiNotTested, &args));
    let line = Line::from(vec![
        Span::styled(" s ", Style::default().fg(Color::Black).bg(ACCENT).bold()),
        Span::styled(
            format!(" {}  ", translator.text(MessageKey::TuiSyncNow, &args)),
            Style::default().fg(ACCENT).bold(),
        ),
        Span::styled(
            format!("{}  ", translator.text(MessageKey::TuiSource, &args)),
            Style::default().fg(MUTED),
        ),
        Span::styled(
            source
                .map(|source| source.config.name.as_str())
                .unwrap_or("—"),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("  {}  ", translator.text(MessageKey::TuiStatus, &args)),
            Style::default().fg(MUTED),
        ),
        Span::styled(status, Style::default().fg(SECONDARY)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(pane_block(
            &translator.text(MessageKey::TuiSync, &args),
            false,
        )),
        area,
    );
}

fn render_source_list(frame: &mut Frame<'_>, app: &App, area: Rect, show_status: bool) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let items = app
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let selected = index == app.selected_source;
            let header = format!(
                "{} {}  {:7} {}",
                if selected { "›" } else { " " },
                source.config.name,
                source.kind_label(),
                if source.is_default {
                    translator.text(MessageKey::TuiDefault, &args)
                } else {
                    String::new()
                }
            );
            let item = if show_status {
                let status = source
                    .status
                    .clone()
                    .unwrap_or_else(|| translator.text(MessageKey::TuiNotTested, &args));
                ListItem::new(vec![Line::raw(header), Line::raw(format!("  {status}"))])
            } else {
                ListItem::new(header)
            };
            item.style(selection_style(selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(pane_block(
            &translator.text(MessageKey::TuiSources, &args),
            app.focus == FocusPane::List,
        )),
        area,
    );
}

fn render_source_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let translator = Translator::new(app.language);
    let args = MessageArgs::default();
    let lines = app.selected_source().map_or_else(Vec::new, |source| {
        vec![
            Line::styled(
                source.config.name.clone(),
                Style::default().fg(Color::White).bold(),
            ),
            Line::raw(""),
            detail_line(
                translator.text(MessageKey::TuiType, &args),
                source.kind_label().to_string(),
            ),
            detail_line(
                translator.text(MessageKey::TuiEndpoint, &args),
                source.safe_endpoint(),
            ),
            detail_line(
                translator.text(MessageKey::TuiRemotePath, &args),
                format!(
                    "{}/v2/db-v6/{}",
                    source.config.remote_root, source.config.profile
                ),
            ),
            Line::raw(""),
            detail_line(
                translator.text(MessageKey::TuiStatus, &args),
                source
                    .status
                    .clone()
                    .unwrap_or_else(|| translator.text(MessageKey::TuiNotTested, &args)),
            ),
        ]
    });
    frame.render_widget(
        Paragraph::new(lines)
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
    let compact = area.width < 100;
    let footer_key = match (app.view, compact) {
        (MainView::Providers, false) => MessageKey::TuiFooterProviders,
        (MainView::Providers, true) => MessageKey::TuiFooterProvidersCompact,
        (MainView::Sources, false) => MessageKey::TuiFooterSources,
        (MainView::Sources, true) => MessageKey::TuiFooterSourcesCompact,
        (MainView::Skills, false) => MessageKey::TuiFooterSkills,
        (MainView::Skills, true) => MessageKey::TuiFooterSkillsCompact,
        (MainView::Activity, false) => MessageKey::TuiFooterActivity,
        (MainView::Activity, true) => MessageKey::TuiFooterActivityCompact,
    };
    let retry = if app.view == MainView::Activity && app.progress.retry_available {
        format!("  r {}", translator.text(MessageKey::TuiRetry, &args))
    } else {
        String::new()
    };
    let active = if app.progress.active {
        format!("  ◌ {}", translator.text(MessageKey::TuiWorking, &args))
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(format!(
            " {}{retry}{active}",
            translator.text(footer_key, &args)
        ))
        .style(Style::default().fg(MUTED)),
        area,
    );
}

fn detail_line(label: String, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(MUTED)),
        Span::raw(value),
    ])
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
    let marker = if focused { "› " } else { "" };
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {marker}{title} "))
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        })
}

fn selection_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(ACCENT)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn visible_agent_range(app: &App, area_width: u16) -> (usize, usize) {
    let len = app.agents.len();
    if len == 0 {
        return (0, 0);
    }
    let selected = app.selected_agent.min(len - 1);
    let capacity = usize::from(area_width.saturating_sub(4));
    let widths = app
        .agents
        .iter()
        .map(|agent| display_width(&agent.to_string()) + 4)
        .collect::<Vec<_>>();
    let mut start = selected;
    let mut end = selected + 1;
    let mut used = widths[selected];

    while start > 0 {
        let candidate = start - 1;
        let hidden_left = usize::from(candidate > 0) * 2;
        let hidden_right = usize::from(end < len) * 2;
        if used + widths[candidate] + hidden_left + hidden_right > capacity {
            break;
        }
        start = candidate;
        used += widths[candidate];
    }
    while end < len {
        let hidden_left = usize::from(start > 0) * 2;
        let hidden_right = usize::from(end + 1 < len) * 2;
        if used + widths[end] + hidden_left + hidden_right > capacity {
            break;
        }
        used += widths[end];
        end += 1;
    }
    (start, end)
}

fn truncate_to_width(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let ellipsis_width = display_width("…");
    let target = max_width.saturating_sub(ellipsis_width);
    let mut result = String::new();
    let mut width = 0;
    for grapheme in value.graphemes(true) {
        let grapheme_width = display_width(grapheme);
        if width + grapheme_width > target {
            break;
        }
        result.push_str(grapheme);
        width += grapheme_width;
    }
    result.push('…');
    result
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}
