mod common;
use everyframe_miner::{cli, cli_errors, cli_help, display, onboarding, state::State};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(words: &[&str], state: &std::path::Path, input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args(words)
        .arg("--state-dir")
        .arg(state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn parser_errors_keep_the_real_command_and_safe_flag_names() {
    let dir = common::tempdir();
    for (words, message, help) in [
        (
            vec!["doctor", "--bogus"],
            "Unsupported option --bogus for doctor",
            "help doctor",
        ),
        (
            vec!["--network", "mainnet", "doctor", "--bogus=DO-NOT-ECHO"],
            "Unsupported option --bogus for doctor",
            "help doctor",
        ),
        (
            vec!["miner", "doctor", "--stdin"],
            "Unsupported option --stdin for doctor",
            "help doctor",
        ),
        (
            vec!["offer", "--discount-pct", "10", "--withdraw"],
            "cannot combine --discount-pct with --withdraw",
            "help offer",
        ),
        (
            vec!["worker-update", "--stdin"],
            "Unsupported option --stdin for worker-update",
            "help worker-update",
        ),
    ] {
        for json_mode in [false, true] {
            let mut words = words.clone();
            if json_mode {
                words.push("--json");
            }
            let out = run(&words, dir.path(), b"");
            let text = output(&out);
            assert_eq!(out.status.code(), Some(2));
            assert!(text.contains(message), "{text}");
            assert!(text.contains(help), "{text}");
            assert!(!text.contains("help command") && !text.contains("DO-NOT-ECHO"));
            if json_mode {
                serde_json::from_slice::<Value>(&out.stdout).unwrap();
            }
        }
    }
    for flag in [
        "--secret-sensitive-key",
        "--bogus=DO-NOT-ECHO",
        "--key-DO-NOT-ECHO",
    ] {
        let out = run(&["doctor", flag, "--json"], dir.path(), b"");
        assert!(!output(&out).contains("sensitive-key"));
        assert!(!output(&out).contains("DO-NOT-ECHO"));
    }
}

#[test]
fn key_and_network_errors_are_actionable_without_leaking_values() {
    let dir = common::tempdir();
    let profile = dir.path().join("profile");
    for (words, input, expected) in [
        (
            vec!["set-api-keys"],
            b"".as_slice(),
            "--provider NAME --stdin",
        ),
        (
            vec!["set-api-keys", "--provider", "DO-NOT-ECHO"],
            b"".as_slice(),
            "Valid providers: phala, fal",
        ),
        (
            vec!["doctor", "--network", "DO-NOT-ECHO"],
            b"".as_slice(),
            "Use mainnet or testnet",
        ),
        (
            vec!["set-api-keys", "--provider", "fal", "--stdin"],
            b"\n".as_slice(),
            "The key was empty",
        ),
        (
            vec!["set-api-keys", "--provider", "fal", "--stdin"],
            b" \t\r\n".as_slice(),
            "The key was empty",
        ),
    ] {
        let out = run(&words, &profile, input);
        assert_eq!(out.status.code(), Some(2));
        assert!(output(&out).contains(expected), "{}", output(&out));
        assert!(!output(&out).contains("DO-NOT-ECHO"));
        assert!(!profile.join("credentials.json").exists());
    }
    let providers = onboarding::credential_providers();
    for name in ["vivix", "fal-billing", "openrouter-billing"] {
        assert!(providers.contains(&name));
    }
}

#[test]
fn key_changes_are_human_by_default_and_absent_removal_is_a_noop() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("profile"),
    };
    for json_mode in [false, true] {
        let mut words = vec!["remove-api-key", "--provider", "fal"];
        if json_mode {
            words.push("--json");
        }
        let out = run(&words, &state.directory, b"");
        assert!(out.status.success(), "{}", output(&out));
        assert!(!state.directory.join("credentials.json").exists());
        assert!(
            !output(&out).contains("Continue?") && !output(&out).contains("Keys saved locally")
        );
        if json_mode {
            let v: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert!(v["removed"].is_null());
            assert_eq!(v["notSaved"], "FAL_KEY");
        } else {
            assert!(output(&out).contains("Nothing removed"));
        }
    }
    let out = run(
        &["set-api-keys", "--provider", "fal", "--stdin"],
        &state.directory,
        b"synthetic-do-not-show-key\n",
    );
    assert!(out.status.success());
    assert!(output(&out).contains("Saved locally: fal"));
    assert!(!output(&out).contains("synthetic-do-not-show-key"));
    assert!(serde_json::from_slice::<Value>(&out.stdout).is_err());
    let out = run(
        &["remove-api-key", "--provider", "fal", "--yes"],
        &state.directory,
        b"",
    );
    assert!(out.status.success());
    assert!(output(&out).contains("Removed fal key"));
    assert!(!output(&out).contains("Keys saved locally"));
    assert!(
        state
            .read("credentials", false)
            .unwrap()
            .get("FAL_KEY")
            .is_none()
    );
    let absent = onboarding::save_keys(&state, &json!({}), Some("fal")).unwrap();
    assert!(absent["removed"].is_null());
    assert_eq!(absent["notSaved"], "FAL_KEY");
    assert_eq!(absent["savedLocally"], false);
}

#[test]
fn public_registration_and_updates_have_readable_summaries() {
    let a = cli::parse(&["register-hotkey", "--hotkey-ss58", "public-address"].map(str::to_string))
        .unwrap();
    let cmd = cli_errors::registration_command(Some(&a));
    assert!(cmd.contains("--wallet.name <wallet> --wallet.hotkey <hotkey>"));
    for registered in [true, false] {
        let v = json!({"registered":registered,"network":"mainnet","netuid":117,"hotkey":"public-address","uid":131,"registrationCommand":cmd});
        let text = display::render("register-hotkey", &v);
        assert!(text.contains("SN117") && text.contains("public-address"));
        assert_eq!(text.contains("btcli subnet register"), !registered);
        assert!(!text.contains("Everycli") && !text.starts_with('{'));
    }
    for available in [true, false] {
        let v = json!({"current":"0.3.0","latest":"v0.3.1","updated":false,"updateAvailable":available});
        let text = display::render("self-update", &v);
        assert!(text.contains("Installed: 0.3.0") && text.contains("Latest: v0.3.1"));
        assert_eq!(text.contains("Run everycli update"), available);
        assert!(!text.starts_with('{'));
    }
}

#[test]
fn continue_setup_is_replayable_local_only_and_never_exposes_the_journal() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("profile"),
    };
    for phase in ["awaiting_attestation", "awaiting_readmission", "running"] {
        state
            .write(
                "deployment",
                &json!({"phase":phase,"appId":"synthetic","private_field":"DO-NOT-ECHO"}),
            )
            .unwrap();
        let before = state.read("deployment", false).unwrap();
        let a = cli::parse(&["setup", "--continue"].map(str::to_string)).unwrap();
        cli::validate_args(&a).unwrap();
        let v = cli::setup_with(&a, &state, &|_| panic!("no prompt"), &mut |_| {
            panic!("no network or mutation")
        })
        .unwrap();
        assert_eq!(v["phase"], phase);
        assert_eq!(v["nextSteps"].as_array().unwrap().len(), 7);
        assert!(!v.to_string().contains("DO-NOT-ECHO"));
        assert_eq!(state.read("deployment", false).unwrap(), before);
        let text = display::render("setup", &v);
        assert!(text.contains("1. Run everycli status") && text.contains("7. Run everycli doctor"));
        assert!(text.contains("--state-dir") && text.contains("no"));
        let out = run(&["setup", "--continue", "--json"], &state.directory, b"");
        assert!(out.status.success());
        assert!(!output(&out).contains("DO-NOT-ECHO"));
    }
    state
        .write("deployment", &json!({"phase":"provision_intent"}))
        .unwrap();
    let out = run(&["setup", "--continue"], &state.directory, b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(output(&out).contains("reconcile"));
    let out = run(&["setup", "--continue"], &dir.path().join("empty"), b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(output(&out).contains("no local deployment"));
}

#[test]
fn readonly_help_omits_unused_consent_and_prompts_describe_saved_state() {
    for command in [
        "doctor",
        "status",
        "offers",
        "providers",
        "balances",
        "earnings",
        "register-hotkey",
        "init",
        "reconcile",
    ] {
        assert!(!cli_help::render(command).contains("--yes"), "{command}");
    }
    for command in ["deploy", "activate", "resume", "offer", "remove-api-key"] {
        assert!(cli_help::render(command).contains("--yes"), "{command}");
    }
    assert!(cli::key_prompt("fal", true).contains("Saved — Enter keeps existing"));
    assert!(cli::key_prompt("fal", false).contains("Not saved — Enter skips"));
}
