mod common;
use everyframe_miner::{cli, display};
use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn settings_are_global_human_by_default_and_json_only_on_request() {
    let home = common::tempdir();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_everycli"))
            .args(args)
            .env("HOME", home.path())
            .env_remove("EVERYCLI_DISABLE_AUTO_UPDATE")
            .output()
            .unwrap()
    };
    let out = run(&["auto-update"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("CLI AUTO-UPDATE"));
    assert!(
        !home.path().join(".config").exists(),
        "Status must not create config"
    );
    for (flag, enabled) in [("--disable", false), ("--enable", true)] {
        let out = run(&["auto-update", flag, "--json"]);
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["enabled"], enabled);
        assert_eq!(v["nativeInstallation"], false);
        assert!(v["lastAttempt"].is_null());
    }
    assert!(!home.path().join(".config/everycli-mainnet117").exists());
    let out = run(&["auto-update", "--enable", "--disable", "--json"]);
    assert!(!out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["error"],
        "conflicting_auto_update_options"
    );
}

#[test]
fn environment_opt_out_does_not_change_saved_preference() {
    let home = common::tempdir();
    let out = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args(["auto-update", "--json"])
        .env("HOME", home.path())
        .env("EVERYCLI_DISABLE_AUTO_UPDATE", "1")
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["enabled"], true);
    assert_eq!(v["effective"], false);
    assert_eq!(v["environmentDisabled"], true);
    assert!(!home.path().join(".config").exists());
}

#[test]
fn json_help_version_ci_and_nonterminal_do_not_schedule_updates() {
    let home = common::tempdir();
    let bin = home.path().join(".cargo/bin/everycli");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_everycli"), &bin).unwrap();
    for args in [
        vec!["--version"],
        vec!["--help"],
        vec!["providers", "--json"],
        vec!["providers"],
    ] {
        let out = Command::new(&bin)
            .args(args)
            .env("HOME", home.path())
            .env("CI", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        assert!(!home.path().join(".config/everycli-updater").exists());
    }
}

#[test]
fn automatic_update_options_do_not_silently_ignore_profile_flags() {
    for args in [
        vec!["auto-update", "--state-dir", "/tmp/example"],
        vec!["auto-update", "--yes"],
        vec!["status", "--disable"],
    ] {
        assert!(cli::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err());
    }
    let v = serde_json::json!({"enabled":true,"effective":true,"nativeInstallation":true,"lastAttempt":{"state":"failed","code":"release_checksum_mismatch"}});
    let text = display::render("auto-update", &v);
    assert!(text.contains("release_checksum_mismatch") && text.contains("everycli update"));
}

#[test]
fn cargo_install_builds_are_never_auto_updated_or_self_replaced() {
    let home = common::tempdir();
    let bin = home.path().join(".cargo/bin/everycli");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_everycli"), &bin).unwrap();
    let run = |args: &[&str]| {
        Command::new(&bin)
            .args(args)
            .env("HOME", home.path())
            .env_remove("CARGO_HOME")
            .env_remove("EVERYCLI_DISABLE_AUTO_UPDATE")
            .output()
            .unwrap()
    };
    // Installer-placed binary (no cargo records): not a cargo build.
    let v: Value = serde_json::from_slice(&run(&["auto-update", "--json"]).stdout).unwrap();
    assert_eq!(v["cargoInstall"], false);
    // `cargo install` records the binary in the install root.
    fs::write(
        home.path().join(".cargo/.crates2.json"),
        r#"{"installs":{"everyframe-miner 0.5.8 (git+https://github.com/everyframe-studio/everyframe-miner#b1c20c7)":{"bins":["everycli","everyframe-worker"]}}}"#,
    )
    .unwrap();
    let v: Value = serde_json::from_slice(&run(&["auto-update", "--json"]).stdout).unwrap();
    assert_eq!(v["cargoInstall"], true);
    assert_eq!(v["nativeInstallation"], false);
    assert_eq!(v["effective"], false);
    // Manual update stops before any network access and points to cargo.
    let before = fs::read(&bin).unwrap();
    let out = run(&["update", "--json"]);
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "cargo_install_update_disabled");
    assert!(v["next"].as_str().unwrap().contains("cargo install"));
    assert_eq!(fs::read(&bin).unwrap(), before);
    assert!(!bin.with_file_name("everycli.rollback").exists());
    assert!(!home.path().join(".config/everycli-updater").exists());
}
