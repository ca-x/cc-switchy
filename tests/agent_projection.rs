use std::fs;

use cc_switchy::agent::{
    effective_current_provider, Agent, AgentPaths, AgentRepository, DeviceSettings, SkillSyncMethod,
};
use cc_switchy::AppError;
use rusqlite::Connection;
use tempfile::TempDir;

fn seeded_database(home: &TempDir) -> std::path::PathBuf {
    let path = home.path().join("cc-switch.db");
    let connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(include_str!("fixtures/cc-switch-v2/db.sql"))
        .expect("fixture schema");
    connection
        .execute_batch(
            "DELETE FROM providers;
             INSERT INTO providers (id, app_type, name, settings_config, created_at, sort_index, meta, is_current)
             VALUES
               ('fallback', 'codex', 'Fallback', '{\"api_key\":\"fallback\"}', 2, NULL, '{\"unknownFutureField\":{\"keep\":true}}', 1),
               ('sorted', 'codex', 'Sorted', '{\"api_key\":\"sorted\"}', 1, 10, '{\"commonConfigEnabled\":true,\"liveConfigManaged\":false}', 0),
               ('claude-main', 'claude', 'Claude Main', '{}', 1, 1, '{}', 1);
             INSERT INTO mcp_servers (id, name, server_config, tags, enabled_claude, enabled_codex)
             VALUES ('docs', 'Docs', '{\"command\":\"mcp-docs\",\"args\":[]}', '[\"docs\"]', 1, 0);
             INSERT OR REPLACE INTO settings (key, value) VALUES ('fixtureSetting', 'fixture-value');",
        )
        .expect("seed rows");
    path
}

#[test]
fn repository_orders_providers_and_reads_current_selection() {
    let home = TempDir::new().expect("home");
    let db_path = seeded_database(&home);
    let repo = AgentRepository::open(&db_path).expect("repository");

    let codex = repo.providers(Agent::Codex).expect("codex providers");
    assert_eq!(
        codex
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["sorted", "fallback"]
    );
    assert_eq!(
        repo.database_current_provider(Agent::Codex)
            .expect("database current")
            .as_deref(),
        Some("fallback")
    );
    assert!(codex[0].meta.common_config_enabled());
    assert!(!codex[0].meta.live_config_managed());
    assert_eq!(
        codex[1]
            .meta
            .get("unknownFutureField")
            .and_then(|value| value.get("keep"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn repository_reads_mcp_skills_and_settings() {
    let home = TempDir::new().expect("home");
    let db_path = seeded_database(&home);
    let repo = AgentRepository::open(&db_path).expect("repository");

    let mcp = repo.mcp_servers().expect("MCP servers");
    assert_eq!(mcp.len(), 1);
    assert!(mcp[0].enabled_for(Agent::Claude));
    assert!(!mcp[0].enabled_for(Agent::Codex));
    let skills = repo.installed_skills().expect("installed Skills");
    assert_eq!(skills[0].directory, "demo");
    assert!(skills[0].enabled_for(Agent::Codex));
    assert_eq!(
        repo.setting("fixtureSetting").expect("setting").as_deref(),
        Some("fixture-value")
    );
}

#[test]
fn valid_local_provider_wins_and_stale_provider_is_cleared_atomically() {
    let home = TempDir::new().expect("home");
    let db_path = seeded_database(&home);
    let repo = AgentRepository::open(&db_path).expect("repository");
    let settings_path = home.path().join(".cc-switch/settings.json");
    fs::create_dir_all(settings_path.parent().expect("settings parent")).expect("settings dir");
    fs::write(
        &settings_path,
        r#"{"currentProviderCodex":"sorted","unknownDeviceKey":{"keep":true}}"#,
    )
    .expect("settings");
    let mut settings = DeviceSettings::load(&settings_path).expect("device settings");

    let selected = effective_current_provider(&repo, &mut settings, Agent::Codex)
        .expect("effective current")
        .expect("selected provider");
    assert_eq!(selected.id, "sorted");

    settings.set_current_provider(Agent::Codex, Some("removed-on-this-device"));
    settings.save_atomic(&settings_path).expect("save stale ID");
    let selected = effective_current_provider(&repo, &mut settings, Agent::Codex)
        .expect("fallback current")
        .expect("fallback provider");
    assert_eq!(selected.id, "fallback");
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings_path).expect("persisted settings"))
            .expect("settings JSON");
    assert!(persisted.get("currentProviderCodex").is_none());
    assert_eq!(
        persisted.pointer("/unknownDeviceKey/keep"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn device_settings_resolve_overrides_skills_and_sync_method() {
    let home = TempDir::new().expect("home");
    let settings_path = home.path().join("settings.json");
    fs::write(
        &settings_path,
        r#"{
          "claudeConfigDir": "~/custom-claude",
          "codexConfigDir": "/tmp/custom-codex",
          "skillStorageLocation": "unified",
          "skillSyncMethod": "copy"
        }"#,
    )
    .expect("settings");
    let settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);

    assert_eq!(
        paths.config_dir(Agent::Claude).expect("Claude path"),
        home.path().join("custom-claude")
    );
    assert_eq!(
        paths.config_dir(Agent::Codex).expect("Codex path"),
        std::path::PathBuf::from("/tmp/custom-codex")
    );
    assert_eq!(
        settings.skills_ssot(home.path()),
        home.path().join(".agents/skills")
    );
    assert_eq!(settings.skill_sync_method(), SkillSyncMethod::Copy);
}

#[cfg(target_os = "linux")]
#[test]
fn claude_desktop_does_not_invent_a_linux_path() {
    let home = TempDir::new().expect("home");
    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);

    assert!(matches!(
        paths.config_dir(Agent::ClaudeDesktop),
        Err(AppError::UnsupportedAgentFeature { .. })
    ));
}

#[test]
fn repository_can_update_database_current_provider_transactionally() {
    let home = TempDir::new().expect("home");
    let db_path = seeded_database(&home);
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    repo.set_database_current_provider(Agent::Codex, "sorted")
        .expect("switch database current");
    assert_eq!(
        repo.database_current_provider(Agent::Codex)
            .expect("database current")
            .as_deref(),
        Some("sorted")
    );
    assert!(repo
        .set_database_current_provider(Agent::Codex, "missing")
        .is_err());
}
