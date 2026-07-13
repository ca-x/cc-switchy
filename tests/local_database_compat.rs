use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cc_switchy::agent::{Agent, AgentPaths, AgentRepository, DeviceSettings, ProviderProjector};
use cc_switchy::progress::NoopProgress;
use tempfile::TempDir;

#[test]
#[ignore = "requires CC_SWITCHY_REAL_DB and is run manually against a private local database copy"]
fn projects_a_real_cc_switch_database_only_into_a_temporary_home() {
    let source = PathBuf::from(
        std::env::var_os("CC_SWITCHY_REAL_DB").expect("CC_SWITCHY_REAL_DB must be set"),
    );
    let temporary = TempDir::new().expect("temporary home");
    let database = temporary.path().join("cc-switch.db");
    fs::copy(&source, &database).expect("copy real database");

    let mut repo = AgentRepository::open(&database).expect("open copied database");
    for agent in [Agent::Claude, Agent::Codex, Agent::Gemini, Agent::OpenCode] {
        assert!(
            !repo.providers(agent).expect("read providers").is_empty(),
            "expected at least one {agent} provider in the compatibility sample"
        );
    }

    let settings_path = temporary.path().join(".cc-switch/settings.json");
    if let Some(source_settings) = std::env::var_os("CC_SWITCHY_REAL_SETTINGS") {
        let source_settings = PathBuf::from(source_settings);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&source_settings).expect("read real settings"))
                .expect("parse real settings");
        let selections = value
            .as_object()
            .expect("real settings must be an object")
            .iter()
            .filter(|(key, _)| key.starts_with("currentProvider"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<serde_json::Map<_, _>>();
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("temporary settings directory");
        fs::write(
            &settings_path,
            serde_json::to_vec(&selections).expect("serialize safe selections"),
        )
        .expect("write temporary selections");
    }
    let mut settings = DeviceSettings::load(&settings_path).expect("temporary settings");
    for agent in [Agent::Claude, Agent::Codex, Agent::Gemini] {
        if settings.current_provider(agent).is_none()
            && repo
                .database_current_provider(agent)
                .expect("database current")
                .is_none()
        {
            let first = repo
                .providers(agent)
                .expect("fallback providers")
                .into_iter()
                .next()
                .expect("provider sample");
            settings.set_current_provider(agent, Some(&first.id));
        }
    }
    let paths = AgentPaths::from_settings(temporary.path(), &settings);
    let report = ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress))
        .project_all();

    for agent in [Agent::Claude, Agent::Codex, Agent::Gemini, Agent::OpenCode] {
        assert!(
            report.applied_agents.contains(&agent),
            "real compatibility projection did not apply {agent}: {:?}",
            report.warnings
        );
    }
}
