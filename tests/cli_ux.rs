mod common;
use everyframe_miner::{Error, cli, cli_errors, cli_help, display, invitation, state::State};
use serde_json::{Value, json};
use std::{
    process::{Command, Output},
    time::Instant,
};

fn args(words: &[&str]) -> cli::Args {
    cli::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}
fn run(words: &[&str], state: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args(words)
        .arg("--state-dir")
        .arg(state)
        .output()
        .unwrap()
}
fn output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn every_command_has_help_and_example_without_profile_or_network() {
    for command in cli_help::COMMANDS.iter().filter(|c| **c != "help") {
        let a = Command::new(env!("CARGO_BIN_EXE_everycli"))
            .args([command, "--help"])
            .output()
            .unwrap();
        let b = Command::new(env!("CARGO_BIN_EXE_everycli"))
            .args(["help", command])
            .output()
            .unwrap();
        assert!(a.status.success(), "{command}: {}", output(&a));
        assert!(b.status.success(), "{command}: {}", output(&b));
        assert_eq!(a.stdout, b.stdout);
        let text = output(&a);
        assert!(
            text.contains(&format!("Usage: everycli {command}")),
            "{text}"
        );
        assert!(text.contains("Example:"));
        assert!(!text.contains("No local miner profile"));
    }
    let global = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .arg("help")
        .output()
        .unwrap();
    assert!(global.status.success());
    assert!(output(&global).contains("Usage: everycli COMMAND"));
    let json_help = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args(["deploy", "--help", "--json"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&json_help.stdout).unwrap();
    assert!(value["help"].as_str().unwrap().contains("--max-hourly-usd"));
}

#[test]
fn missing_options_and_bad_values_fail_before_profile_creation() {
    let dir = common::tempdir();
    let state = dir.path().join("never-created");
    for (words, expected) in [
        (vec!["deploy"], "deploy needs --max-hourly-usd"),
        (
            vec!["worker-update", "--check", "--release", "file.json"],
            "cannot combine --check with --release",
        ),
        (vec!["remove-api-key"], "remove-api-key needs --provider"),
        (
            vec!["offer", "--discount-pct", "150"],
            "between 0 and 99.90",
        ),
        (
            vec!["offer", "--model", "fixture/video"],
            "offer needs --discount-pct",
        ),
        (
            vec!["start", "--max-hourly-usd", "NaN"],
            "positive finite number",
        ),
        (
            vec!["deploy", "--max-hourly-usd", "0"],
            "positive finite number",
        ),
        (
            vec!["deploy", "--max-hourly-usd", "--json"],
            "needs a value",
        ),
    ] {
        let out = run(&words, &state);
        assert_eq!(out.status.code(), Some(2));
        assert!(output(&out).contains(expected), "{}", output(&out));
        assert!(!output(&out).contains("profile"));
        assert!(!state.exists());
    }
}

#[test]
fn unknown_commands_get_a_suggestion_without_echoing_secret_options() {
    let dir = common::tempdir();
    let out = run(&["deply"], dir.path());
    assert!(output(&out).contains("Unknown command 'deply'"));
    assert!(output(&out).contains("Did you mean everycli deploy?"));
    for words in [
        vec!["status", "--secret-DO-NOT-ECHO"],
        vec!["sk-SECRET-DO-NOT-ECHO"],
    ] {
        let out = run(&words, dir.path());
        assert!(!output(&out).contains("DO-NOT-ECHO"));
    }
}

#[test]
fn missing_hotkey_identifies_the_file_and_never_suggests_diagnosing_a_vm() {
    let dir = common::tempdir();
    let path = dir.path().join("missing-hotkey");
    let out = run(
        &[
            "register-hotkey",
            "--hotkey-file",
            path.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "file_not_found");
    assert_eq!(v["file"], path.to_str().unwrap());
    assert!(!output(&out).contains("everycli doctor"));
    assert!(!output(&out).contains("everycli status"));
}

#[test]
fn no_profile_has_one_message_code_and_exit_status_across_commands() {
    let dir = common::tempdir();
    let state = dir.path().join("fresh");
    let mut baseline = Value::Null;
    let start = Instant::now();
    for command in [
        "status",
        "doctor",
        "health",
        "balances",
        "earnings",
        "activate",
        "resume",
        "reconcile",
    ] {
        let out = run(&[command, "--json"], &state);
        assert_eq!(out.status.code(), Some(2), "{command}");
        let mut value: Value = serde_json::from_slice(&out.stdout).unwrap();
        value.as_object_mut().unwrap().remove("checks");
        if baseline.is_null() {
            baseline = value.clone();
        }
        assert_eq!(value, baseline, "{command}");
        assert!(out.stderr.is_empty());
        assert!(!output(&out).contains("uncertain operations"));
    }
    assert!(
        start.elapsed().as_secs() < 5,
        "Fresh profile checks must stay local"
    );
}

#[test]
fn providers_are_a_table_with_unknown_permissions_before_init() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("fresh"),
    };
    state
        .write("credentials", &json!({"FAL_KEY":"synthetic-do-not-show"}))
        .unwrap();
    let out = run(&["providers"], &state.directory);
    let text = output(&out);
    assert!(out.status.success());
    assert!(text.contains("PROVIDER") && text.contains("REASON") && text.contains("Unknown"));
    assert!(!text.contains("releaseAllows") && !text.contains("synthetic-do-not-show"));
    let out = run(&["providers", "--json"], &state.directory);
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["releaseAllows"].is_null())
    );
    let known = invitation::provider_summary(
        &json!({"request":{"compose_file":{"allowed_envs":["FAL_KEY"]}}}),
        &json!({"FAL_KEY":"synthetic-key"}),
    );
    assert_eq!(known[0]["releaseAllows"], true);
    assert_eq!(known[1]["releaseAllows"], false);
    assert!(
        known[1]["reason"]
            .as_str()
            .unwrap()
            .contains("Not included")
    );
}

#[test]
fn noninteractive_removal_requires_yes_and_preserves_the_key() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("profile"),
    };
    state
        .write("credentials", &json!({"FAL_KEY":"synthetic-private-key"}))
        .unwrap();
    let out = run(&["remove-api-key", "--provider", "fal"], &state.directory);
    assert_eq!(out.status.code(), Some(2));
    assert!(output(&out).contains("add --yes"));
    assert!(!output(&out).contains("Continue?"));
    assert_eq!(
        state.read("credentials", false).unwrap()["FAL_KEY"],
        "synthetic-private-key"
    );
    let out = run(
        &["remove-api-key", "--provider", "fal", "--yes", "--json"],
        &state.directory,
    );
    assert!(out.status.success());
    assert!(state.read("credentials", false).unwrap()["FAL_KEY"].is_null());
}

#[test]
fn registration_error_uses_the_actual_network_and_never_assumes_network_failure_is_absence() {
    assert!(
        cli_errors::registration_command(Some(&args(&["init", "--wallet", "wallet"])))
            .contains("--wallet.hotkey default")
    );
    for (network, subnet, chain) in [("mainnet", "SN117", "finney"), ("testnet", "SN566", "test")] {
        let a = args(&[
            "init",
            "--network",
            network,
            "--wallet",
            "wallet",
            "--hotkey",
            "default",
        ]);
        let d = cli_errors::diagnostic(&Error("hotkey_not_registered"), Some(&a));
        assert!(d["message"].as_str().unwrap().contains(subnet));
        assert!(
            d["registrationCommand"]
                .as_str()
                .unwrap()
                .contains(&format!("--subtensor.network {chain}"))
        );
        assert!(
            d["registrationCommand"]
                .as_str()
                .unwrap()
                .contains("--wallet.name wallet --wallet.hotkey default")
        );
        let d = cli_errors::diagnostic(&Error("request_timeout"), Some(&a));
        assert!(!d["message"].as_str().unwrap().contains("not registered"));
    }
}

#[test]
fn setup_runs_in_order_and_never_auto_activates_or_resumes() {
    let dir = common::tempdir();
    let state = State {
        directory: dir.path().join("profile"),
    };
    let a = args(&[
        "setup",
        "--wallet",
        "wallet",
        "--provider",
        "fal",
        "--max-hourly-usd",
        "0.05",
    ]);
    let mut seen = vec![];
    let out = cli::setup_with(
        &a,
        &state,
        &|_| panic!("no visible prompts needed"),
        &mut |a| {
            seen.push(format!(
                "{}:{}",
                a.command,
                a.get("--provider").unwrap_or("")
            ));
            assert!(!a.flag("--yes"));
            Ok(match a.command.as_str() {
                "register-hotkey" => json!({"registered":true}),
                "set-api-keys" | "init" => json!({}),
                "status" => {
                    json!({"coordinator":{"appId":"0000000000000000000000000000000000000000"}})
                }
                "deploy" => {
                    assert_eq!(a.get("--max-hourly-usd"), Some("0.05"));
                    json!({"appId":"synthetic","phase":"awaiting_attestation"})
                }
                other => panic!("unexpected paid operation: {other}"),
            })
        },
    )
    .unwrap();
    assert_eq!(
        seen,
        [
            "register-hotkey:",
            "set-api-keys:phala",
            "set-api-keys:fal",
            "init:",
            "status:",
            "deploy:"
        ]
    );
    assert_eq!(out["deploymentComplete"], true);
    assert_eq!(out["setupComplete"], false);
    assert_eq!(out["readinessVerified"], false);
    let text = display::render("setup", &out);
    assert!(text.contains("Finish miner setup — steps 5–11"));
    for step in [
        "5. Run everycli status",
        "6. If keys",
        "everycli activate",
        "everycli resume",
        "everycli offer --model",
        "11. Run everycli doctor",
    ] {
        assert!(text.contains(step), "{text}");
    }
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("everycli setup --continue")
    );
    assert_eq!(out["nextSteps"].as_array().unwrap().len(), 7);
}

#[test]
fn setup_stops_at_failures_cancellation_and_existing_workloads() {
    for failure in [
        "unregistered",
        "keys",
        "init",
        "status",
        "remote_workload",
        "cancel",
        "local_workload",
    ] {
        let dir = common::tempdir();
        let state = State {
            directory: dir.path().join("profile"),
        };
        if failure == "local_workload" {
            state
                .write("deployment", &json!({"phase":"provision_intent"}))
                .unwrap();
        }
        let a = args(&["setup", "--wallet", "wallet", "--max-hourly-usd", "0.05"]);
        let mut seen = vec![];
        let out = cli::setup_with(&a, &state, &|_| panic!("unexpected prompt"), &mut |a| {
            seen.push(a.command.clone());
            match a.command.as_str() {
                "register-hotkey" => Ok(json!({"registered": failure != "unregistered"})),
                "set-api-keys" if failure == "keys" => Err(Error("interrupted")),
                "init" if failure == "init" => Err(Error("request_timeout")),
                "status" => Ok(if failure == "status" {
                    json!({"coordinatorError":{"code":"request_timeout"}})
                } else {
                    json!({"coordinator":{"appId": if failure == "remote_workload" { "existing-worker" } else { "0000000000000000000000000000000000000000" }}})
                }),
                "deploy" => Err(Error("cancelled")),
                _ => Ok(json!({})),
            }
        });
        if failure == "unregistered" {
            let out = out.unwrap();
            assert_eq!(out["error"], "hotkey_not_registered");
            assert!(
                out["next"]
                    .as_str()
                    .unwrap()
                    .contains("rerun everycli setup")
            );
        } else {
            assert!(out.is_err(), "{failure}");
        }
        assert_eq!(
            seen.iter().filter(|c| *c == "deploy").count(),
            usize::from(failure == "cancel")
        );
        if failure == "local_workload" {
            assert!(seen.is_empty());
        }
    }
    let dir = common::tempdir();
    let out = run(&["setup", "--yes"], dir.path());
    assert!(!out.status.success());
    assert!(output(&out).contains("interactive terminal"));
}
