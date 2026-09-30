//! Test the `slumber collection` subcommand

mod common;

use predicates::{prelude::predicate, str::PredicateStrExt};
use std::fs;

/// `slumber collection` prints the loaded collection in YAML
#[test]
fn test_print_config() {
    let (mut command, _) = common::slumber();
    command.args(["collection"]);
    let expected =
        serde_yaml::to_string(&common::collection_file().load().unwrap())
            .unwrap();
    command.assert().success().stdout(predicate::eq(expected));
}

/// `slumber collection --path` prints the collection path
#[test]
fn test_print_path() {
    let (mut command, _) = common::slumber();
    command.args(["collection", "--path"]);
    let expected = common::collection_file().path().display().to_string();
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
    fs::write(&config, "editor: cat\n").unwrap();
    command
        .env("SLUMBER_CONFIG_PATH", &config)
        .env("EDITOR", "nonexistent-editor")
        .args(["collection", "--edit"]);
    let expected =
        fs::read_to_string(common::collection_file().path()).unwrap();
    command.assert().success().stdout(predicate::eq(expected));
}

/// No implicit subprocess fallback when interactive editing has no terminal.
/// Real terminal save/cancel is tested by the packaged-executable Actions test.
#[test]
fn test_builtin_requires_terminal_without_spawning() {
    let (mut command, data_dir) = common::slumber();
    let config = data_dir.join("config.yml");
    fs::write(&config, "editor: builtin\n").unwrap();
    let collection = common::collection_file();
    let original = fs::read(collection.path()).unwrap();
    command
        .env("SLUMBER_CONFIG_PATH", &config)
        .env("PATH", "")
        .env("EDITOR", "nonexistent-editor")
        .env("VISUAL", "nonexistent-editor")
        .args(["collection", "--edit"])
        .assert()
        .failure()
        .stdout(predicate::eq(""))
        .stderr(predicate::str::contains("interactive terminal"));
    assert_eq!(fs::read(collection.path()).unwrap(), original);
}
