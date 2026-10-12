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
            b"short-key\n".as_slice(),
            "That key looks too short (minimum 10 characters)",
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
    assert!(out.stderr.is_empty());
    assert_eq!(output(&out).to_lowercase().matches("removed").count(), 1);
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
        assert!(text.contains("Installed: v0.3.0") && text.contains("Latest: v0.3.1"));
        assert_eq!(text.contains("Run everycli update"), available);
        assert!(!text.starts_with('{'));
    }
    let a = cli::parse(
        &["register-hotkey", "--wallet", "alice", "--hotkey", "miner"].map(str::to_string),
    )
    .unwrap();
    let text = display::render(
        "register-hotkey",
        &json!({"registered":false,"registrationCommand":cli_errors::registration_command(Some(&a))}),
    );
    assert!(text.contains("--wallet.name alice --wallet.hotkey miner"));
    assert!(!text.contains("Replace <"));
    let text = display::render(
        "self-update",
        &json!({"previous":"0.3.1","version":"v0.3.2","updated":true}),
    );
    assert!(text.contains("from v0.3.1 to v0.3.2"));
}

#[test]
fn update_summaries_use_one_v_prefix_without_changing_json_values() {
    for previous in ["0.3.2", "v0.3.2"] {
        for latest in ["0.3.3", "v0.3.3"] {
            let value = json!({"previous":previous,"version":latest,"updated":true});
            let before = value.clone();
            assert!(
                display::render("self-update", &value)
                    .starts_with("Updated everycli from v0.3.2 to v0.3.3.")
            );
            assert_eq!(value, before);
        }
    }
}

#[test]
fn missing_wallet_lists_only_safe_directory_names_without_reading_keys() {
    use std::{fs, os::unix::fs::symlink};
    let dir = common::tempdir();
    let root = dir.path().join("wallets");
    fs::create_dir_all(root.join("zeta")).unwrap();
    fs::create_dir(root.join("alpha")).unwrap();
    fs::create_dir(root.join("bad\nname")).unwrap();
    fs::write(root.join("not-a-wallet"), "DO-NOT-ECHO").unwrap();
    symlink(root.join("alpha"), root.join("linked")).unwrap();
    let path = root.join("nosuch/hotkeys/default");
    let out = cli_errors::wallet_file_diagnostic(&path, &root).unwrap();
    assert_eq!(out["message"], "Wallet ‘nosuch’ not found.");
    assert_eq!(out["error"], "wallet_not_found");
    assert_eq!(out["availableWallets"], json!(["alpha", "zeta"]));
    let human = display::render("error", &out);
    assert!(human.contains("alpha, zeta"));
    assert!(!human.contains("DO-NOT-ECHO") && !human.contains("linked"));
    assert!(
        cli_errors::wallet_file_diagnostic(&root.join("alpha/hotkeys/missing"), &root).is_none()
    );
    assert!(cli_errors::wallet_file_diagnostic(&dir.path().join("elsewhere"), &root).is_none());
    let absent = dir.path().join("no-wallet-directory");
    let out = cli_errors::wallet_file_diagnostic(&absent.join("nosuch/hotkeys/default"), &absent)
        .unwrap();
    assert_eq!(out["availableWallets"], json!([]));
    assert!(out["next"].as_str().unwrap().contains("No wallets found"));
    let linked = dir.path().join("linked-root");
    symlink(&root, &linked).unwrap();
    assert!(
        cli_errors::wallet_file_diagnostic(&linked.join("nosuch/hotkeys/default"), &linked)
            .is_none()
    );
}

#[test]
fn short_key_json_errors_preserve_existing_credentials_and_yes_removal_is_quiet() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("profile"),
    };
    onboarding::save_keys(&state, &json!({"FAL_KEY":"synthetic-saved-key"}), None).unwrap();
    let before = state.read("credentials", false).unwrap();
    for key in ["x", "short-key"] {
        let out = run(
            &["set-api-keys", "--provider", "fal", "--stdin", "--json"],
            &state.directory,
            key.as_bytes(),
        );
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error"], "credential_too_short");
        assert_eq!(
            v["message"],
            "That key looks too short (minimum 10 characters)."
        );
        assert_eq!(state.read("credentials", false).unwrap(), before);
    }
    let out = run(
        &["remove-api-key", "--provider", "fal", "--yes", "--json"],
        &state.directory,
        b"",
    );
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["removed"],
        "FAL_KEY"
    );
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
        assert!(
            text.contains("5. Run everycli status") && text.contains("11. Run everycli doctor")
        );
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
