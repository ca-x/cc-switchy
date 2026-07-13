mod mcp;
mod model;
mod paths;
mod provider;
mod repository;
mod settings;

pub use mcp::McpProjector;
pub use model::{
    Agent, AgentFlags, InstalledSkill, McpServer, ProjectionReport, ProjectionStage,
    ProjectionWarning, Provider, ProviderMeta, SkillSyncMethod,
};
pub use paths::AgentPaths;
pub use provider::ProviderProjector;
pub use repository::AgentRepository;
pub use settings::{effective_current_provider, DeviceSettings};
