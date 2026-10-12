mod common;
use everyframe_miner::{cli, display, health, now};
use serde_json::{Value, json};
use std::process::Command;

fn snapshot() -> Value {
    json!({"online":true,"attested":true,"lastSeen":now()-1000,"sessionExpires":now()+60000,
        "offers":[{"model":"fixture/video","offered":true,"pricingCurrent":true}],
        "modelCatalog":[{"model":"fixture/video","offerable":true,"workerAvailable":true,"blockers":[]}],
        "activeJobs":0,"jobs":[{"state":"accepted","count":4},{"state":"failed","count":1}]})
}

#[test]
fn health_parsing_help_and_no_profile_preserve_json_and_exit_codes() {
    let args = cli::parse(&["health".into(), "--json".into()]).unwrap();
    assert_eq!(args.command, "health");
    let dir = common::tempdir();
    for command in ["health", "status"] {
        for json_mode in [false, true] {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_everycli"));
            cmd.args([command, "--state-dir"])
                .arg(dir.path().join("profile"));
            if json_mode {
                cmd.arg("--json");
            }
            let out = cmd.output().unwrap();
            assert_eq!(out.status.code(), Some(2));
            assert!(!out.stdout.contains(&0x1b));
            assert!(out.stderr.is_empty());
            if json_mode {
                let v: Value = serde_json::from_slice(&out.stdout).unwrap();
                assert_eq!(v["ok"], false);
                assert_eq!(v["error"], "profile_not_initialized");
            } else {
                assert!(String::from_utf8_lossy(&out.stdout).contains("No local miner profile"));
                assert!(serde_json::from_slice::<Value>(&out.stdout).is_err());
            }
        }
    }
}

#[test]
fn readiness_requires_fresh_session_heartbeat_and_active_model_evidence() {
    let v = snapshot();
    let at = now();
    assert!(
        health::runtime_checks(&v, at)
            .iter()
            .all(|c| c["ok"] == true)
    );
    for (pointer, bad) in [
        ("/online", json!(false)),
        ("/lastSeen", json!(at - 30001)),
        ("/lastSeen", json!(at + 30001)),
        ("/sessionExpires", json!(at - 1)),
        ("/attested", json!(false)),
        ("/offers/0/pricingCurrent", json!(false)),
        ("/modelCatalog/0/workerAvailable", json!(false)),
        ("/modelCatalog/0/offerable", json!(false)),
        (
            "/modelCatalog/0/blockers",
            json!(["worker_provider_required"]),
        ),
    ] {
        let mut bad_snapshot = v.clone();
        *bad_snapshot.pointer_mut(pointer).unwrap() = bad;
        assert!(
            health::runtime_checks(&bad_snapshot, at)
                .iter()
                .any(|c| c["ok"] == false),
            "{pointer}"
        );
    }
    for field in ["lastSeen", "sessionExpires", "modelCatalog", "offers"] {
        let mut incomplete = v.clone();
        incomplete.as_object_mut().unwrap().remove(field);
        let checks = health::runtime_checks(&incomplete, at);
        assert!(checks.iter().any(|c| c["state"] == "WARN"), "{field}");
    }
    assert!(
        health::runtime_checks(&Value::Null, at)
            .iter()
            .all(|c| c["ok"] == false)
    );
}

#[test]
fn unused_catalog_models_do_not_fail_health_but_active_unsupported_models_do() {
    let mut v = snapshot();
    v["modelCatalog"]
        .as_array_mut()
        .unwrap()
        .push(json!({"model":"future/video","offerable":false,"workerAvailable":false}));
    assert!(
        health::runtime_checks(&v, now())
            .iter()
            .all(|c| c["ok"] == true)
    );
    v["offers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"model":"future/video","offered":true,"pricingCurrent":true}));
    assert!(
        health::runtime_checks(&v, now())
            .iter()
            .any(|c| c["state"] == "FAIL")
    );
}

#[test]
fn idle_and_historical_failures_are_not_current_readiness_failures() {
    let v = snapshot();
    let activity = health::activity(&v);
    assert_eq!(activity["acceptedJobs"], 4);
    assert_eq!(activity["activeJobs"], 0);
    let checks = health::runtime_checks(&v, now());
    let report = json!({"ok":true,"checks":checks,"activity":activity});
    let plain = display::render("health", &report);
    assert!(plain.contains("All readiness checks passed"));
    assert!(plain.contains("Historical acceptance is not a live test"));
    assert!(plain.contains("failed"));
    assert!(plain.contains("normal when idle"));
    assert!(!plain.contains("[FAIL]"));
    assert!(!plain.contains('\x1b'));
    let colored = display::render_with_color("health", &report, true);
    assert!(colored.contains("\x1b[32m[PASS]\x1b[0m"));
    assert_eq!(display::render_with_color("health", &report, false), plain);
    assert!(health::activity(&Value::Null)["acceptedJobs"].is_null());
}

#[test]
fn status_tables_preserve_structured_value_and_show_errors_safely() {
    let remote = snapshot();
    let mut report = json!({"minerId":"test-miner","network":"mainnet","netuid":117,"release":"test-release",
        "phase":"awaiting_readmission","invitationExpired":false,"appId":"1".repeat(40),
        "cloud":{"status":"running"},"coordinator":remote});
    report["coordinator"]["enabled"] = json!(true);
    report["coordinator"]["draining"] = json!(false);
    report["coordinator"]["routingEnabled"] = json!(true);
    let before = report.clone();
    let plain = display::render("status", &report);
    assert_eq!(report, before);
    for label in [
        "Miner status",
        "Local phase",
        "awaiting_readmission",
        "Routing enabled",
        "MODEL OFFER",
        "Historically accepted jobs",
        "everycli health",
    ] {
        assert!(plain.contains(label), "{label}");
    }
    assert!(!plain.contains("All readiness checks passed"));
    assert!(serde_json::from_str::<Value>(&plain).is_err());
    report["coordinator"] = Value::Null;
    report["cloud"] = Value::Null;
    report["coordinatorError"] =
        json!({"code":"request_timeout", "message":"NEVER-PRINT-RAW-SECRET"});
    report["cloudError"] = json!({"code":"cloud_unauthorized", "message":"NEVER-PRINT-RAW-SECRET"});
    let plain = display::render("status", &report);
    assert!(plain.contains("request_timeout"));
    assert!(plain.contains("cloud_unauthorized"));
    assert!(!plain.contains("NEVER-PRINT-RAW-SECRET"));
    assert!(plain.contains("[WARN]"));
    assert!(plain.contains("Not reported"));
}

#[test]
fn table_text_is_control_safe_and_wrapped() {
    let report = json!({"ok":false,"checks":[{"name":"Model: /some/very/long/model/identifier/that/does/not/fit", "ok":false, "detail":"\u{1b}[31m Do not redeploy. ".repeat(15)}]});
    let plain = display::render("health", &report);
    assert!(!plain.contains('\x1b'));
    // Fixed explanatory footer is separate; the table itself fits 108 columns.
    for line in plain
        .lines()
        .skip_while(|l| !l.starts_with("CHECK"))
        .take_while(|l| !l.is_empty())
    {
        assert!(line.chars().count() <= 108, "{line}");
    }
    assert!(plain.contains("[FAIL]"));
    assert!(!plain.contains("All readiness checks passed"));
}
