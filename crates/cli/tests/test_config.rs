//! Test the `slumber config` subcommand

mod common;

use predicates::{prelude::predicate, str::PredicateStrExt};
use slumber_config::Config;
use std::fs;

/// `slumber config` prints the loaded config in YAML
#[test]
fn test_print_config() {
    let (mut command, _) = common::slumber();
    command.args(["config"]);
    let expected = serde_yaml::to_string(&Config::default()).unwrap();
    command.assert().success().stdout(predicate::eq(expected));
}

/// `slumber config --path` prints the config path
#[test]
fn test_print_path() {
    let (mut command, data_dir) = common::slumber();
    command.args(["config", "--path"]);
    let expected = data_dir.join("config.yml").display().to_string();
    command
        .assert()
        .success()
        .stdout(predicate::eq(expected).trim());
}

/// Explicit normal-mode editor integration remains available.
#[test]
fn test_explicit_external_editor() {
    let (mut command, data_dir) = common::slumber();
    let config = data_dir.join("config.yml");
    let expected = "editor: cat\n";
    fs::write(&config, expected).unwrap();
    command
        .env("SLUMBER_CONFIG_PATH", &config)
        .env("EDITOR", "nonexistent-editor")
        .args(["config", "--edit"]);
    command.assert().success().stdout(predicate::eq(expected));
}

/// Built-in editing cannot silently call EDITOR when stdin is not a terminal.
#[test]
fn test_builtin_requires_terminal_without_spawning() {
    let (mut command, data_dir) = common::slumber();
    let config = data_dir.join("config.yml");
    let original = "editor: builtin\n";
    fs::write(&config, original).unwrap();
    command
        .env("SLUMBER_CONFIG_PATH", &config)
        .env("PATH", "")
        .env("EDITOR", "nonexistent-editor")
        .env("VISUAL", "nonexistent-editor")
        .args(["config", "--edit"])
        .assert()
        .failure()
        .stdout(predicate::eq(""))
        .stderr(predicate::str::contains("interactive terminal"));
    assert_eq!(fs::read_to_string(config).unwrap(), original);
}
