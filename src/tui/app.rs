use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::event::{ActivityStatus, ProgressModel};
use crate::agent::Agent;
use crate::config::{SourceConfig, SourceKind};
use crate::progress::ProgressEvent;
use crate::Language;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MainView {
    #[default]
    Providers,
    Skills,
    Activity,
    Sources,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusPane {
    #[default]
    Agents,
    List,
    Details,
    Activity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorState {
    pub selected: usize,
    pub scroll: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PersistedUiState {
    pub view: MainView,
    pub focus: FocusPane,
    pub agent: Option<Agent>,
    pub selected_source: Option<String>,
    pub provider_cursors: HashMap<Agent, CursorState>,
}

#[derive(Debug, Clone)]
pub struct ViewProvider {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub is_current: bool,
    pub managed: bool,
}

#[derive(Debug, Clone)]
pub struct ViewSkill {
    pub directory: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct ViewSource {
    pub config: SourceConfig,
    pub is_default: bool,
    pub status: Option<String>,
}

impl ViewSource {
    pub fn kind_label(&self) -> &'static str {
        match self.config.kind {
            SourceKind::WebDav { .. } => "WebDAV",
            SourceKind::S3 { .. } => "S3",
        }
    }

    pub fn safe_endpoint(&self) -> String {
        match &self.config.kind {
            SourceKind::WebDav { webdav } => safe_url(&webdav.base_url),
            SourceKind::S3 { s3 } if s3.endpoint.is_empty() => {
                format!("AWS · {} · {}", s3.region, s3.bucket)
            }
            SourceKind::S3 { s3 } => format!("{} · {}", safe_url(&s3.endpoint), s3.bucket),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    Quit,
    SwitchView(MainView),
    Move(i32),
    FocusNext,
    FocusPrevious,
    PreviousAgent,
    NextAgent,
    SwitchProvider { agent: Agent, provider_id: String },
    ReapplyProviders { agent: Agent },
    SyncSource { source: String },
    TestSource { source: String },
    MakeDefault { source: String },
    OpenWizard,
    ChangeLanguage(Language),
    RetryWarnings,
    CancelActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiCommand {
    Quit,
    SwitchProvider { agent: Agent, provider_id: String },
    ReapplyProviders { agent: Agent },
    SyncSource { source: String },
    TestSource { source: String },
    MakeDefault { source: String },
    OpenWizard,
    ChangeLanguage(Language),
    RetryWarnings,
    CancelActive,
    PersistUi,
}

pub struct App {
    pub language: Language,
    pub view: MainView,
    pub focus: FocusPane,
    pub agents: Vec<Agent>,
    pub selected_agent: usize,
    pub provider_cursors: HashMap<Agent, CursorState>,
    pub providers: HashMap<Agent, Vec<ViewProvider>>,
    pub skills: HashMap<Agent, Vec<ViewSkill>>,
    pub sources: Vec<ViewSource>,
    pub selected_source: usize,
    pub progress: ProgressModel,
    commands: VecDeque<TuiCommand>,
}

impl App {
    pub fn new(
        language: Language,
        providers: HashMap<Agent, Vec<ViewProvider>>,
        skills: HashMap<Agent, Vec<ViewSkill>>,
        sources: Vec<ViewSource>,
        persisted: PersistedUiState,
    ) -> Self {
        let agents = vec![
            Agent::Claude,
            Agent::Codex,
            Agent::Gemini,
            Agent::OpenCode,
            Agent::OpenClaw,
            Agent::Hermes,
            Agent::ClaudeDesktop,
        ];
        let selected_agent = persisted
            .agent
            .and_then(|agent| agents.iter().position(|candidate| *candidate == agent))
            .unwrap_or_default();
        let selected_source = persisted
            .selected_source
            .as_deref()
            .and_then(|name| sources.iter().position(|source| source.config.name == name))
            .unwrap_or_default();
        let view = if sources.is_empty() {
            MainView::Sources
        } else {
            persisted.view
        };
        let focus = normalized_focus(view, persisted.focus);
        Self {
            language,
            view,
            focus,
            agents,
            selected_agent,
            provider_cursors: persisted.provider_cursors,
            providers,
            skills,
            sources,
            selected_source,
            progress: ProgressModel::default(),
            commands: VecDeque::new(),
        }
    }

    pub fn selected_agent(&self) -> Agent {
        self.agents[self.selected_agent]
    }

    pub fn selected_provider(&self) -> Option<&ViewProvider> {
        let agent = self.selected_agent();
        let cursor = self
            .provider_cursors
            .get(&agent)
            .copied()
            .unwrap_or_default();
        self.providers.get(&agent)?.get(cursor.selected)
    }

    pub fn selected_source(&self) -> Option<&ViewSource> {
        self.sources.get(self.selected_source)
    }

    pub fn default_source_name(&self) -> Option<&str> {
        self.sources
            .iter()
            .find(|source| source.is_default)
            .map(|source| source.config.name.as_str())
    }

    pub fn update(&mut self, action: TuiAction) {
        match action {
            TuiAction::Quit => self.commands.push_back(TuiCommand::Quit),
            TuiAction::SwitchView(view) => {
                self.view = view;
                self.focus = match view {
                    MainView::Providers | MainView::Skills => FocusPane::Agents,
                    MainView::Activity => FocusPane::Activity,
                    MainView::Sources => FocusPane::List,
                };
                self.persist();
            }
            TuiAction::Move(delta) => self.move_selection(delta),
            TuiAction::FocusNext => {
                self.focus = next_focus(self.view, self.focus, true);
                self.persist();
            }
            TuiAction::FocusPrevious => {
                self.focus = next_focus(self.view, self.focus, false);
                self.persist();
            }
            TuiAction::PreviousAgent => self.change_agent(-1),
            TuiAction::NextAgent => self.change_agent(1),
            TuiAction::SwitchProvider { agent, provider_id } => self
                .commands
                .push_back(TuiCommand::SwitchProvider { agent, provider_id }),
            TuiAction::ReapplyProviders { agent } => self
                .commands
                .push_back(TuiCommand::ReapplyProviders { agent }),
            TuiAction::SyncSource { source } => {
                self.commands.push_back(TuiCommand::SyncSource { source });
            }
            TuiAction::TestSource { source } => {
                self.commands.push_back(TuiCommand::TestSource { source });
            }
            TuiAction::MakeDefault { source } => {
                self.commands.push_back(TuiCommand::MakeDefault { source });
            }
            TuiAction::OpenWizard => self.commands.push_back(TuiCommand::OpenWizard),
            TuiAction::ChangeLanguage(language) => {
                self.language = language;
                self.commands
                    .push_back(TuiCommand::ChangeLanguage(language));
            }
            TuiAction::RetryWarnings => self.commands.push_back(TuiCommand::RetryWarnings),
            TuiAction::CancelActive => self.commands.push_back(TuiCommand::CancelActive),
        }
    }

    pub fn pop_command(&mut self) -> Option<TuiCommand> {
        self.commands.pop_front()
    }

    pub fn apply_progress(&mut self, event: ProgressEvent, elapsed: Duration) {
        self.progress.apply(event, elapsed, self.language);
    }

    pub fn push_activity(&mut self, status: ActivityStatus, text: impl Into<String>) {
        self.progress.push(status, text.into());
    }

    pub(crate) fn set_source_status(&mut self, source: &str, status: impl Into<String>) {
        if let Some(item) = self
            .sources
            .iter_mut()
            .find(|item| item.config.name == source)
        {
            item.status = Some(status.into());
        }
    }

    pub(crate) fn preserve_source_statuses_from(&mut self, old: &App) {
        let statuses = old
            .sources
            .iter()
            .filter_map(|source| {
                source
                    .status
                    .clone()
                    .map(|status| (source.config.name.clone(), status))
            })
            .collect::<HashMap<_, _>>();
        for source in &mut self.sources {
            source.status = statuses.get(&source.config.name).cloned();
        }
    }

    pub fn persisted(&self) -> PersistedUiState {
        PersistedUiState {
            view: self.view,
            focus: self.focus,
            agent: Some(self.selected_agent()),
            selected_source: self
                .selected_source()
                .map(|source| source.config.name.clone()),
            provider_cursors: self.provider_cursors.clone(),
        }
    }

    fn change_agent(&mut self, delta: i32) {
        self.selected_agent = move_index(self.selected_agent, self.agents.len(), delta);
        self.persist();
    }

    fn move_selection(&mut self, delta: i32) {
        match (self.view, self.focus) {
            (MainView::Providers | MainView::Skills, FocusPane::Agents) => {
                self.change_agent(delta);
            }
            (MainView::Providers, FocusPane::List) => {
                let agent = self.selected_agent();
                let len = self.providers.get(&agent).map(Vec::len).unwrap_or_default();
                let cursor = self.provider_cursors.entry(agent).or_default();
                cursor.selected = move_index(cursor.selected, len, delta);
                cursor.scroll = cursor.selected.saturating_sub(8);
                self.persist();
            }
            (MainView::Sources, FocusPane::List) => {
                self.selected_source = move_index(self.selected_source, self.sources.len(), delta);
                self.persist();
            }
            _ => {}
        }
    }

    fn persist(&mut self) {
        if !self
            .commands
            .iter()
            .any(|command| *command == TuiCommand::PersistUi)
        {
            self.commands.push_back(TuiCommand::PersistUi);
        }
    }
}

impl PersistedUiState {
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Self::default();
        };
        root.get("ui")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), crate::AppError> {
        let parent = path
            .parent()
            .ok_or_else(|| crate::AppError::Restore("state path has no parent".to_string()))?;
        fs::create_dir_all(parent).map_err(|error| crate::AppError::io(parent, error))?;
        let mut root = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        root.insert(
            "ui".to_string(),
            serde_json::to_value(self)
                .map_err(|error| crate::AppError::Restore(error.to_string()))?,
        );
        let mut bytes = serde_json::to_vec_pretty(&root)
            .map_err(|error| crate::AppError::Restore(error.to_string()))?;
        bytes.push(b'\n');
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| crate::AppError::io(parent, error))?;
        temporary
            .write_all(&bytes)
            .map_err(|error| crate::AppError::io(temporary.path(), error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| crate::AppError::io(temporary.path(), error))?;
        temporary
            .persist(path)
            .map_err(|error| crate::AppError::io(path, error.error))?;
        set_private_file(path)
    }
}

fn next_focus(view: MainView, focus: FocusPane, forward: bool) -> FocusPane {
    let panes: &[FocusPane] = match view {
        MainView::Providers => &[FocusPane::Agents, FocusPane::List, FocusPane::Details],
        MainView::Skills => &[FocusPane::Agents, FocusPane::List],
        MainView::Activity => &[FocusPane::Activity],
        MainView::Sources => &[FocusPane::List, FocusPane::Details],
    };
    let current = panes
        .iter()
        .position(|pane| *pane == focus)
        .unwrap_or_default();
    let next = if forward {
        (current + 1) % panes.len()
    } else if current == 0 {
        panes.len() - 1
    } else {
        current - 1
    };
    panes[next]
}

fn normalized_focus(view: MainView, focus: FocusPane) -> FocusPane {
    match view {
        MainView::Providers
            if matches!(
                focus,
                FocusPane::Agents | FocusPane::List | FocusPane::Details
            ) =>
        {
            focus
        }
        MainView::Providers => FocusPane::Agents,
        MainView::Skills if matches!(focus, FocusPane::Agents | FocusPane::List) => focus,
        MainView::Skills => FocusPane::Agents,
        MainView::Activity => FocusPane::Activity,
        MainView::Sources if matches!(focus, FocusPane::List | FocusPane::Details) => focus,
        MainView::Sources => FocusPane::List,
    }
}

fn move_index(current: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as i64 + i64::from(delta);
    next.clamp(0, len.saturating_sub(1) as i64) as usize
}

fn safe_url(raw: &str) -> String {
    let candidate = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let Ok(mut url) = url::Url::parse(&candidate) else {
        return "<invalid endpoint>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), crate::AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| crate::AppError::io(path, error))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), crate::AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SourceKind, WebDavConfig};

    fn app_with_sources<const N: usize>(sources: [(&str, Option<&str>); N]) -> App {
        let sources = sources
            .into_iter()
            .map(|(name, status)| ViewSource {
                config: SourceConfig {
                    name: name.to_string(),
                    remote_root: "cc-switch-sync".to_string(),
                    profile: "default".to_string(),
                    kind: SourceKind::WebDav {
                        webdav: WebDavConfig {
                            base_url: "https://dav.example.test".to_string(),
                            username: String::new(),
                            password: String::new(),
                        },
                    },
                },
                is_default: name == "home",
                status: status.map(str::to_string),
            })
            .collect();
        App::new(
            Language::EnUs,
            HashMap::new(),
            HashMap::new(),
            sources,
            PersistedUiState::default(),
        )
    }

    #[test]
    fn source_statuses_survive_reload_only_for_exact_names() {
        let old = app_with_sources([("home", Some("✓ Snapshot abc")), ("old", Some("× failed"))]);
        let mut new = app_with_sources([("home", None), ("renamed", None)]);

        new.preserve_source_statuses_from(&old);

        assert_eq!(new.sources[0].status.as_deref(), Some("✓ Snapshot abc"));
        assert_eq!(new.sources[1].status, None);
    }

    #[test]
    fn source_status_updates_only_the_requested_source() {
        let mut app = app_with_sources([("home", None), ("work", None)]);

        app.set_source_status("work", "Testing…");

        assert_eq!(app.sources[0].status, None);
        assert_eq!(app.sources[1].status.as_deref(), Some("Testing…"));
    }
}
