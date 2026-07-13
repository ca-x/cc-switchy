mod model;
mod paths;
mod repository;
mod settings;

pub use model::{
    Agent, AgentFlags, InstalledSkill, McpServer, ProjectionReport, ProjectionStage,
    ProjectionWarning, Provider, ProviderMeta, SkillSyncMethod,
};
pub use paths::AgentPaths;
pub use repository::AgentRepository;
pub use settings::{effective_current_provider, DeviceSettings};
