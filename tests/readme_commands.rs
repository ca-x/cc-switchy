use std::fs;

use cc_switchy::{Cli, RunMode};
use clap::Parser;

#[test]
fn documented_commands_match_the_cli_contract() {
    let readme = fs::read_to_string("README.md").expect("README.md");
    for command in [
        "cc-switchy",
        "cc-switchy --wizard",
        "cc-switchy --sync",
        "cc-switchy --sync --source backup-s3",
        "cc-switchy --lang zh",
        "cc-switchy --lang en",
    ] {
        assert!(readme.contains(command), "README is missing {command}");
    }

    assert!(matches!(
        Cli::try_parse_from(["cc-switchy"])
            .expect("bare command")
            .run_mode(),
        RunMode::Tui { source: None }
    ));
    assert!(matches!(
        Cli::try_parse_from(["cc-switchy", "--wizard"])
            .expect("wizard command")
            .run_mode(),
        RunMode::Wizard
    ));
    assert!(matches!(
        Cli::try_parse_from(["cc-switchy", "--sync"])
            .expect("sync command")
            .run_mode(),
        RunMode::Sync { source: None }
    ));
    assert!(matches!(
        Cli::try_parse_from(["cc-switchy", "--sync", "--source", "backup-s3"])
            .expect("source override")
            .run_mode(),
        RunMode::Sync {
            source: Some(source)
        } if source == "backup-s3"
    ));
    assert!(Cli::try_parse_from(["cc-switchy", "--lang", "zh"]).is_ok());
    assert!(Cli::try_parse_from(["cc-switchy", "--lang", "en"]).is_ok());

    for key_contract in [
        "Tab/Shift+Tab",
        "`q` 在非表单界面退出",
        "`Ctrl+C` 可从任意向导界面退出",
        "`q` exits outside forms",
        "`Ctrl+C` exits from every Wizard screen",
    ] {
        assert!(
            readme.contains(key_contract),
            "README is missing {key_contract}"
        );
    }
}

#[test]
fn readme_documents_security_paths_results_and_release_targets() {
    let readme = fs::read_to_string("README.md").expect("README.md");
    for required in [
        "WebDAV",
        "S3",
        "db.sql",
        "~/.cc-switchy",
        "~/.cc-switch",
        "Exit code `0`",
        "Exit code `1`",
        "Exit code `2`",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "Grok Build",
        "SQLite schema v16",
        "`1` Switch, `2` Sync",
    ] {
        assert!(readme.contains(required), "README is missing {required}");
    }
}
