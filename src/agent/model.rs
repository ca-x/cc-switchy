use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    #[serde(
        rename = "claude-desktop",
        alias = "claude_desktop",
        alias = "claudeDesktop"
    )]
    ClaudeDesktop,
    Codex,
    Gemini,
    OpenCode,
    OpenClaw,
    Hermes,
}

impl Agent {
    pub const ALL: [Agent; 7] = [
        Agent::Claude,
        Agent::ClaudeDesktop,
        Agent::Codex,
        Agent::Gemini,
        Agent::OpenCode,
        Agent::OpenClaw,
        Agent::Hermes,
    ];

    pub fn db_key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
            Self::OpenClaw => "openclaw",
            Self::Hermes => "hermes",
        }
    }

    pub fn is_additive(self) -> bool {
        matches!(self, Self::OpenCode | Self::OpenClaw | Self::Hermes)
    }

    pub fn supports_mcp(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Gemini | Self::OpenCode | Self::Hermes
        )
    }

    pub fn supports_skills(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Gemini | Self::OpenCode | Self::Hermes
        )
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Claude => "Claude",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini",
            Self::OpenCode => "OpenCode",
            Self::OpenClaw => "OpenClaw",
            Self::Hermes => "Hermes",
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct ProviderMeta(Map<String, Value>);

impl ProviderMeta {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn common_config_enabled(&self) -> bool {
        self.get("commonConfigEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn live_config_managed(&self) -> bool {
        self.get("liveConfigManaged")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }
}

#[derive(Clone)]
pub struct Provider {
    pub id: String,
    pub app: Agent,
    pub name: String,
    pub settings_config: Value,
    pub website_url: Option<String>,
    pub category: Option<String>,
    pub sort_index: Option<i64>,
    pub created_at: Option<i64>,
    pub notes: Option<String>,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub meta: ProviderMeta,
    pub is_current: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentFlags {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
    pub opencode: bool,
    pub hermes: bool,
}

impl AgentFlags {
    pub fn enabled_for(&self, agent: Agent) -> bool {
        match agent {
            Agent::Claude => self.claude,
            Agent::Codex => self.codex,
            Agent::Gemini => self.gemini,
            Agent::OpenCode => self.opencode,
            Agent::Hermes => self.hermes,
            Agent::ClaudeDesktop | Agent::OpenClaw => false,
        }
    }
}

#[derive(Clone)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub server: Value,
    pub apps: AgentFlags,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub docs: Option<String>,
    pub tags: Vec<String>,
}

impl McpServer {
    pub fn enabled_for(&self, agent: Agent) -> bool {
        self.apps.enabled_for(agent)
    }
}

#[derive(Clone)]
pub struct InstalledSkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub directory: String,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub repo_branch: Option<String>,
    pub readme_url: Option<String>,
    pub apps: AgentFlags,
    pub installed_at: i64,
    pub content_hash: Option<String>,
    pub updated_at: i64,
}

impl InstalledSkill {
    pub fn enabled_for(&self, agent: Agent) -> bool {
        self.apps.enabled_for(agent)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SkillSyncMethod {
    #[default]
    Auto,
    Symlink,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionStage {
    Provider,
    Mcp,
    Skills,
}

#[derive(Debug, Clone)]
pub struct ProjectionWarning {
    pub stage: ProjectionStage,
    pub agent: Option<Agent>,
    pub message: String,
}

#[derive(Debug, Default, Clone)]
pub struct ProjectionReport {
    pub applied_agents: Vec<Agent>,
    pub skipped_agents: Vec<Agent>,
    pub warnings: Vec<ProjectionWarning>,
}

impl ProjectionReport {
    pub fn merge(&mut self, other: Self) {
        self.applied_agents.extend(other.applied_agents);
        self.skipped_agents.extend(other.skipped_agents);
        self.warnings.extend(other.warnings);
    }
}
