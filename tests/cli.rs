//! Integration tests for the command line interface.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;

fn blogen() -> Command {
    Command::cargo_bin("blogen").expect("binary is built")
}

#[test]
fn version_and_help() {
    blogen()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("blogen"));
    blogen()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("static blog generator"));
}

#[test]
fn theme_list_shows_default_theme() {
    blogen()
        .args(["theme", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn init_creates_site_and_build_generates_output() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("cli-site");
    blogen()
        .args([
            "init",
            dir.to_str().unwrap(),
            "--name",
            "CLI Blog",
            "--url",
            "https://cli.example",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("blogen.toml"));

    blogen()
        .args([
            "build",
            "--config",
            dir.join("blogen.toml").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Built 1 article"));

    assert!(dir.join("public/index.html").is_file());
    assert!(dir.join("public/rss.xml").is_file());
}

#[test]
fn build_without_config_fails_helpfully() {
    let temp = tempfile::tempdir().unwrap();
    blogen()
        .current_dir(temp.path())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("blogen.toml"));
}

#[test]
fn theme_extract_produces_loadable_theme() {
    let temp = tempfile::tempdir().unwrap();
    let theme_dir = temp.path().join("my-theme");
    blogen()
        .args(["theme", "extract", "default", theme_dir.to_str().unwrap()])
        .assert()
        .success();
    assert!(theme_dir.join("theme.toml").is_file());
    assert!(theme_dir.join("templates/article.html").is_file());
    assert!(theme_dir.join("assets/main.css").is_file());
}

#[test]
fn clean_removes_generated_output() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("cli-site");
    blogen()
        .args(["init", dir.to_str().unwrap()])
        .assert()
        .success();
    let config = dir.join("blogen.toml");
    blogen()
        .args(["build", "--config", config.to_str().unwrap()])
        .assert()
        .success();
    let output = dir.join("public");
    assert!(output.join("index.html").is_file());
    blogen()
        .args(["clean", "--config", config.to_str().unwrap()])
        .assert()
        .success();
    assert!(!output.join("index.html").exists());
}
