use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn command(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("cc-switchy").expect("binary");
    cmd.env("CC_SWITCHY_TEST_HOME", home.path())
        .env("CC_SWITCH_TEST_HOME", home.path())
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG");
    cmd
}

#[test]
fn sync_without_configuration_prints_english_wizard_guidance() {
    let home = TempDir::new().expect("temp home");
    command(&home)
        .args(["--sync", "--lang", "en"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Run: cc-switchy --wizard"));
}

#[test]
fn sync_without_configuration_prints_chinese_wizard_guidance() {
    let home = TempDir::new().expect("temp home");
    command(&home)
        .args(["--sync", "--lang", "zh"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("请运行：cc-switchy --wizard"));
}

#[test]
fn wizard_and_sync_are_mutually_exclusive() {
    let home = TempDir::new().expect("temp home");
    command(&home)
        .args(["--wizard", "--sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn help_is_rendered_in_the_requested_language() {
    let home = TempDir::new().expect("temp home");
    command(&home)
        .args(["--lang", "zh", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("用法"));
    command(&home)
        .args(["--lang", "en", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}
