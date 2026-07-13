use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::config::{S3Config, SourceConfig, SourceKind, WebDavConfig};
use crate::{Language, MessageArgs, MessageKey, Translator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardMode {
    List,
    Details,
    ChooseType,
    EditWebDav,
    EditS3,
    ConfirmDelete,
    ChooseReplacementDefault,
    TestConnection,
    LanguageSelect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardAction {
    Move(i32),
    Details,
    Add,
    Edit,
    Delete,
    Test,
    MakeDefault,
    Language,
    ChooseWebDav,
    ChooseS3,
    Input(char),
    Backspace,
    NextField,
    PreviousField,
    Confirm,
    Cancel,
    Quit,
}

#[derive(Clone)]
pub enum WizardCommand {
    Add(SourceConfig),
    Update {
        original: String,
        source: SourceConfig,
    },
    Delete {
        name: String,
        replacement: Option<String>,
    },
    Test(String),
    MakeDefault(String),
    ChangeLanguage(Language),
    Exit,
}

#[derive(Clone)]
struct FormField {
    label: MessageKey,
    value: String,
    secret: bool,
}

pub struct WizardState {
    pub mode: WizardMode,
    pub language: Language,
    pub sources: Vec<SourceConfig>,
    pub default_source: Option<String>,
    pub selected: usize,
    pub status: Option<String>,
    type_cursor: usize,
    language_cursor: usize,
    replacement_cursor: usize,
    fields: Vec<FormField>,
    field: usize,
    edit_original: Option<String>,
    commands: VecDeque<WizardCommand>,
}

impl WizardState {
    pub fn new(
        language: Language,
        sources: Vec<SourceConfig>,
        default_source: Option<String>,
    ) -> Self {
        Self {
            mode: WizardMode::List,
            language,
            sources,
            default_source,
            selected: 0,
            status: None,
            type_cursor: 0,
            language_cursor: language_index(language),
            replacement_cursor: 0,
            fields: Vec::new(),
            field: 0,
            edit_original: None,
            commands: VecDeque::new(),
        }
    }

    pub fn selected_source(&self) -> Option<&SourceConfig> {
        self.sources.get(self.selected)
    }

    pub fn update_sources(&mut self, sources: Vec<SourceConfig>, default_source: Option<String>) {
        let selected_name = self.selected_source().map(|source| source.name.clone());
        self.sources = sources;
        self.default_source = default_source;
        self.selected = selected_name
            .as_deref()
            .and_then(|name| self.sources.iter().position(|source| source.name == name))
            .unwrap_or_else(|| self.selected.min(self.sources.len().saturating_sub(1)));
    }

    pub fn update(&mut self, action: WizardAction) {
        if action == WizardAction::Quit {
            self.commands.push_back(WizardCommand::Exit);
            return;
        }
        match self.mode {
            WizardMode::List => self.update_list(action),
            WizardMode::Details | WizardMode::TestConnection => match action {
                WizardAction::Cancel | WizardAction::Confirm => {
                    self.mode = WizardMode::List;
                }
                _ => {}
            },
            WizardMode::ChooseType => self.update_choose_type(action),
            WizardMode::EditWebDav | WizardMode::EditS3 => self.update_form(action),
            WizardMode::ConfirmDelete => self.update_delete(action),
            WizardMode::ChooseReplacementDefault => self.update_replacement(action),
            WizardMode::LanguageSelect => self.update_language(action),
        }
    }

    pub fn pop_command(&mut self) -> Option<WizardCommand> {
        self.commands.pop_front()
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub fn mutation_failed(&mut self, error: String) {
        self.status = Some(format!("× {error}"));
    }

    pub fn mutation_succeeded(
        &mut self,
        sources: Vec<SourceConfig>,
        default_source: Option<String>,
    ) {
        self.update_sources(sources, default_source);
        self.fields.clear();
        self.field = 0;
        self.edit_original = None;
        self.mode = WizardMode::List;
        self.status = Some(
            Translator::new(self.language).text(MessageKey::WizardSaved, &MessageArgs::default()),
        );
    }

    pub fn form_values(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|field| field.value.clone())
            .collect()
    }

    fn update_list(&mut self, action: WizardAction) {
        match action {
            WizardAction::Move(delta) => {
                self.selected = move_index(self.selected, self.sources.len(), delta);
            }
            WizardAction::Details | WizardAction::Confirm if !self.sources.is_empty() => {
                self.mode = WizardMode::Details;
            }
            WizardAction::Add => {
                self.type_cursor = 0;
                self.edit_original = None;
                self.mode = WizardMode::ChooseType;
            }
            WizardAction::Edit => self.begin_edit(),
            WizardAction::Delete if !self.sources.is_empty() => {
                self.mode = WizardMode::ConfirmDelete;
            }
            WizardAction::Test if !self.sources.is_empty() => {
                let name = self.sources[self.selected].name.clone();
                self.mode = WizardMode::TestConnection;
                self.status = Some(
                    Translator::new(self.language)
                        .text(MessageKey::WizardTesting, &MessageArgs::default()),
                );
                self.commands.push_back(WizardCommand::Test(name));
            }
            WizardAction::MakeDefault if !self.sources.is_empty() => {
                self.commands.push_back(WizardCommand::MakeDefault(
                    self.sources[self.selected].name.clone(),
                ));
            }
            WizardAction::Language => {
                self.language_cursor = language_index(self.language);
                self.mode = WizardMode::LanguageSelect;
            }
            _ => {}
        }
    }

    fn update_choose_type(&mut self, action: WizardAction) {
        match action {
            WizardAction::Move(delta) => {
                self.type_cursor = move_index(self.type_cursor, 2, delta);
            }
            WizardAction::ChooseWebDav => {
                self.type_cursor = 0;
                self.begin_new_form();
            }
            WizardAction::ChooseS3 => {
                self.type_cursor = 1;
                self.begin_new_form();
            }
            WizardAction::Confirm => self.begin_new_form(),
            WizardAction::Cancel => self.mode = WizardMode::List,
            _ => {}
        }
    }

    fn update_form(&mut self, action: WizardAction) {
        match action {
            WizardAction::Input(character) => self.fields[self.field].value.push(character),
            WizardAction::Backspace => {
                self.fields[self.field].value.pop();
            }
            WizardAction::NextField => {
                self.field = (self.field + 1).min(self.fields.len().saturating_sub(1));
            }
            WizardAction::PreviousField => self.field = self.field.saturating_sub(1),
            WizardAction::Confirm if self.field + 1 < self.fields.len() => self.field += 1,
            WizardAction::Confirm => {
                if let Some(source) = self.build_source() {
                    if let Some(original) = self.edit_original.clone() {
                        self.commands
                            .push_back(WizardCommand::Update { original, source });
                    } else {
                        self.commands.push_back(WizardCommand::Add(source));
                    }
                } else {
                    self.status = Some(
                        Translator::new(self.language)
                            .text(MessageKey::WizardRequired, &MessageArgs::default()),
                    );
                }
            }
            WizardAction::Cancel => {
                self.fields.clear();
                self.edit_original = None;
                self.mode = WizardMode::List;
            }
            _ => {}
        }
    }

    fn update_delete(&mut self, action: WizardAction) {
        match action {
            WizardAction::Confirm => {
                let Some(source) = self.selected_source() else {
                    self.mode = WizardMode::List;
                    return;
                };
                if self.default_source.as_deref() == Some(source.name.as_str())
                    && self.sources.len() > 1
                {
                    self.replacement_cursor = 0;
                    self.mode = WizardMode::ChooseReplacementDefault;
                } else {
                    self.commands.push_back(WizardCommand::Delete {
                        name: source.name.clone(),
                        replacement: None,
                    });
                    self.mode = WizardMode::List;
                }
            }
            WizardAction::Cancel => self.mode = WizardMode::List,
            _ => {}
        }
    }

    fn update_replacement(&mut self, action: WizardAction) {
        let replacements = self.replacement_names();
        match action {
            WizardAction::Move(delta) => {
                self.replacement_cursor =
                    move_index(self.replacement_cursor, replacements.len(), delta);
            }
            WizardAction::Confirm => {
                if let (Some(source), Some(replacement)) = (
                    self.selected_source(),
                    replacements.get(self.replacement_cursor),
                ) {
                    self.commands.push_back(WizardCommand::Delete {
                        name: source.name.clone(),
                        replacement: Some(replacement.clone()),
                    });
                    self.mode = WizardMode::List;
                }
            }
            WizardAction::Cancel => self.mode = WizardMode::List,
            _ => {}
        }
    }

    fn update_language(&mut self, action: WizardAction) {
        match action {
            WizardAction::Move(delta) => {
                self.language_cursor = move_index(self.language_cursor, 3, delta);
            }
            WizardAction::Confirm => {
                let language =
                    [Language::Auto, Language::ZhCn, Language::EnUs][self.language_cursor];
                self.commands
                    .push_back(WizardCommand::ChangeLanguage(language));
                self.mode = WizardMode::List;
            }
            WizardAction::Cancel => self.mode = WizardMode::List,
            _ => {}
        }
    }

    fn begin_new_form(&mut self) {
        self.field = 0;
        if self.type_cursor == 0 {
            self.mode = WizardMode::EditWebDav;
            self.fields = webdav_fields(None);
        } else {
            self.mode = WizardMode::EditS3;
            self.fields = s3_fields(None);
        }
    }

    fn begin_edit(&mut self) {
        let Some(source) = self.selected_source().cloned() else {
            return;
        };
        self.edit_original = Some(source.name.clone());
        self.field = 0;
        match source.kind {
            SourceKind::WebDav { .. } => {
                self.mode = WizardMode::EditWebDav;
                self.fields = webdav_fields(Some(source));
            }
            SourceKind::S3 { .. } => {
                self.mode = WizardMode::EditS3;
                self.fields = s3_fields(Some(source));
            }
        }
    }

    fn build_source(&self) -> Option<SourceConfig> {
        match self.mode {
            WizardMode::EditWebDav => Some(SourceConfig {
                name: self.fields[0].value.clone(),
                remote_root: self.fields[4].value.clone(),
                profile: self.fields[5].value.clone(),
                kind: SourceKind::WebDav {
                    webdav: WebDavConfig {
                        base_url: self.fields[1].value.clone(),
                        username: self.fields[2].value.clone(),
                        password: self.fields[3].value.clone(),
                    },
                },
            }),
            WizardMode::EditS3 => Some(SourceConfig {
                name: self.fields[0].value.clone(),
                remote_root: self.fields[6].value.clone(),
                profile: self.fields[7].value.clone(),
                kind: SourceKind::S3 {
                    s3: S3Config {
                        region: self.fields[1].value.clone(),
                        bucket: self.fields[2].value.clone(),
                        endpoint: self.fields[3].value.clone(),
                        access_key_id: self.fields[4].value.clone(),
                        secret_access_key: self.fields[5].value.clone(),
                    },
                },
            }),
            _ => None,
        }
    }

    fn replacement_names(&self) -> Vec<String> {
        let selected = self.selected_source().map(|source| source.name.as_str());
        self.sources
            .iter()
            .filter(|source| Some(source.name.as_str()) != selected)
            .map(|source| source.name.clone())
            .collect()
    }
}

pub fn action_for_key(state: &WizardState, key: KeyEvent) -> Option<WizardAction> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(WizardAction::Quit);
    }
    if matches!(state.mode, WizardMode::EditWebDav | WizardMode::EditS3) {
        return match key.code {
            KeyCode::Esc => Some(WizardAction::Cancel),
            KeyCode::Enter => Some(WizardAction::Confirm),
            KeyCode::Tab => Some(WizardAction::NextField),
            KeyCode::BackTab => Some(WizardAction::PreviousField),
            KeyCode::Backspace => Some(WizardAction::Backspace),
            KeyCode::Char(character) => Some(WizardAction::Input(character)),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char('q') => Some(WizardAction::Quit),
        KeyCode::Esc => Some(WizardAction::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(WizardAction::Move(-1)),
        KeyCode::Down | KeyCode::Char('j') => Some(WizardAction::Move(1)),
        KeyCode::Enter => Some(WizardAction::Confirm),
        KeyCode::Char('a') => Some(WizardAction::Add),
        KeyCode::Char('e') => Some(WizardAction::Edit),
        KeyCode::Char('x') => Some(WizardAction::Delete),
        KeyCode::Char('t') => Some(WizardAction::Test),
        KeyCode::Char('m') => Some(WizardAction::MakeDefault),
        KeyCode::Char('L') => Some(WizardAction::Language),
        KeyCode::Char('w') => Some(WizardAction::ChooseWebDav),
        KeyCode::Char('s') => Some(WizardAction::ChooseS3),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, state: &WizardState) {
    let area = frame.area();
    let translator = Translator::new(state.language);
    if area.width < 50 || area.height < 15 {
        frame.render_widget(
            Paragraph::new(translator.text(MessageKey::WizardResize, &MessageArgs::default()))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title("cc-switchy")),
            area,
        );
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(translator.text(MessageKey::WizardTitle, &MessageArgs::default()))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::BOTTOM)),
        rows[0],
    );
    match state.mode {
        WizardMode::List => render_list(frame, state, rows[1]),
        WizardMode::Details | WizardMode::TestConnection => {
            render_details(frame, state, rows[1]);
        }
        WizardMode::ChooseType => render_choices(
            frame,
            rows[1],
            &translator.text(MessageKey::WizardChooseType, &MessageArgs::default()),
            &["WebDAV", "S3"],
            state.type_cursor,
        ),
        WizardMode::EditWebDav | WizardMode::EditS3 => render_form(frame, state, rows[1]),
        WizardMode::ConfirmDelete => frame.render_widget(
            Paragraph::new(
                translator.text(MessageKey::WizardConfirmDelete, &MessageArgs::default()),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " {} ",
                translator.text(MessageKey::WizardConfirmHint, &MessageArgs::default())
            ))),
            centered(rows[1], 60, 7),
        ),
        WizardMode::ChooseReplacementDefault => {
            let replacements = state.replacement_names();
            let refs = replacements.iter().map(String::as_str).collect::<Vec<_>>();
            render_choices(
                frame,
                rows[1],
                &translator.text(MessageKey::WizardReplacement, &MessageArgs::default()),
                &refs,
                state.replacement_cursor,
            );
        }
        WizardMode::LanguageSelect => {
            let choices = [
                translator.text(MessageKey::WizardAutoLanguage, &MessageArgs::default()),
                "简体中文".to_string(),
                "English".to_string(),
            ];
            let refs = choices.iter().map(String::as_str).collect::<Vec<_>>();
            render_choices(
                frame,
                rows[1],
                &translator.text(MessageKey::WizardLanguage, &MessageArgs::default()),
                &refs,
                state.language_cursor,
            );
        }
    }
    let status = state.status.as_deref().unwrap_or("");
    let footer_key = match state.mode {
        WizardMode::List => MessageKey::WizardFooterList,
        WizardMode::EditWebDav | WizardMode::EditS3 => MessageKey::WizardFooterForm,
        WizardMode::Details | WizardMode::TestConnection => MessageKey::WizardFooterBack,
        WizardMode::ConfirmDelete => MessageKey::WizardFooterConfirm,
        WizardMode::ChooseType
        | WizardMode::ChooseReplacementDefault
        | WizardMode::LanguageSelect => MessageKey::WizardFooterNavigate,
    };
    let hint = translator.text(footer_key, &MessageArgs::default());
    let footer = if status.is_empty() {
        vec![Line::from(format!(" {hint}"))]
    } else {
        vec![
            Line::from(format!(" {status}")),
            Line::from(format!(" {hint}")),
        ]
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        rows[2],
    );
}

fn render_list(frame: &mut Frame<'_>, state: &WizardState, area: Rect) {
    let translator = Translator::new(state.language);
    if state.sources.is_empty() {
        frame.render_widget(
            Paragraph::new(translator.text(MessageKey::WizardNoSources, &MessageArgs::default()))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " {} ",
                    translator.text(MessageKey::TuiSources, &MessageArgs::default())
                ))),
            area,
        );
        return;
    }
    let items = state
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let kind = match source.kind {
                SourceKind::WebDav { .. } => "WebDAV",
                SourceKind::S3 { .. } => "S3",
            };
            ListItem::new(format!(
                "{} {}  {:7} {}",
                if index == state.selected { "›" } else { " " },
                source.name,
                kind,
                if state.default_source.as_deref() == Some(source.name.as_str()) {
                    translator.text(MessageKey::TuiDefault, &MessageArgs::default())
                } else {
                    String::new()
                }
            ))
            .style(if index == state.selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            " {} ",
            translator.text(MessageKey::TuiSources, &MessageArgs::default())
        ))),
        area,
    );
}

fn render_details(frame: &mut Frame<'_>, state: &WizardState, area: Rect) {
    let translator = Translator::new(state.language);
    let args = MessageArgs::default();
    let text = state.selected_source().map_or_else(String::new, |source| {
        let detail = match &source.kind {
            SourceKind::WebDav { webdav } => format!(
                "{}      WebDAV\nURL       {}\n{}  {}\n{}  {}",
                translator.text(MessageKey::TuiType, &args),
                webdav.base_url,
                translator.text(MessageKey::FieldUsername, &args),
                webdav.username,
                translator.text(MessageKey::FieldPassword, &args),
                mask(&webdav.password)
            ),
            SourceKind::S3 { s3 } => format!(
                "{}      S3\n{}    {}\n{}    {}\n{}  {}\n{} {}\n{}    {}",
                translator.text(MessageKey::TuiType, &args),
                translator.text(MessageKey::FieldRegion, &args),
                s3.region,
                translator.text(MessageKey::FieldBucket, &args),
                s3.bucket,
                translator.text(MessageKey::FieldEndpoint, &args),
                s3.endpoint,
                translator.text(MessageKey::FieldAccessKeyId, &args),
                mask_access_id(&s3.access_key_id),
                translator.text(MessageKey::FieldSecretKey, &args),
                mask(&s3.secret_access_key)
            ),
        };
        format!(
            "{}\n\n{}\n\n{}    {}/v2/db-v6/{}\n\n{}",
            source.name,
            detail,
            translator.text(MessageKey::TuiRemotePath, &args),
            source.remote_root,
            source.profile,
            state.status.as_deref().unwrap_or("")
        )
    });
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default().borders(Borders::ALL).title(format!(
                " {} ",
                translator.text(MessageKey::WizardDetails, &args)
            )),
        ),
        area,
    );
}

fn render_form(frame: &mut Frame<'_>, state: &WizardState, area: Rect) {
    let translator = Translator::new(state.language);
    let args = MessageArgs::default();
    let active_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let items = state
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let active = index == state.field;
            let value = displayed_form_value(field);
            ListItem::new(Line::from(vec![
                Span::styled(
                    if active { "› " } else { "  " },
                    if active {
                        active_style
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("{:14}", translator.text(field.label, &args)),
                    if active {
                        active_style
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(
                    value,
                    if active {
                        active_style
                    } else {
                        Style::default()
                    },
                ),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            " {} ",
            translator.text(MessageKey::WizardEditHint, &args)
        ))),
        area,
    );
    let active_field = &state.fields[state.field];
    let cursor_prefix = format!(
        "› {:14}{}",
        translator.text(active_field.label, &args),
        displayed_form_value(active_field)
    );
    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(Line::from(cursor_prefix).width() as u16)
        .min(area.right().saturating_sub(2));
    let cursor_y = area
        .y
        .saturating_add(1)
        .saturating_add(state.field as u16)
        .min(area.bottom().saturating_sub(2));
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn displayed_form_value(field: &FormField) -> String {
    if field.label == MessageKey::FieldAccessKeyId {
        mask_access_id(&field.value)
    } else if field.secret {
        mask(&field.value)
    } else {
        field.value.clone()
    }
}

fn render_choices(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    choices: &[&str],
    selected: usize,
) {
    let items = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            ListItem::new(format!(
                "{} {choice}",
                if index == selected { "›" } else { " " }
            ))
            .style(if index == selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {title} ")),
        ),
        centered(area, 50, (choices.len() as u16 + 4).max(7)),
    );
}

fn webdav_fields(source: Option<SourceConfig>) -> Vec<FormField> {
    let (name, remote_root, profile, config) = match source {
        Some(SourceConfig {
            name,
            remote_root,
            profile,
            kind: SourceKind::WebDav { webdav },
        }) => (name, remote_root, profile, webdav),
        _ => (
            String::new(),
            "cc-switch-sync".to_string(),
            "default".to_string(),
            WebDavConfig {
                base_url: String::new(),
                username: String::new(),
                password: String::new(),
            },
        ),
    };
    vec![
        field(MessageKey::FieldName, name, false),
        field(MessageKey::FieldBaseUrl, config.base_url, false),
        field(MessageKey::FieldUsername, config.username, false),
        field(MessageKey::FieldPassword, config.password, true),
        field(MessageKey::FieldRemoteRoot, remote_root, false),
        field(MessageKey::FieldProfile, profile, false),
    ]
}

fn s3_fields(source: Option<SourceConfig>) -> Vec<FormField> {
    let (name, remote_root, profile, config) = match source {
        Some(SourceConfig {
            name,
            remote_root,
            profile,
            kind: SourceKind::S3 { s3 },
        }) => (name, remote_root, profile, s3),
        _ => (
            String::new(),
            "cc-switch-sync".to_string(),
            "default".to_string(),
            S3Config {
                region: "us-east-1".to_string(),
                bucket: String::new(),
                endpoint: String::new(),
                access_key_id: String::new(),
                secret_access_key: String::new(),
            },
        ),
    };
    vec![
        field(MessageKey::FieldName, name, false),
        field(MessageKey::FieldRegion, config.region, false),
        field(MessageKey::FieldBucket, config.bucket, false),
        field(MessageKey::FieldEndpoint, config.endpoint, false),
        field(MessageKey::FieldAccessKeyId, config.access_key_id, true),
        field(MessageKey::FieldSecretKey, config.secret_access_key, true),
        field(MessageKey::FieldRemoteRoot, remote_root, false),
        field(MessageKey::FieldProfile, profile, false),
    ]
}

fn field(label: MessageKey, value: String, secret: bool) -> FormField {
    FormField {
        label,
        value,
        secret,
    }
}

fn mask(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        "•".repeat(value.chars().count().max(8))
    }
}

fn mask_access_id(value: &str) -> String {
    if value.chars().count() <= 4 {
        return mask(value);
    }
    format!(
        "{}{}",
        value.chars().take(4).collect::<String>(),
        "•".repeat(8)
    )
}

fn language_index(language: Language) -> usize {
    match language {
        Language::Auto => 0,
        Language::ZhCn => 1,
        Language::EnUs => 2,
    }
}

fn move_index(current: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (current as i64 + i64::from(delta)).clamp(0, len.saturating_sub(1) as i64) as usize
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height.min(area.height)),
            Constraint::Min(0),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width.min(area.width)),
            Constraint::Min(0),
        ])
        .split(vertical[1])[1]
}
