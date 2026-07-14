use std::fs;

use cc_switchy::agent::{
    effective_current_provider, Agent, AgentPaths, AgentRepository, DeviceSettings, McpProjector,
    ProviderProjector, SkillProjector, SkillSyncMethod,
};
use cc_switchy::progress::{NoopProgress, ProgressEvent, ProgressSink};
use cc_switchy::AppError;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static SYMLINK_ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct RecordingSkillProgress {
    events: Mutex<Vec<ProgressEvent>>,
    skills: Mutex<Vec<(String, String, usize, usize)>>,
}

impl ProgressSink for RecordingSkillProgress {
    fn emit(&self, event: ProgressEvent) {
        self.events.lock().expect("events lock").push(event);
    }

    fn emit_skill(&self, agent: String, skill: String, completed: usize, total: usize) {
        self.skills
            .lock()
            .expect("skills lock")
            .push((agent.clone(), skill, completed, total));
        self.emit(ProgressEvent::ApplyingSkills {
            agent,
            completed,
            total,
        });
    }
}

fn seeded_database(home: &TempDir) -> std::path::PathBuf {
    let path = home.path().join("cc-switch.db");
    let connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(include_str!("fixtures/cc-switch-v2/db.sql"))
        .expect("fixture schema");
    connection
        .execute_batch(
            "DELETE FROM providers;
             DELETE FROM mcp_servers;
             DELETE FROM skills;
             INSERT INTO providers (id, app_type, name, settings_config, created_at, sort_index, meta, is_current)
             VALUES
               ('fallback', 'codex', 'Fallback', '{\"api_key\":\"fallback\"}', 2, NULL, '{\"unknownFutureField\":{\"keep\":true}}', 1),
               ('sorted', 'codex', 'Sorted', '{\"api_key\":\"sorted\"}', 1, 10, '{\"commonConfigEnabled\":true,\"liveConfigManaged\":false}', 0),
               ('claude-main', 'claude', 'Claude Main', '{}', 1, 1, '{}', 1);
             INSERT INTO mcp_servers (id, name, server_config, tags, enabled_claude, enabled_codex)
             VALUES ('docs', 'Docs', '{\"command\":\"mcp-docs\",\"args\":[]}', '[\"docs\"]', 1, 0);
             INSERT INTO skills (id, name, directory, enabled_codex, installed_at, updated_at)
             VALUES ('demo', 'Demo', 'demo', 1, 1, 1);
             INSERT OR REPLACE INTO settings (key, value) VALUES ('fixtureSetting', 'fixture-value');",
        )
        .expect("seed rows");
    path
}

fn provider_database(home: &TempDir) -> std::path::PathBuf {
    let path = home.path().join("providers.db");
    let connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(include_str!("fixtures/cc-switch-v2/db.sql"))
        .expect("fixture schema");
    connection
        .execute_batch(
            r#"DELETE FROM providers;
               INSERT INTO providers (id, app_type, name, settings_config, created_at, sort_index, meta, is_current)
               VALUES
                 ('claude-a', 'claude', 'Claude A', '{"env":{"ANTHROPIC_AUTH_TOKEN":"claude-a"}}', 1, 1, '{"commonConfigEnabled":true}', 1),
                 ('claude-b', 'claude', 'Claude B', '{"env":{"ANTHROPIC_AUTH_TOKEN":"claude-b"}}', 2, 2, '{}', 0),
                 ('codex-a', 'codex', 'Codex A', '{"auth":{"OPENAI_API_KEY":"codex-a"},"config":"model_provider = \"a\"\n[model_providers.a]\nbase_url = \"https://a.example/v1\"\n"}', 1, 1, '{}', 1),
                 ('gemini-a', 'gemini', 'Gemini A', '{"env":{"GEMINI_API_KEY":"gemini-a"},"config":{"theme":"system"}}', 1, 1, '{}', 1),
                 ('opencode-managed', 'opencode', 'OpenCode Managed', '{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://managed.example/v1"}}', 1, 1, '{}', 0),
                 ('opencode-db-only', 'opencode', 'OpenCode DB Only', '{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://skip.example/v1"}}', 2, 2, '{"liveConfigManaged":false}', 0),
                 ('openclaw-managed', 'openclaw', 'OpenClaw Managed', '{"baseUrl":"https://claw.example/v1","api":"openai-responses","models":[]}', 1, 1, '{}', 0),
                 ('hermes-managed', 'hermes', 'Hermes Managed', '{"base_url":"https://hermes.example/v1","api_key":"hermes-key","model":"gpt"}', 1, 1, '{}', 0),
                 ('desktop-proxy', 'claude-desktop', 'Desktop Proxy', '{"env":{"ANTHROPIC_AUTH_TOKEN":"desktop"}}', 1, 1, '{"claudeDesktopMode":"proxy"}', 1);
               INSERT OR REPLACE INTO settings (key, value)
               VALUES ('common_config_claude', '{"includeCoAuthoredBy":false}');"#,
        )
        .expect("provider rows");
    path
}

fn mcp_database(home: &TempDir) -> std::path::PathBuf {
    let path = provider_database(home);
    let connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(
            r#"DELETE FROM mcp_servers;
               INSERT INTO mcp_servers (
                 id, name, server_config, tags,
                 enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_hermes
               ) VALUES
                 ('stdio-managed', 'Managed stdio', '{"type":"stdio","command":"npx","args":["-y","mcp-demo"],"env":{"TOKEN":"value"}}', '[]', 1, 1, 1, 1, 1),
                 ('remote-managed', 'Managed remote', '{"type":"http","url":"https://mcp.example.test/api","headers":{"Authorization":"Bearer test"}}', '[]', 0, 1, 1, 1, 1),
                 ('known-disabled', 'Known disabled', '{"type":"sse","url":"https://disabled.example.test/sse"}', '[]', 0, 0, 0, 0, 0);"#,
        )
        .expect("MCP rows");
    path
}

fn skills_database(home: &TempDir) -> std::path::PathBuf {
    let path = provider_database(home);
    let connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(
            "DELETE FROM skills;
             INSERT INTO skills (
               id, name, directory, enabled_claude, enabled_codex, enabled_gemini,
               enabled_opencode, enabled_hermes, installed_at, updated_at
             ) VALUES
               ('good', 'Good Skill', 'good', 1, 1, 1, 1, 1, 1, 1),
               ('disabled', 'Disabled Skill', 'disabled', 0, 0, 0, 0, 0, 1, 1),
               ('bad', 'Bad Skill', 'bad', 1, 0, 0, 0, 0, 1, 1);",
        )
        .expect("Skill rows");
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

#[test]
fn provider_projection_writes_exclusive_agents_and_merges_additive_agents() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let settings_path = home.path().join(".cc-switch/settings.json");
    fs::create_dir_all(settings_path.parent().expect("settings parent")).expect("settings dir");
    fs::write(
        &settings_path,
        r#"{"currentProviderClaude":"claude-a","currentProviderCodex":"codex-a","currentProviderGemini":"gemini-a"}"#,
    )
    .expect("settings");
    let opencode_path = home.path().join(".config/opencode/opencode.json");
    fs::create_dir_all(opencode_path.parent().expect("OpenCode parent")).expect("OpenCode dir");
    fs::write(
        &opencode_path,
        r#"{"$schema":"https://opencode.ai/config.json","provider":{"user-owned":{"npm":"custom"}},"unknownRoot":true}"#,
    )
    .expect("OpenCode live config");
    let mut settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let mut repo = AgentRepository::open(&db_path).expect("repository");
    let mut projector =
        ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress));

    let report = projector.project_all();

    let claude: serde_json::Value = serde_json::from_slice(
        &fs::read(home.path().join(".claude/settings.json")).expect("Claude settings"),
    )
    .expect("Claude JSON");
    assert_eq!(
        claude
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(|v| v.as_str()),
        Some("claude-a")
    );
    assert_eq!(
        claude.get("includeCoAuthoredBy").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(home.path().join(".codex/auth.json").exists());
    assert!(fs::read_to_string(home.path().join(".codex/config.toml"))
        .expect("Codex config")
        .contains("https://a.example/v1"));
    assert!(fs::read_to_string(home.path().join(".gemini/.env"))
        .expect("Gemini env")
        .contains("GEMINI_API_KEY=gemini-a"));
    let opencode: serde_json::Value =
        serde_json::from_slice(&fs::read(&opencode_path).expect("OpenCode output"))
            .expect("OpenCode JSON");
    assert!(opencode.pointer("/provider/user-owned").is_some());
    assert!(opencode.pointer("/provider/opencode-managed").is_some());
    assert!(opencode.pointer("/provider/opencode-db-only").is_none());
    assert_eq!(
        opencode.get("unknownRoot").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(home.path().join(".openclaw/openclaw.json").exists());
    assert!(home.path().join(".hermes/config.yaml").exists());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.agent == Some(Agent::ClaudeDesktop)));
}

#[test]
fn restore_projection_never_backfills_live_drift_into_the_database() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let settings_path = home.path().join("settings.json");
    fs::write(&settings_path, r#"{"currentProviderClaude":"claude-a"}"#).expect("settings");
    let live_path = home.path().join(".claude/settings.json");
    fs::create_dir_all(live_path.parent().expect("live parent")).expect("live dir");
    fs::write(
        &live_path,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"local-drift"}}"#,
    )
    .expect("live drift");
    let mut settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress))
        .project_agent(Agent::Claude)
        .expect("project Claude");

    let stored = repo
        .provider(Agent::Claude, "claude-a")
        .expect("provider")
        .expect("Claude A");
    assert_eq!(
        stored
            .settings_config
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(serde_json::Value::as_str),
        Some("claude-a")
    );
}

#[test]
fn invalid_codex_config_leaves_all_codex_files_unchanged() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute(
            "UPDATE providers SET settings_config=?1 WHERE id='codex-a' AND app_type='codex'",
            [r#"{"auth":{"OPENAI_API_KEY":"new"},"config":"[broken"}"#],
        )
        .expect("break Codex config");
    drop(connection);
    let codex_dir = home.path().join(".codex");
    fs::create_dir_all(&codex_dir).expect("Codex dir");
    fs::write(codex_dir.join("auth.json"), b"{\"old\":true}").expect("old auth");
    fs::write(codex_dir.join("config.toml"), b"model = \"old\"\n").expect("old config");
    let mut settings = DeviceSettings::default();
    settings.set_current_provider(Agent::Codex, Some("codex-a"));
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    assert!(
        ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress),)
            .project_agent(Agent::Codex)
            .is_err()
    );
    assert_eq!(
        fs::read(codex_dir.join("auth.json")).expect("auth"),
        b"{\"old\":true}"
    );
    assert_eq!(
        fs::read(codex_dir.join("config.toml")).expect("config"),
        b"model = \"old\"\n"
    );
}

#[test]
fn codex_model_catalog_is_generated_and_referenced_atomically() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute(
            "UPDATE providers SET settings_config=?1 WHERE id='codex-a' AND app_type='codex'",
            [r#"{
              "auth":{"OPENAI_API_KEY":"codex-a"},
              "config":"model_provider = \"a\"\nmodel_context_window = 200000\n[model_providers.a]\nbase_url = \"https://a.example/v1\"\n",
              "modelCatalog":{"models":[
                {"model":"vendor/coder","displayName":"Vendor Coder","contextWindow":180000},
                {"model":"vendor/coder"},
                {"model":"vendor/vision","inputModalities":["text","image"]}
              ]}
            }"#],
        )
        .expect("add model catalog");
    drop(connection);
    let mut settings = DeviceSettings::default();
    settings.set_current_provider(Agent::Codex, Some("codex-a"));
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress))
        .project_agent(Agent::Codex)
        .expect("project Codex catalog");

    let config =
        fs::read_to_string(home.path().join(".codex/config.toml")).expect("Codex config.toml");
    assert!(config.contains("model_catalog_json = \"cc-switch-model-catalog.json\""));
    let catalog: serde_json::Value = serde_json::from_slice(
        &fs::read(home.path().join(".codex/cc-switch-model-catalog.json")).expect("Codex catalog"),
    )
    .expect("catalog JSON");
    let models = catalog["models"].as_array().expect("catalog models");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["slug"], "vendor/coder");
    assert_eq!(models[0]["display_name"], "Vendor Coder");
    assert_eq!(models[0]["context_window"], 180000);
    assert_eq!(models[1]["context_window"], 200000);
    assert_eq!(
        models[1]["input_modalities"],
        serde_json::json!(["text", "image"])
    );
    assert!(models[0].get("apply_patch_tool_type").is_none());
}

#[test]
fn manual_exclusive_switch_updates_device_database_and_live_config() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute(
            "INSERT INTO mcp_servers (id, name, server_config, tags, enabled_claude)
             VALUES ('switch-mcp', 'Switch MCP', '{\"type\":\"stdio\",\"command\":\"mcp-switch\"}', '[]', 1)",
            [],
        )
        .expect("MCP row");
    drop(connection);
    let settings_path = home.path().join(".cc-switch/settings.json");
    fs::create_dir_all(settings_path.parent().expect("settings parent")).expect("settings dir");
    fs::write(&settings_path, r#"{"currentProviderClaude":"claude-a"}"#).expect("settings");
    let mut settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    fs::create_dir_all(home.path().join(".claude")).expect("Claude dir");
    fs::write(
        home.path().join(".claude/settings.json"),
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"claude-a"}}"#,
    )
    .expect("current live");
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress))
        .switch_exclusive(Agent::Claude, "claude-b")
        .expect("switch Claude");

    assert_eq!(settings.current_provider(Agent::Claude), Some("claude-b"));
    assert_eq!(
        repo.database_current_provider(Agent::Claude)
            .expect("database current")
            .as_deref(),
        Some("claude-b")
    );
    let live: serde_json::Value = serde_json::from_slice(
        &fs::read(home.path().join(".claude/settings.json")).expect("live settings"),
    )
    .expect("live JSON");
    assert_eq!(
        live.pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(serde_json::Value::as_str),
        Some("claude-b")
    );
    let mcp: serde_json::Value = serde_json::from_slice(
        &fs::read(home.path().join(".claude.json")).expect("Claude MCP after switch"),
    )
    .expect("Claude MCP JSON");
    assert_eq!(
        mcp.pointer("/mcpServers/switch-mcp/command")
            .and_then(serde_json::Value::as_str),
        Some("mcp-switch")
    );
}

#[test]
fn failed_manual_switch_restores_files_device_database_and_backfilled_provider() {
    let home = TempDir::new().expect("home");
    let db_path = provider_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute(
            "UPDATE providers SET settings_config='[]' WHERE id='claude-b' AND app_type='claude'",
            [],
        )
        .expect("make target invalid");
    drop(connection);

    let settings_path = home.path().join(".cc-switch/settings.json");
    fs::create_dir_all(settings_path.parent().expect("settings parent")).expect("settings dir");
    let original_settings = br#"{"currentProviderClaude":"claude-a","localOnly":true}"#;
    fs::write(&settings_path, original_settings).expect("settings");
    let live_path = home.path().join(".claude/settings.json");
    fs::create_dir_all(live_path.parent().expect("Claude parent")).expect("Claude dir");
    let original_live = br#"{"env":{"ANTHROPIC_AUTH_TOKEN":"local-drift"},"keep":true}"#;
    fs::write(&live_path, original_live).expect("live config");

    let mut settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let mut repo = AgentRepository::open(&db_path).expect("repository");

    assert!(
        ProviderProjector::new(&mut repo, &mut settings, &paths, Arc::new(NoopProgress),)
            .switch_exclusive(Agent::Claude, "claude-b")
            .is_err()
    );

    assert_eq!(settings.current_provider(Agent::Claude), Some("claude-a"));
    assert_eq!(
        fs::read(&settings_path).expect("settings rollback"),
        original_settings
    );
    assert_eq!(fs::read(&live_path).expect("live rollback"), original_live);
    assert_eq!(
        repo.database_current_provider(Agent::Claude)
            .expect("database current")
            .as_deref(),
        Some("claude-a")
    );
    let restored = repo
        .provider(Agent::Claude, "claude-a")
        .expect("provider query")
        .expect("Claude A");
    assert_eq!(
        restored
            .settings_config
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(serde_json::Value::as_str),
        Some("claude-a")
    );
}

#[test]
fn mcp_projection_preserves_unknown_entries_and_removes_known_disabled_entries() {
    let home = TempDir::new().expect("home");
    let db_path = mcp_database(&home);
    let claude_path = home.path().join(".claude.json");
    fs::write(
        &claude_path,
        r#"{"keepRoot":true,"mcpServers":{"user-owned":{"command":"local"},"known-disabled":{"url":"old"}}}"#,
    )
    .expect("Claude MCP");
    let codex_dir = home.path().join(".codex");
    fs::create_dir_all(&codex_dir).expect("Codex dir");
    fs::write(
        codex_dir.join("config.toml"),
        "model = \"keep\"\n[mcp_servers.user-owned]\ncommand = \"local\"\n[mcp_servers.known-disabled]\nurl = \"https://old\"\n",
    )
    .expect("Codex MCP");
    let gemini_dir = home.path().join(".gemini");
    fs::create_dir_all(&gemini_dir).expect("Gemini dir");
    fs::write(
        gemini_dir.join("settings.json"),
        r#"{"theme":"keep","mcpServers":{"user-owned":{"command":"local"},"known-disabled":{"url":"old"}}}"#,
    )
    .expect("Gemini MCP");
    let opencode_dir = home.path().join(".config/opencode");
    fs::create_dir_all(&opencode_dir).expect("OpenCode dir");
    fs::write(
        opencode_dir.join("opencode.json"),
        r#"{"provider":{"keep":{}},"mcp":{"user-owned":{"type":"local","command":["local"]},"known-disabled":{"type":"remote","url":"https://old"}}}"#,
    )
    .expect("OpenCode MCP");
    let hermes_dir = home.path().join(".hermes");
    fs::create_dir_all(&hermes_dir).expect("Hermes dir");
    fs::write(
        hermes_dir.join("config.yaml"),
        "agent:\n  keep: true\nmcp_servers:\n  user-owned:\n    command: local\n    timeout: 9\n  known-disabled:\n    url: https://old\n",
    )
    .expect("Hermes MCP");

    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");
    let report = McpProjector::new(&repo, &paths, Arc::new(NoopProgress)).project_all();

    let claude: serde_json::Value =
        serde_json::from_slice(&fs::read(&claude_path).expect("Claude output"))
            .expect("Claude JSON");
    assert_eq!(claude["keepRoot"], true);
    assert!(claude.pointer("/mcpServers/user-owned").is_some());
    assert!(claude.pointer("/mcpServers/stdio-managed").is_some());
    assert!(claude.pointer("/mcpServers/remote-managed").is_none());
    assert!(claude.pointer("/mcpServers/known-disabled").is_none());

    let codex: toml::Value =
        toml::from_str(&fs::read_to_string(codex_dir.join("config.toml")).expect("Codex output"))
            .expect("Codex TOML");
    assert_eq!(codex["model"].as_str(), Some("keep"));
    assert!(codex["mcp_servers"].get("user-owned").is_some());
    assert_eq!(
        codex["mcp_servers"]["stdio-managed"]["command"].as_str(),
        Some("npx")
    );
    assert_eq!(
        codex["mcp_servers"]["remote-managed"]["http_headers"]["Authorization"].as_str(),
        Some("Bearer test")
    );
    assert!(codex["mcp_servers"].get("known-disabled").is_none());

    let gemini: serde_json::Value =
        serde_json::from_slice(&fs::read(gemini_dir.join("settings.json")).expect("Gemini output"))
            .expect("Gemini JSON");
    assert_eq!(gemini["theme"], "keep");
    assert!(gemini.pointer("/mcpServers/user-owned").is_some());
    assert!(gemini.pointer("/mcpServers/stdio-managed").is_some());
    assert!(gemini.pointer("/mcpServers/known-disabled").is_none());

    let opencode: serde_json::Value = serde_json::from_slice(
        &fs::read(opencode_dir.join("opencode.json")).expect("OpenCode output"),
    )
    .expect("OpenCode JSON");
    assert!(opencode.pointer("/provider/keep").is_some());
    assert!(opencode.pointer("/mcp/user-owned").is_some());
    assert_eq!(opencode["mcp"]["stdio-managed"]["type"], "local");
    assert_eq!(
        opencode["mcp"]["stdio-managed"]["command"],
        serde_json::json!(["npx", "-y", "mcp-demo"])
    );
    assert_eq!(opencode["mcp"]["remote-managed"]["type"], "remote");
    assert!(opencode.pointer("/mcp/known-disabled").is_none());

    let hermes: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(hermes_dir.join("config.yaml")).expect("Hermes output"),
    )
    .expect("Hermes YAML");
    assert_eq!(hermes["agent"]["keep"].as_bool(), Some(true));
    assert!(hermes["mcp_servers"].get("user-owned").is_some());
    assert_eq!(
        hermes["mcp_servers"]["remote-managed"]["url"].as_str(),
        Some("https://mcp.example.test/api")
    );
    assert!(hermes["mcp_servers"].get("known-disabled").is_none());
    assert!(report.skipped_agents.contains(&Agent::ClaudeDesktop));
    assert!(report.skipped_agents.contains(&Agent::OpenClaw));
    assert!(report.warnings.is_empty());
}

#[test]
fn corrupt_mcp_config_warns_without_blocking_other_agents() {
    let home = TempDir::new().expect("home");
    let db_path = mcp_database(&home);
    fs::write(home.path().join(".claude.json"), "[").expect("corrupt Claude config");
    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");

    let report = McpProjector::new(&repo, &paths, Arc::new(NoopProgress)).project_all();

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.agent == Some(Agent::Claude)));
    assert!(report.applied_agents.contains(&Agent::Gemini));
    let gemini: serde_json::Value = serde_json::from_slice(
        &fs::read(home.path().join(".gemini/settings.json")).expect("Gemini output"),
    )
    .expect("Gemini JSON");
    assert!(gemini.pointer("/mcpServers/stdio-managed").is_some());
}

#[test]
fn skills_copy_reconciles_managed_targets_and_preserves_unrelated_directories() {
    let home = TempDir::new().expect("home");
    let db_path = skills_database(&home);
    let ssot = home.path().join(".cc-switch/skills");
    for name in ["good", "disabled", "orphan", "bad"] {
        fs::create_dir_all(ssot.join(name)).expect("SSOT Skill directory");
    }
    fs::write(ssot.join("good/SKILL.md"), "# Good\n").expect("Good SKILL.md");
    fs::write(ssot.join("good/data.txt"), "copied").expect("Good data");
    fs::write(ssot.join("disabled/SKILL.md"), "# Disabled\n").expect("Disabled SKILL.md");
    fs::write(ssot.join("orphan/SKILL.md"), "# Orphan\n").expect("Orphan SKILL.md");
    let settings_path = home.path().join("settings.json");
    fs::write(&settings_path, r#"{"skillSyncMethod":"copy"}"#).expect("settings");
    let settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let app_dir = paths.skills_dir(Agent::Claude).expect("Claude Skills path");
    fs::create_dir_all(app_dir.join("personal")).expect("personal Skill");
    fs::write(app_dir.join("personal/keep.txt"), "keep").expect("personal data");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(ssot.join("disabled"), app_dir.join("disabled"))
            .expect("disabled link");
        std::os::unix::fs::symlink(ssot.join("orphan"), app_dir.join("orphan"))
            .expect("orphan link");
    }

    let repo = AgentRepository::open(&db_path).expect("repository");
    let report =
        SkillProjector::new(&repo, &settings, &paths, Arc::new(NoopProgress)).project_all();

    let good = app_dir.join("good");
    assert!(good.join("SKILL.md").is_file());
    assert_eq!(
        fs::read_to_string(good.join("data.txt")).expect("copied data"),
        "copied"
    );
    assert!(good.join(".cc-switchy-managed").is_file());
    assert!(!good
        .symlink_metadata()
        .expect("Good metadata")
        .file_type()
        .is_symlink());
    assert!(app_dir.join("personal/keep.txt").is_file());
    assert!(!app_dir.join("disabled").exists());
    #[cfg(unix)]
    assert!(!app_dir.join("orphan").exists());
    assert!(fs::read_dir(&app_dir)
        .expect("Claude Skills entries")
        .all(|entry| !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.agent == Some(Agent::Claude)));
    assert!(report.applied_agents.contains(&Agent::Codex));
    assert!(report.skipped_agents.contains(&Agent::ClaudeDesktop));
    assert!(report.skipped_agents.contains(&Agent::OpenClaw));
}

#[test]
fn skills_auto_falls_back_to_copy_when_symlink_creation_fails() {
    let _environment_guard = SYMLINK_ENV_LOCK.lock().expect("symlink environment lock");
    let home = TempDir::new().expect("home");
    let db_path = skills_database(&home);
    let ssot = home.path().join(".cc-switch/skills/good");
    fs::create_dir_all(&ssot).expect("SSOT Skill");
    fs::write(ssot.join("SKILL.md"), "# Good\n").expect("SKILL.md");
    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");

    std::env::set_var("CC_SWITCHY_TEST_FORCE_SYMLINK_FAILURE", "1");
    let result = SkillProjector::new(&repo, &settings, &paths, Arc::new(NoopProgress))
        .project_agent(Agent::Codex);
    std::env::remove_var("CC_SWITCHY_TEST_FORCE_SYMLINK_FAILURE");
    result.expect("Auto fallback");

    let destination = paths
        .skills_dir(Agent::Codex)
        .expect("Codex Skills")
        .join("good");
    assert!(destination.join("SKILL.md").is_file());
    assert!(destination.join(".cc-switchy-managed").is_file());
    assert!(!destination
        .symlink_metadata()
        .expect("destination metadata")
        .file_type()
        .is_symlink());
}

#[test]
fn skill_progress_uses_directory_when_display_name_is_blank() {
    let home = TempDir::new().expect("home");
    let db_path = skills_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute("UPDATE skills SET name = '' WHERE id = 'good'", [])
        .expect("blank Skill name");
    let ssot = home.path().join(".cc-switch/skills/good");
    fs::create_dir_all(&ssot).expect("SSOT Skill");
    fs::write(ssot.join("SKILL.md"), "# Good\n").expect("SKILL.md");
    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");
    let progress = Arc::new(RecordingSkillProgress::default());

    SkillProjector::new(&repo, &settings, &paths, progress.clone())
        .project_agent(Agent::Codex)
        .expect("Skill projection");

    assert!(progress
        .events
        .lock()
        .expect("events lock")
        .iter()
        .any(|event| matches!(
            event,
            ProgressEvent::ApplyingSkills {
                agent,
                completed: 1,
                total: 1,
            } if agent == "Codex"
        )));
    assert!(progress.skills.lock().expect("skills lock").iter().any(
        |(agent, skill, completed, total)| agent == "Codex"
            && skill == "good"
            && *completed == 1
            && *total == 1
    ));
}

#[test]
fn skill_progress_sanitizes_and_bounds_remote_display_names() {
    let home = TempDir::new().expect("home");
    let db_path = skills_database(&home);
    let connection = Connection::open(&db_path).expect("database");
    let malicious = format!("Demo\nFORGED\u{1b}]0;owned\u{7}{}", "x".repeat(100));
    connection
        .execute(
            "UPDATE skills SET name = ?1 WHERE id = 'good'",
            [&malicious],
        )
        .expect("malicious Skill name");
    let ssot = home.path().join(".cc-switch/skills/good");
    fs::create_dir_all(&ssot).expect("SSOT Skill");
    fs::write(ssot.join("SKILL.md"), "# Good\n").expect("SKILL.md");
    let settings = DeviceSettings::default();
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");
    let progress = Arc::new(RecordingSkillProgress::default());

    SkillProjector::new(&repo, &settings, &paths, progress.clone())
        .project_agent(Agent::Codex)
        .expect("Skill projection");

    let skills = progress.skills.lock().expect("skills lock");
    let (_, label, _, _) = skills.first().expect("Skill progress");
    assert!(!label.chars().any(char::is_control));
    assert!(label.chars().count() <= 81);
    assert!(label.contains('�'));
    assert!(label.ends_with('…'));
}

#[cfg(unix)]
#[test]
fn skills_symlink_mode_links_only_to_a_valid_ssot_source() {
    let _environment_guard = SYMLINK_ENV_LOCK.lock().expect("symlink environment lock");
    let home = TempDir::new().expect("home");
    let db_path = skills_database(&home);
    let ssot = home.path().join(".cc-switch/skills/good");
    fs::create_dir_all(&ssot).expect("SSOT Skill");
    fs::write(ssot.join("SKILL.md"), "# Good\n").expect("SKILL.md");
    let settings_path = home.path().join("settings.json");
    fs::write(&settings_path, r#"{"skillSyncMethod":"symlink"}"#).expect("settings");
    let settings = DeviceSettings::load(&settings_path).expect("device settings");
    let paths = AgentPaths::from_settings(home.path(), &settings);
    let repo = AgentRepository::open(&db_path).expect("repository");

    SkillProjector::new(&repo, &settings, &paths, Arc::new(NoopProgress))
        .project_agent(Agent::Codex)
        .expect("Symlink projection");

    let destination = paths
        .skills_dir(Agent::Codex)
        .expect("Codex Skills")
        .join("good");
    assert!(destination
        .symlink_metadata()
        .expect("destination metadata")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(destination).expect("Skill link target"), ssot);
}
