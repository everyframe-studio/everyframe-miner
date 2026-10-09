mod common;
use everyframe_miner::cli;
use std::process::Command;

fn parse(args: &[&str]) -> everyframe_miner::Result<cli::Args> {
    cli::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn help_uses_the_current_brand_without_renaming_the_command() {
    assert!(cli::HELP.starts_with("everycli · EveryFrame miner tools"));
    assert!(!cli::HELP.contains("Everyframe"));
}

#[test]
fn every_miner_command_is_top_level_with_legacy_equivalence() {
    for command in [
        "init",
        "register-hotkey",
        "set-api-keys",
        "remove-api-key",
        "apply-api-keys",
        "doctor",
        "status",
        "providers",
        "balances",
        "offers",
        "offer",
        "earnings",
        "deploy",
        "activate",
        "resume",
        "stop",
        "start",
        "reconcile",
    ] {
        let flat = parse(&[command, "--network", "mainnet", "--json"]).unwrap();
        let legacy = parse(&["miner", command, "--network", "mainnet", "--json"]).unwrap();
        assert_eq!(flat.command, command);
        assert_eq!(flat.command, legacy.command);
        assert_eq!(flat.flags, legacy.flags);
        assert_eq!(flat.options, legacy.options);
    }
    for args in [
        vec!["init", "--wallet", "my-wallet", "--hotkey", "default"],
        vec!["set-api-keys", "--provider", "fal-billing", "--stdin"],
        vec!["balances", "--publish", "--json"],
        vec!["offer", "--model", "fixture/video", "--discount-pct", "10"],
        vec!["stop", "--drain-only"],
        vec!["deploy", "--max-hourly-usd", "0.06"],
    ] {
        let flat = parse(&args).unwrap();
        let mut old = vec!["miner"];
        old.extend(&args);
        let legacy = parse(&old).unwrap();
        assert_eq!(flat.command, legacy.command);
        assert_eq!(flat.options, legacy.options);
        assert_eq!(flat.flags, legacy.flags);
    }
}

#[test]
fn upgrades_have_unambiguous_names_and_scoped_flags() {
    assert_eq!(parse(&["update"]).unwrap().command, "self-update");
    assert_eq!(
        parse(&["worker-update", "--release", "approved.json"])
            .unwrap()
            .command,
        "update"
    );
    assert_eq!(
        parse(&["miner", "update", "--release", "approved.json"])
            .unwrap()
            .command,
        "update"
    );
    for args in [
        vec!["update", "--release", "approved.json"],
        vec!["worker-update", "--check"],
        vec!["status", "--publish"],
        vec!["doctor", "--stdin"],
        vec!["offer", "--withdraw", "--discount-pct", "10"],
        vec!["status", "extra"],
        vec!["miner", "status", "extra"],
        vec!["self-update"],
    ] {
        assert!(
            parse(&args).is_err(),
            "accepted invalid arguments: {args:?}"
        );
    }
}

#[test]
fn help_and_runtime_dispatch_use_direct_commands() {
    let binary = env!("CARGO_BIN_EXE_everycli");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("Usage: everycli COMMAND"));
    assert!(text.contains("everycli worker-update --release FILE"));
    assert!(!text.contains("everycli miner"));
    let dir = common::tempdir();
    let state = dir.path().join("profile");
    let doctor = Command::new(binary)
        .args(["doctor", "--json", "--state-dir"])
        .arg(&state)
        .output()
        .unwrap();
    assert_eq!(doctor.status.code(), Some(2));
    let result: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(result["ok"], false);
    let unknown = Command::new(binary)
        .arg("secret-argument-must-not-be-echoed")
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    let error = String::from_utf8(unknown.stderr).unwrap();
    assert!(error.contains("everycli --help"));
    assert!(error.contains("everycli doctor"));
    assert!(!error.contains("secret-argument-must-not-be-echoed"));
}

#[test]
fn profile_diagnostics_distinguish_setup_from_broken_or_unsafe_files() {
    use everyframe_miner::state::State;
    use serde_json::json;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    let dir = common::tempdir();
    for (case, expected) in [
        ("fresh", "profile_not_initialized"),
        ("keys_only", "profile_not_initialized"),
        ("registered_only", "profile_not_initialized"),
        ("missing_credentials", "profile_credentials_missing"),
        ("corrupt", "invalid_json_file"),
        ("permissions", "insecure_file"),
        ("directory_permissions", "insecure_directory"),
        ("symlink", "unsafe_path"),
    ] {
        let state = State {
            directory: dir.path().join(case),
        };
        match case {
            "keys_only" => state
                .write("credentials", &json!({"FAL_KEY":"secret-never-print"}))
                .unwrap(),
            "registered_only" => state
                .write("registration", &json!({"registered":true}))
                .unwrap(),
            "missing_credentials" => state.write("config", &json!({})).unwrap(),
            "corrupt" => {
                state.write("config", &json!({})).unwrap();
                fs::write(state.path("config").unwrap(), b"broken secret-never-print").unwrap();
            }
            "permissions" => {
                state.write("config", &json!({})).unwrap();
                fs::set_permissions(
                    state.path("config").unwrap(),
                    fs::Permissions::from_mode(0o644),
                )
                .unwrap();
            }
            "directory_permissions" => {
                state.prepare().unwrap();
                fs::set_permissions(&state.directory, fs::Permissions::from_mode(0o755)).unwrap();
            }
            "symlink" => {
                state.prepare().unwrap();
                symlink(
                    state.directory.join("missing"),
                    state.path("config").unwrap(),
                )
                .unwrap();
            }
            _ => {}
        }
        for command in ["status", "doctor"] {
            let output = Command::new(env!("CARGO_BIN_EXE_everycli"))
                .args([command, "--json", "--state-dir"])
                .arg(&state.directory)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{case}/{command}");
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["error"], expected, "{case}/{command}");
            assert_eq!(value["registration"], "not_checked");
            assert_eq!(value["ok"], false);
            assert!(value["next"].as_str().unwrap().len() > 20);
            if command == "doctor" {
                assert_eq!(value["checks"][0]["detail"], expected);
            }
            assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-never-print"));
        }
    }
}

#[test]
fn missing_profile_has_actionable_human_output_including_legacy_commands() {
    let dir = common::tempdir();
    for command in [
        vec!["status"],
        vec!["doctor"],
        vec!["miner", "status"],
        vec!["miner", "doctor"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_everycli"))
            .args(command)
            .arg("--state-dir")
            .arg(dir.path().join("fresh"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("No local miner profile found"));
        assert!(text.contains("everycli init --wallet <wallet> --hotkey default"));
        assert!(text.contains("have not been checked"));
        assert!(!text.contains("file unavailable"));
        assert!(output.stderr.is_empty());
    }
}
