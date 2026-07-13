use cc_switchy::config::{
    ConfigStore, S3Config, SourceCatalog, SourceConfig, SourceKind, WebDavConfig,
};
use cc_switchy::{AppPaths, Language};
use tempfile::TempDir;

fn webdav(name: &str) -> SourceConfig {
    SourceConfig {
        name: name.to_string(),
        remote_root: "cc-switch-sync".to_string(),
        profile: "default".to_string(),
        kind: SourceKind::WebDav {
            webdav: WebDavConfig {
                base_url: "https://dav.example.test/root".to_string(),
                username: "user".to_string(),
                password: "secret".to_string(),
            },
        },
    }
}

fn s3(name: &str) -> SourceConfig {
    SourceConfig {
        name: name.to_string(),
        remote_root: "cc-switch-sync".to_string(),
        profile: "default".to_string(),
        kind: SourceKind::S3 {
            s3: S3Config {
                region: "auto".to_string(),
                bucket: "cc-switch".to_string(),
                endpoint: "https://account.r2.cloudflarestorage.com".to_string(),
                access_key_id: "access-key".to_string(),
                secret_access_key: "secret-key".to_string(),
            },
        },
    }
}

fn catalog(home: &TempDir) -> SourceCatalog {
    let paths = AppPaths::from_home(home.path());
    SourceCatalog::load(ConfigStore::new(paths.config_file)).expect("load catalog")
}

#[test]
fn first_source_becomes_default_and_round_trips() {
    let home = TempDir::new().expect("temp home");
    let paths = AppPaths::from_home(home.path());
    let mut catalog = catalog(&home);

    catalog.add(webdav("home")).expect("add source");

    assert_eq!(catalog.config().default_source.as_deref(), Some("home"));
    let loaded = SourceCatalog::load(ConfigStore::new(paths.config_file)).expect("reload");
    assert_eq!(loaded.config().sources[0].name, "home");
    assert_eq!(loaded.config().language, Language::Auto);
}

#[test]
fn exact_toml_shape_and_language_value_are_stable() {
    let home = TempDir::new().expect("temp home");
    let paths = AppPaths::from_home(home.path());
    let mut catalog = catalog(&home);

    catalog.add(webdav("home-webdav")).expect("add webdav");
    catalog.add(s3("backup-s3")).expect("add s3");
    catalog.set_language(Language::ZhCn).expect("set language");

    let content = std::fs::read_to_string(paths.config_file).expect("config text");
    assert!(content.contains("version = 1"));
    assert!(content.contains("language = \"zh-CN\""));
    assert!(content.contains("default_source = \"home-webdav\""));
    assert!(content.contains("type = \"webdav\""));
    assert!(content.contains("[sources.webdav]"));
    assert!(content.contains("type = \"s3\""));
    assert!(content.contains("[sources.s3]"));
}

#[test]
fn duplicate_names_are_rejected_without_rewriting_config() {
    let home = TempDir::new().expect("temp home");
    let paths = AppPaths::from_home(home.path());
    let mut catalog = catalog(&home);
    catalog.add(webdav("home")).expect("first add");
    let before = std::fs::read(&paths.config_file).expect("before bytes");

    assert!(catalog.add(webdav(" home ")).is_err());

    let after = std::fs::read(&paths.config_file).expect("after bytes");
    assert_eq!(after, before);
}

#[test]
fn deleting_default_selects_the_requested_replacement() {
    let home = TempDir::new().expect("temp home");
    let mut catalog = catalog(&home);
    catalog.add(webdav("a")).expect("add a");
    catalog.add(webdav("b")).expect("add b");

    catalog.delete("a", Some("b")).expect("delete a");

    assert_eq!(catalog.config().default_source.as_deref(), Some("b"));
    assert_eq!(catalog.config().sources.len(), 1);
}

#[test]
fn deleting_the_only_source_clears_the_default() {
    let home = TempDir::new().expect("temp home");
    let mut catalog = catalog(&home);
    catalog.add(webdav("only")).expect("add source");

    catalog.delete("only", None).expect("delete source");

    assert!(catalog.config().sources.is_empty());
    assert_eq!(catalog.config().default_source, None);
}

#[test]
fn updating_a_default_source_name_updates_the_default_reference() {
    let home = TempDir::new().expect("temp home");
    let mut catalog = catalog(&home);
    catalog.add(webdav("old")).expect("add source");

    catalog.update("old", webdav("new")).expect("rename source");

    assert_eq!(catalog.config().default_source.as_deref(), Some("new"));
}

#[test]
fn explicit_resolution_does_not_change_the_default() {
    let home = TempDir::new().expect("temp home");
    let mut catalog = catalog(&home);
    catalog.add(webdav("default")).expect("add default");
    catalog.add(s3("other")).expect("add other");

    let resolved = catalog.resolve(Some("other")).expect("resolve other");

    assert_eq!(resolved.name, "other");
    assert_eq!(catalog.config().default_source.as_deref(), Some("default"));
}

#[test]
fn invalid_webdav_scheme_is_rejected() {
    let home = TempDir::new().expect("temp home");
    let mut catalog = catalog(&home);
    let mut source = webdav("invalid");
    let SourceKind::WebDav { webdav } = &mut source.kind else {
        unreachable!();
    };
    webdav.base_url = "ftp://dav.example.test".to_string();

    assert!(catalog.add(source).is_err());
    assert!(!AppPaths::from_home(home.path()).config_file.exists());
}

#[test]
fn redacted_debug_never_contains_credentials() {
    let source = s3("private");
    let debug = format!("{:?}", source.redacted());

    assert!(!debug.contains("secret-key"));
    assert!(!debug.contains("access-key"));
    assert!(debug.contains("private"));
    assert!(debug.contains("cc-switch"));
}

#[test]
fn saving_a_second_revision_creates_a_backup() {
    let home = TempDir::new().expect("temp home");
    let paths = AppPaths::from_home(home.path());
    let mut catalog = catalog(&home);
    catalog.add(webdav("first")).expect("first revision");
    let first = std::fs::read(&paths.config_file).expect("first bytes");

    catalog.add(s3("second")).expect("second revision");

    let backup = std::fs::read(paths.config_file.with_extension("toml.bak")).expect("backup bytes");
    assert_eq!(backup, first);
}

#[cfg(unix)]
#[test]
fn config_and_backup_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().expect("temp home");
    let paths = AppPaths::from_home(home.path());
    let mut catalog = catalog(&home);
    catalog.add(webdav("first")).expect("first revision");
    catalog.add(s3("second")).expect("second revision");

    let config_mode = std::fs::metadata(&paths.config_file)
        .expect("config metadata")
        .permissions()
        .mode()
        & 0o777;
    let backup_mode = std::fs::metadata(paths.config_file.with_extension("toml.bak"))
        .expect("backup metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(config_mode, 0o600);
    assert_eq!(backup_mode, 0o600);
}
