mod common;
use everyframe_miner::cli;
use std::process::Command;

fn parse(args: &[&str]) -> everyframe_miner::Result<cli::Args> {
    cli::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
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
