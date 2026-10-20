use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
mod common;

use everyframe_miner::{
    Error, Result, cloud, invitation,
    miner::Miner,
    network::{Http, Request},
    now,
    protocol::{self, Keys},
    state::State,
};
use serde_json::{Value, json};
use std::{cell::RefCell, path::PathBuf};
use x25519_dalek::{PublicKey, StaticSecret};

#[test]
fn doctor_without_local_deployment_explains_setup_without_calling_phala() {
    // Cover both a newly enrolled miner and a fresh device viewing a remote worker.
    for remote_app in ["0".repeat(40), "1".repeat(40)] {
        let r = Rig::new();
        r.status.borrow_mut()["appId"] = json!(remote_app);
        let out = r.miner().doctor().unwrap();
        assert_eq!(out["ok"], false);
        let check = out["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Phala workload")
            .unwrap();
        assert_eq!(check["ok"], false);
        let detail = check["detail"].as_str().unwrap();
        assert!(detail.contains("No workload deployed in this local profile"));
        assert!(detail.contains("everycli deploy --max-hourly-usd <limit>"));
        assert!(detail.contains("not a total budget"));
        assert!(detail.contains("original management profile; do not redeploy"));
        assert!(!out.to_string().contains("invalid_string"));
        assert_eq!(r.calls.borrow().len(), 1);
        assert!(r.calls.borrow()[0].0.starts_with("GET "));
        assert!(r.calls.borrow()[0].0.ends_with("/v1/miner/status"));
        assert!(r.state().read("deployment", true).unwrap().is_null());
    }
}

#[test]
fn doctor_incomplete_or_invalid_deployment_does_not_recommend_new_vm() {
    for app in [Value::Null, json!(123), json!("bad-app-id"), json!("")] {
        let r = Rig::new();
        let deployment = json!({"phase":"provision_intent", "appId":app});
        r.state().write("deployment", &deployment).unwrap();
        let out = r.miner().doctor().unwrap();
        assert_eq!(out["ok"], false);
        let check = out["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Phala workload")
            .unwrap();
        let detail = check["detail"].as_str().unwrap();
        assert!(detail.contains("Do not redeploy"));
        if app.is_null() {
            assert!(detail.contains("everycli reconcile"));
        } else {
            assert!(detail.contains("app ID is invalid"));
        }
        assert!(!detail.contains("everycli deploy"));
        assert!(!out.to_string().contains("invalid_string"));
        assert_eq!(r.calls.borrow().len(), 1);
        assert_eq!(r.state().read("deployment", true).unwrap(), deployment);
    }
}

#[test]
fn doctor_still_checks_existing_cloud_workload() {
    let r = Rig::new();
    r.deploy().unwrap();
    r.calls.borrow_mut().clear();
    let out = r.miner().doctor().unwrap();
    let check = out["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Phala workload")
        .unwrap();
    assert_eq!(check["ok"], true);
    assert!(
        r.calls
            .borrow()
            .iter()
            .any(|(path, _)| path.contains("/cvms/"))
    );
}

#[test]
fn balance_publish_then_auth_only_read_never_mutates_workers_or_exports_keys() {
    let r = Rig::new();
    let out = r.miner().balances(true).unwrap();
    assert_eq!(out["published"], true);
    assert_eq!(out["balances"][0]["remainingUsd"], 12.345678);
    assert!(!out.to_string().contains("synthetic"));
    assert!(r.envs.borrow().is_empty());
    let amounts = r.status.borrow()["balances"].clone();
    assert_eq!(amounts[0]["remainingMicrousd"], 12345678);
    r.state()
        .write(
            "credentials",
            &json!({"MINER_TOKEN":r.credentials["MINER_TOKEN"]}),
        )
        .unwrap();
    r.calls.borrow_mut().clear();
    let remote = r.miner().balances(false).unwrap();
    assert_eq!(remote["balances"][0]["remainingUsd"], 12.345678);
    assert_eq!(remote["balances"][0]["source"], "synced");
    assert_eq!(r.calls.borrow().len(), 1);
    assert!(r.calls.borrow()[0].0.ends_with("/v1/miner/status"));
    assert_eq!(
        r.miner().balances(true).unwrap_err().0,
        "no_local_api_keys_to_check"
    );
    assert_eq!(r.status.borrow()["balances"], amounts);
}

#[test]
fn balance_failure_preserves_local_results_and_billing_keys_never_enter_worker_env() {
    let r = Rig::new();
    *r.fail.borrow_mut() = "/status".into();
    let out = r.miner().balances(false).unwrap();
    assert_eq!(out["remoteUnavailable"], true);
    assert_eq!(out["remoteError"]["code"], "request_failed");
    assert_eq!(out["balances"][0]["remainingUsd"], 12.345678);
    let mut creds = r.credentials.clone();
    creds["FAL_ADMIN_KEY"] = json!("synthetic-admin-key");
    creds["OPENROUTER_MANAGEMENT_KEY"] = json!("synthetic-management-key");
    let env = cloud::environment(&r.inv, &creds, true)
        .unwrap()
        .to_string();
    assert!(!env.contains("ADMIN"));
    assert!(!env.contains("MANAGEMENT"));
    assert!(!env.contains("synthetic-admin-key"));
    assert!(!env.contains("synthetic-management-key"));
    assert!(!env.contains("PHALA_CLOUD_API_KEY"));
}

#[test]
fn failed_balance_publish_keeps_local_results_and_reports_failure() {
    let r = Rig::new();
    *r.fail.borrow_mut() = "/balances".into();
    let out = r.miner().balances(true).unwrap();
    assert_eq!(out["balances"][0]["remainingUsd"], 12.345678);
    assert_eq!(out["published"], false);
    assert_eq!(out["publishRequested"], true);
    assert_eq!(out["remoteUnavailable"], true);
    assert_eq!(out["remoteError"]["code"], "request_failed");
    assert!(everyframe_miner::cli::balance_check_failed(&out));
    assert!(!out.to_string().contains("synthetic"));
    assert!(r.envs.borrow().is_empty());
}

fn decrypt(cipher: &str, secret: &StaticSecret) -> Value {
    let b = hex::decode(cipher).unwrap();
    let peer: [u8; 32] = b[..32].try_into().unwrap();
    let shared = secret.diffie_hellman(&PublicKey::from(peer));
    let clear = Aes256Gcm::new_from_slice(shared.as_bytes())
        .unwrap()
        .decrypt(Nonce::from_slice(&b[32..44]), &b[44..])
        .unwrap();
    serde_json::from_slice(&clear).unwrap()
}
struct Rig {
    dir: tempfile::TempDir,
    key: Keys,
    trust: Value,
    inv: Value,
    credentials: Value,
    status: RefCell<Value>,
    info: RefCell<Value>,
    fail: RefCell<String>,
    calls: RefCell<Vec<(String, String)>>,
    envs: RefCell<Vec<Value>>,
    cvms: RefCell<Vec<Value>>,
    recovered: RefCell<Option<Value>>,
}

fn abandoned_preparation() -> Rig {
    let (mut r, _) = Rig::reconnect_fixture();
    r.inv["keyVersion"] = json!(1);
    r.fail.borrow_mut().clear();
    let mut status = r.status.borrow_mut();
    status["appId"] = json!("0".repeat(40));
    status["online"] = json!(false);
    status["attested"] = json!(false);
    drop(status);
    r.state()
        .write(
            "deployment",
            &json!({"phase":"provision_intent","appId":null,"startedAt":123}),
        )
        .unwrap();
    r.calls.borrow_mut().clear();
    r
}

#[test]
fn abandoned_preparation_is_archived_without_cloud_mutation_or_losing_keys() {
    let r = abandoned_preparation();
    let before = r.state().read("deployment", false).unwrap();
    let keys = r.state().read("credentials", false).unwrap();
    let config = r.state().read("config", false).unwrap();
    let registration = r.state().read("registration", false).unwrap();
    let out = r.miner().reconcile().unwrap();
    assert_eq!(out["resolved"], true);
    assert_eq!(out["phase"], "not_deployed");
    assert_eq!(out["cloudChanged"], false);
    assert_eq!(
        r.state().read("deployment-recovery", false).unwrap()[0]["deployment"],
        before
    );
    assert_eq!(r.state().read("deployment", false).unwrap(), json!({}));
    assert_eq!(r.state().read("credentials", false).unwrap(), keys);
    assert_eq!(r.state().read("config", false).unwrap(), config);
    assert_eq!(r.state().read("registration", false).unwrap(), registration);
    assert_eq!(r.miner().reconcile().unwrap()["resolved"], true);
    assert!(
        r.calls
            .borrow()
            .iter()
            .all(|(method, _)| method.starts_with("GET "))
    );
    // Simulate interruption between archive and clearing: one archive, not two.
    r.state().write("deployment", &before).unwrap();
    r.miner().reconcile().unwrap();
    assert_eq!(
        r.state()
            .read("deployment-recovery", false)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn preparation_recovery_refuses_ambiguity_and_outages_without_changing_journal() {
    for case in 0..10 {
        let r = abandoned_preparation();
        match case {
            0 => r.status.borrow_mut()["appId"] = json!("1".repeat(40)),
            1 => r.status.borrow_mut()["activeJobs"] = json!(1),
            2 => r.status.borrow_mut()["online"] = json!(true),
            3 => r.status.borrow_mut()["attested"] = json!(true),
            4 => r.cvms.borrow_mut().push(r.info.borrow().clone()),
            5 => *r.fail.borrow_mut() = "/status".into(),
            6 => *r.fail.borrow_mut() = "/cvms/paginated".into(),
            7 => r.cvms.borrow_mut().push(json!({"unexpected":"schema"})),
            8 => r
                .state()
                .write("worker-update", &json!({"stage":"applying"}))
                .unwrap(),
            _ => r
                .state()
                .write("deployment", &json!({"phase":"commit_intent","appId":null}))
                .unwrap(),
        }
        let before = r.state().read("deployment", false).unwrap();
        assert!(r.miner().reconcile().is_err(), "case {case}");
        assert_eq!(r.state().read("deployment", false).unwrap(), before);
        assert!(
            r.state()
                .read("deployment-recovery", true)
                .unwrap()
                .is_null()
        );
        assert!(
            r.calls
                .borrow()
                .iter()
                .all(|(method, _)| method.starts_with("GET "))
        );
    }
}

#[test]
fn reconnect_keeps_keys_label_deleted_journal_and_records_verified_uid_zero() {
    let (r, _) = Rig::reconnect_fixture();
    let before = r.state().read("deployment", false).unwrap();
    let out = r.miner().reconnect(None, &|_| Ok(())).unwrap();
    assert_eq!(out["state"], "reconnected");
    assert_eq!(out["uid"], 0);
    assert_eq!(out["workerChanged"], false);
    assert_eq!(r.state().read("deployment", false).unwrap(), before);
    assert_eq!(r.state().read("label", false).unwrap()["name"], "keep-me");
    let keys = r.state().read("credentials", false).unwrap();
    assert_eq!(keys["FAL_KEY"], r.credentials["FAL_KEY"]);
    assert_eq!(
        keys["PHALA_CLOUD_API_KEY"],
        r.credentials["PHALA_CLOUD_API_KEY"]
    );
    assert_eq!(
        keys["CONSOLE_AUTH"]["certificate"]["value"]["keyVersion"],
        7
    );
    assert_eq!(
        keys["WORKER_AUTH"]["certificate"]["value"]["kind"],
        "everyframe-hotkey-delegation-v2"
    );
    assert!(r.state().read("profile-recovery", true).unwrap().is_null());
    assert!(
        r.calls
            .borrow()
            .iter()
            .all(|(p, _)| p.starts_with("GET ") || p == "POST /" || p.contains("/onboarding/"))
    );
    assert!(!out.to_string().contains("synthetic"));
}

#[test]
fn missing_local_journal_recovery_requires_explicit_owner_consent_and_never_mutates_cloud() {
    for mode in [
        "recover",
        "decline",
        "automatic",
        "online",
        "existing",
        "duplicate",
    ] {
        let (r, _) = Rig::reconnect_fixture();
        r.state().write("deployment", &Value::Null).unwrap();
        r.status.borrow_mut()["online"] = json!(mode == "online");
        if mode == "existing" {
            *r.fail.borrow_mut() = String::new();
        }
        if mode == "duplicate" {
            r.cvms.borrow_mut().push(r.info.borrow().clone());
        }
        let before = r.state().read("config", false).unwrap();
        let prompts = RefCell::new(Vec::new());
        let result = if mode == "automatic" {
            r.miner().reconnect_authorization(None)
        } else {
            r.miner().reconnect(None, &|q| {
                prompts.borrow_mut().push(q.to_string());
                if mode == "decline" && q.starts_with("Retire ") {
                    Err(Error("declined"))
                } else {
                    Ok(())
                }
            })
        };
        if mode == "recover" {
            let out = result.unwrap();
            assert_eq!(out["bindingRetired"], true);
            assert_eq!(
                r.state().read("config", false).unwrap()["invitation"]["value"]["keyVersion"],
                8
            );
            assert_eq!(
                r.state().read("credentials", false).unwrap()["FAL_KEY"],
                r.credentials["FAL_KEY"]
            );
            assert_eq!(prompts.borrow().len(), 2);
            assert!(prompts.borrow()[1].contains("wrong-workspace"));
            assert!(r.state().read("deployment", true).unwrap().is_null());
        } else {
            assert!(result.is_err(), "{mode}");
            assert_eq!(r.state().read("config", false).unwrap(), before, "{mode}");
            assert!(r.recovered.borrow().is_none(), "{mode}");
        }
        assert!(
            r.calls
                .borrow()
                .iter()
                .all(|(p, _)| !p.contains("/cvms") || p.starts_with("GET "))
        );
    }
}

#[test]
fn reviewed_production_os_versions_are_not_hardcoded_but_dev_gpu_and_tampering_are_rejected() {
    let r = Rig::new();
    for (image, valid) in [
        ("dstack-0.5.12", true),
        ("dstack-0.6.1", true),
        ("dstack-dev-0.5.12", false),
        ("dstack-nvidia-0.5.12", false),
        ("https://evil.invalid/image", false),
    ] {
        let mut inv = r.inv.clone();
        inv["request"]["image"] = json!(image);
        assert_eq!(
            invitation::validate(&r.sign(&inv), &r.trust, now(), false).is_ok(),
            valid,
            "{image}"
        );
        let mut unsigned = r.sign(&r.inv);
        unsigned["value"]["request"]["image"] = json!(image);
        assert!(invitation::validate(&unsigned, &r.trust, now(), false).is_err());
    }
}

#[test]
fn deploy_refreshes_attached_authorization_once_before_paid_confirmation() {
    let (r, _) = Rig::reconnect_fixture();
    r.status.borrow_mut()["replacementSupported"] = json!(true);
    let before = r.state().read("deployment", false).unwrap();
    let prompts = RefCell::new(Vec::new());
    let result = r.miner().deploy(10.0, &|q| {
        prompts.borrow_mut().push(q.to_string());
        Err(Error("declined"))
    });
    assert_eq!(result.unwrap_err().0, "declined");
    assert_eq!(prompts.borrow().len(), 1);
    assert!(prompts.borrow()[0].starts_with("Create "));
    assert_eq!(
        r.state().read("credentials", false).unwrap()["CONSOLE_AUTH"]["certificate"]["value"]["keyVersion"],
        7
    );
    assert_eq!(r.state().read("deployment", false).unwrap(), before);
    let calls = r.calls.borrow();
    assert_eq!(
        calls
            .iter()
            .filter(|(p, _)| p.ends_with("/onboarding/enroll"))
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|(p, _)| p.starts_with("POST /api/v1/cvms"))
    );
    assert!(
        calls
            .iter()
            .all(|(p, _)| p.starts_with("GET ") || p == "POST /" || p.contains("/onboarding/"))
    );
}

#[test]
fn deploy_failed_recovery_never_provisions_or_retries_forever() {
    for mode in ["auth", "wrong-signer", "timeout"] {
        let (r, path) = Rig::reconnect_fixture();
        r.status.borrow_mut()["replacementSupported"] = json!(true);
        if mode == "auth" {
            *r.fail.borrow_mut() = "auth".into();
        }
        if mode == "timeout" {
            *r.fail.borrow_mut() = "/status".into();
        }
        if mode == "wrong-signer" {
            let other = "5HQfvwpHm2njGiF716YwjdXgeeZoEiyFLZFvFAriZaiHNKc3";
            std::fs::write(path, json!({"ss58Address":other}).to_string()).unwrap();
        }
        let before = r.state().read("credentials", false).unwrap();
        assert!(
            r.miner()
                .deploy(10.0, &|_| panic!("must not request payment"))
                .is_err()
        );
        assert_eq!(r.state().read("credentials", false).unwrap(), before);
        let calls = r.calls.borrow();
        assert!(
            calls
                .iter()
                .filter(|(p, _)| p.ends_with("/onboarding/enroll"))
                .count()
                <= 1
        );
        assert!(
            calls
                .iter()
                .all(|(p, _)| p.starts_with("GET ") || p == "POST /" || p.contains("/onboarding/"))
        );
    }
}

#[test]
fn reconnect_accepts_initial_public_envelope_only_with_exact_live_binding() {
    for mismatch in ["none", "app", "compose", "name", "upgrade"] {
        let (mut r, _) = Rig::reconnect_fixture();
        r.inv["appId"] = Value::Null;
        *r.fail.borrow_mut() = String::new();
        let deployment = json!({"phase":"running","appId":"1".repeat(40)});
        r.state().write("deployment", &deployment).unwrap();
        match mismatch {
            "app" => r.status.borrow_mut()["appId"] = json!("2".repeat(40)),
            "compose" => r.info.borrow_mut()["compose_hash"] = json!("f".repeat(64)),
            "name" => r.info.borrow_mut()["name"] = json!("other-worker"),
            "upgrade" => r.inv["previousComposeHash"] = json!("b".repeat(64)),
            _ => {}
        }
        let result = r.miner().reconnect(None, &|_| Ok(()));
        assert_eq!(result.is_ok(), mismatch == "none", "{mismatch}");
        assert_eq!(r.state().read("deployment", false).unwrap(), deployment);
        assert!(
            r.calls
                .borrow()
                .iter()
                .all(|(p, _)| p.starts_with("GET ") || p == "POST /" || p.contains("/onboarding/"))
        );
    }
}

#[test]
fn reconnect_decline_wrong_wallet_pending_operation_and_wrong_vm_preserve_profile() {
    for mode in [
        "decline",
        "wallet",
        "pending",
        "update",
        "app",
        "exists",
        "auth",
        "cloud_failure",
    ] {
        let (r, _) = Rig::reconnect_fixture();
        match mode {
            "pending" => r
                .state()
                .write("deployment", &json!({"phase":"provision_intent"}))
                .unwrap(),
            "update" => r
                .state()
                .write("worker-update", &json!({"stage":"attesting"}))
                .unwrap(),
            "app" => r.status.borrow_mut()["appId"] = json!("2".repeat(40)),
            "exists" => *r.fail.borrow_mut() = "".into(),
            "auth" => *r.fail.borrow_mut() = "auth".into(),
            "cloud_failure" => *r.fail.borrow_mut() = format!("app_{}", "1".repeat(40)),
            _ => {}
        }
        let before = r.state().read("config", false).unwrap();
        let credentials = r.state().read("credentials", false).unwrap();
        let wrong = r.dir.path().join("missing-hotkey");
        let out = r
            .miner()
            .reconnect(if mode == "wallet" { Some(&wrong) } else { None }, &|_| {
                if mode == "decline" {
                    Err(Error("confirmation_required"))
                } else {
                    Ok(())
                }
            });
        assert!(out.is_err(), "{mode}");
        if mode == "auth" {
            assert_eq!(out.unwrap_err().0, "coordinator_auth_rejected");
        }
        assert_eq!(r.state().read("config", false).unwrap(), before, "{mode}");
        assert_eq!(
            r.state().read("credentials", false).unwrap(),
            credentials,
            "{mode}"
        );
        assert!(r.state().read("profile-recovery", true).unwrap().is_null());
        if ["decline", "wallet", "pending", "update"].contains(&mode) {
            assert!(r.calls.borrow().is_empty());
        }
    }
}

#[test]
fn reconnect_recovers_partial_file_commit_without_repeating_remote_requests() {
    for interruption in ["none", "credentials", "config", "registration", "changed"] {
        let (r, _) = Rig::reconnect_fixture();
        let mut before = json!({});
        for name in ["config", "credentials", "registration"] {
            before[name] = r.state().read(name, true).unwrap();
        }
        r.miner().reconnect(None, &|_| Ok(())).unwrap();
        let mut after = json!({});
        for name in ["config", "credentials", "registration"] {
            after[name] = r.state().read(name, true).unwrap();
        }
        for name in ["config", "credentials", "registration"] {
            r.state().write(name, &before[name]).unwrap();
        }
        r.state().write("profile-recovery", &json!({"version":1,"deployment":r.state().read("deployment", true).unwrap(),"before":before,"after":after})).unwrap();
        if interruption != "none" && interruption != "changed" {
            r.state().write(interruption, &after[interruption]).unwrap();
        }
        if interruption == "changed" {
            let mut changed = before["credentials"].clone();
            changed["FAL_KEY"] = json!("changed-by-user");
            r.state().write("credentials", &changed).unwrap();
        }
        r.calls.borrow_mut().clear();
        assert_eq!(
            r.miner().load(false).err().unwrap().0,
            "profile_recovery_pending"
        );
        let result = r.miner().reconnect(None, &|_| Ok(()));
        if interruption == "changed" {
            assert_eq!(result.unwrap_err().0, "reconnect_state_changed");
        } else {
            result.unwrap();
            for name in ["config", "credentials", "registration"] {
                assert_eq!(r.state().read(name, true).unwrap(), after[name]);
            }
            assert!(r.state().read("profile-recovery", true).unwrap().is_null());
        }
        assert!(r.calls.borrow().is_empty());
    }
}

#[test]
fn health_reads_one_signed_snapshot_and_cloud_without_changing_worker_or_profile() {
    let r = Rig::new();
    r.deploy().unwrap();
    {
        let mut status = r.status.borrow_mut();
        status["routingEnabled"] = json!(true);
        status["draining"] = json!(false);
        status["lastSeen"] = json!(now());
        status["sessionExpires"] = json!(now() + 60000);
        status["offers"][0]["offered"] = json!(true);
        status["modelCatalog"] = json!([{"model":"fixture/video","workerAvailable":true,"offerable":true,"blockers":[]}]);
        status["jobs"] = json!([]);
    }
    let before = r.state().read("deployment", false).unwrap();
    let credentials = r.state().read("credentials", false).unwrap();
    r.calls.borrow_mut().clear();
    r.envs.borrow_mut().clear();
    let report = r.miner().health().unwrap();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["generationSubmitted"], false);
    assert_eq!(report["activity"]["acceptedJobs"], 0);
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["ok"] == true)
    );
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p.ends_with("/v1/miner/status"))
            .count(),
        1
    );
    assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET ")));
    assert!(r.envs.borrow().is_empty());
    assert_eq!(r.state().read("deployment", false).unwrap(), before);
    assert_eq!(r.state().read("credentials", false).unwrap(), credentials);
    assert!(!report.to_string().contains("synthetic-provider-key"));
    for (key, bad) in [
        ("draining", json!(true)),
        ("online", json!(false)),
        ("enabled", json!(false)),
    ] {
        let old = r.status.borrow()[key].clone();
        r.status.borrow_mut()[key] = bad;
        assert_eq!(r.miner().health().unwrap()["ok"], false, "{key}");
        r.status.borrow_mut()[key] = old;
    }
    *r.fail.borrow_mut() = "/status".into();
    assert_eq!(r.miner().health().unwrap()["ok"], false);
}

#[test]
fn health_never_hides_an_optional_saved_provider_release_mismatch() {
    let r = Rig::new();
    let mut credentials = r.credentials.clone();
    credentials["VIVIX_API_KEY"] = json!("synthetic-vivix-not-in-release");
    r.state().write("credentials", &credentials).unwrap();
    let report = r.miner().health().unwrap();
    assert_eq!(report["ok"], false);
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Provider: vivix")
        .unwrap();
    assert_eq!(check["ok"], false);
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains("not a rejected API key")
    );
    assert!(
        !report
            .to_string()
            .contains("synthetic-vivix-not-in-release")
    );
}
impl Rig {
    fn reconnect_fixture() -> (Self, PathBuf) {
        use everyframe_miner::hotkey;
        use schnorrkel::{ExpansionMode, MiniSecretKey};
        use std::os::unix::fs::PermissionsExt;
        let mut r = Self::new();
        r.dir = common::tempdir();
        let seed = [7u8; 32];
        let pair = MiniSecretKey::from_bytes(&seed)
            .unwrap()
            .expand_to_keypair(ExpansionMode::Ed25519);
        let path = r.dir.path().join("hotkey");
        let address = hotkey::address(&pair.public.to_bytes());
        std::fs::write(
            &path,
            json!({"secretSeed":hex::encode(seed),"ss58Address":address}).to_string(),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        r.inv["kind"] = json!("everyframe-miner-deployment-v2");
        r.inv["authMode"] = json!("hotkey-v1");
        r.inv["publicOnboarding"] = json!(true);
        r.inv["keyVersion"] = json!(1);
        r.inv["hotkey"] = json!(address);
        r.inv.as_object_mut().unwrap().remove("tokenHash");
        r.inv["request"]["compose_file"]["allowed_envs"] =
            json!(["MINER_ID", "MINER_AUTH", "FAL_KEY"]);
        r.inv["composeHash"] =
            json!(cloud::compose_hash(&r.inv["request"]["compose_file"]).unwrap());
        r.inv["appId"] = json!("1".repeat(40));
        r.credentials.as_object_mut().unwrap().remove("MINER_TOKEN");
        r.credentials["CONSOLE_AUTH"] = hotkey::create(&path, &r.inv, "console").unwrap();
        r.credentials["WORKER_AUTH"] = hotkey::create(&path, &r.inv, "worker").unwrap();
        r.miner().init(&r.sign(&r.inv), &r.credentials).unwrap();
        r.state()
            .write(
                "deployment",
                &json!({"phase":"deleted","appId":"1".repeat(40),"deleteRequestedAt":123}),
            )
            .unwrap();
        r.state().write("registration", &json!({"registered":true,"hotkey":address,"network":"mainnet","hotkeyFile":path.to_str().unwrap(),"uid":127})).unwrap();
        r.state()
            .write("label", &json!({"name":"keep-me"}))
            .unwrap();
        // Coordinator now serves a newer signed release and key version.
        r.inv["keyVersion"] = json!(7);
        r.inv["hotkeyDelegationVersion"] = json!(2);
        r.inv["release"] = json!("test-recovery-release");
        r.status.borrow_mut()["composeHash"] = r.inv["composeHash"].clone();
        r.info.borrow_mut()["compose_hash"] = r.inv["composeHash"].clone();
        *r.fail.borrow_mut() = "gone".into();
        (r, path)
    }
    fn staged_update(&self, accepting: bool, activated: bool) -> Value {
        self.deploy().unwrap();
        let mut d = self.state().read("deployment", false).unwrap();
        d["phase"] = json!(if activated {
            "running"
        } else {
            "awaiting_attestation"
        });
        d["providerConfigured"] = json!(activated);
        self.state().write("deployment", &d).unwrap();
        let mut inv = self.inv.clone();
        inv["release"] = json!("test-release-2");
        inv["appId"] = self.info.borrow()["app_id"].clone();
        inv["previousComposeHash"] = inv["composeHash"].clone();
        let compose = &mut inv["request"]["compose_file"];
        compose["docker_compose_file"] = json!(
            compose["docker_compose_file"]
                .as_str()
                .unwrap()
                .replace(&"a".repeat(64), &"b".repeat(64))
        );
        inv["composeHash"] = json!(cloud::compose_hash(&inv["request"]["compose_file"]).unwrap());
        let envelope = self.sign(&inv);
        let mut status = self.status.borrow_mut();
        status["draining"] = json!(!accepting);
        status["routingEnabled"] = json!(true);
        status["update"] = json!({"state":"available","release":envelope});
        status["sessionExpires"] = json!(now() + 60000);
        self.calls.borrow_mut().clear();
        self.envs.borrow_mut().clear();
        envelope
    }
    fn advance_update(&self, stage: &str, envelope: &Value) -> bool {
        match stage {
            "draining" => self.status.borrow_mut()["activeJobs"] = json!(0),
            "applying" => {
                self.info.borrow_mut()["compose_hash"] = envelope["value"]["composeHash"].clone()
            }
            "attesting" | "readmitting" | "activating" => {
                let mut status = self.status.borrow_mut();
                status["composeHash"] = envelope["value"]["composeHash"].clone();
                status["attestation"]["at"] =
                    json!(now().max(status["attestation"]["at"].as_i64().unwrap()) + 2);
                status["sessionExpires"] = json!(now() + 60000);
            }
            _ => panic!("unexpected wait: {stage}"),
        }
        true
    }
    fn new() -> Self {
        let dir = common::tempdir();
        let key = Keys::default();
        let mut trust = invitation::trust("mainnet").unwrap();
        trust["publicKey"] = json!(key.signing_key());
        let credentials = json!({"MINER_TOKEN":"synthetic-miner-token-not-real-123456789","PHALA_CLOUD_API_KEY":"synthetic-phala-key","FAL_KEY":"synthetic-provider-key","DSTACK_DOCKER_PASSWORD":"stale-registry-key-never-send"});
        let compose = json!({"name":"everyframe-test-miner","docker_compose_file":format!("services:\n  worker:\n    image: ghcr.io/everyframebuilder/worker@sha256:{}\n","a".repeat(64)),"pre_launch_script":invitation::PUBLIC_SCRIPT,"allowed_envs":["MINER_ID","MINER_TOKEN","FAL_KEY"],"kms_enabled":true,"gateway_enabled":false,"public_logs":false,"public_sysinfo":false,"public_tcbinfo":true});
        let inv = json!({"network":trust["network"],"netuid":trust["netuid"],"coordinatorUrl":trust["coordinatorUrl"],"genesis":trust["genesis"],"kind":"everyframe-miner-invitation-v1","issuedAt":now()-1000,"expiresAt":now()+86400000,"minerId":"test-miner","tokenHash":protocol::sha(format!("Bearer {}",credentials["MINER_TOKEN"].as_str().unwrap())),"hotkey":"5C899MixYEUvgfLhKrCMiCUj4i2gZefTTj1d4fsWS2Bc2wP6","ownershipVerifiedByOperator":true,"release":"test-release-1","appId":null,"previousComposeHash":null,"osImageHash":"c".repeat(64),"composeHash":cloud::compose_hash(&compose).unwrap(),"request":{"name":compose["name"],"instance_type":"tdx.small","disk_size":20,"image":"dstack-0.5.9","kms":"PHALA","listed":false,"compose_file":compose}});
        let info = json!({"app_id":"1".repeat(40),"name":compose["name"],"compose_hash":inv["composeHash"],"status":"running"});
        let status = json!({"minerId":"test-miner","activeJobs":0,"draining":true,"enabled":true,"online":true,"attested":true,"attestation":{"state":"accepted","at":now()},"appId":info["app_id"],"composeHash":inv["composeHash"],"routingEnabled":false,"earnings":{"fees":[],"payoutsImplemented":false},"routingPolicy":"discount-lottery-v1","offers":[{"model":"fixture/video","revision":0,"pricingHash":"d".repeat(64),"baseMinerRewardMicrousd":123456,"discountBp":null,"offered":false,"pricingCurrent":true}]});
        let r = Self {
            dir,
            key,
            trust,
            inv,
            credentials,
            status: RefCell::new(status),
            info: RefCell::new(info),
            fail: RefCell::new(String::new()),
            calls: RefCell::default(),
            envs: RefCell::default(),
            cvms: RefCell::default(),
            recovered: RefCell::default(),
        };
        r.miner().init(&r.sign(&r.inv), &r.credentials).unwrap();
        r
    }
    fn path(&self) -> PathBuf {
        self.dir.path().join("profile")
    }
    fn state(&self) -> State {
        State {
            directory: self.path(),
        }
    }
    fn miner(&self) -> Miner<'_> {
        Miner {
            state: self.state(),
            trust: self.trust.clone(),
            http: self,
        }
    }
    fn sign(&self, v: &Value) -> Value {
        protocol::signed(v, &self.key.signing).unwrap()
    }
    fn deploy(&self) -> Result<Value> {
        self.miner().deploy(0.06, &|_| Ok(()))
    }
    fn phase(&self) -> String {
        self.state().read("deployment", true).unwrap()["phase"]
            .as_str()
            .unwrap_or("")
            .into()
    }
}
#[test]
fn automatic_worker_update_restores_accepting_paused_and_unactivated_states() {
    for (accepting, activated) in [(true, true), (false, true), (false, false)] {
        let r = Rig::new();
        let envelope = r.staged_update(accepting, activated);
        r.status.borrow_mut()["activeJobs"] = json!(2);
        let offers = r.status.borrow()["offers"].clone();
        let confirms = std::cell::Cell::new(0);
        let waits = std::cell::Cell::new(0);
        let out = r
            .miner()
            .worker_update_with_wait(
                false,
                &|_| {
                    confirms.set(confirms.get() + 1);
                    Ok(())
                },
                &|_| {},
                &|stage| {
                    waits.set(waits.get() + 1);
                    assert!(waits.get() < 10);
                    r.advance_update(stage, &envelope)
                },
            )
            .unwrap();
        assert_eq!(out["state"], "complete");
        assert_eq!(out["accepting"], accepting);
        assert_eq!(out["keysReactivated"], activated);
        assert_eq!(confirms.get(), 1);
        assert_eq!(r.status.borrow()["draining"], !accepting);
        assert_eq!(r.status.borrow()["offers"], offers);
        assert_eq!(
            r.state().read("worker-update", false).unwrap()["stage"],
            "complete"
        );
        assert_eq!(
            r.phase(),
            if accepting {
                "running"
            } else if activated {
                "drained"
            } else {
                "awaiting_attestation"
            }
        );
        let calls = r.calls.borrow();
        assert_eq!(
            calls
                .iter()
                .filter(|(p, _)| p.ends_with("/compose_file") && p.starts_with("PATCH"))
                .count(),
            1
        );
        assert_eq!(
            calls.iter().filter(|(p, _)| p.ends_with("/envs")).count(),
            usize::from(activated)
        );
        assert!(!calls.iter().any(|(p, _)| p.ends_with("/cvms")
            || p.ends_with("/cvms/provision")
            || p.ends_with("/offer")));
        assert!(!out.to_string().contains("synthetic"));
    }
}

#[test]
fn automatic_update_check_and_declined_confirmation_are_read_only() {
    let r = Rig::new();
    r.staged_update(true, true);
    let before = r.state().read("deployment", false).unwrap();
    let check = r
        .miner()
        .worker_update_with_wait(
            true,
            &|_| panic!("check must not confirm"),
            &|_| {},
            &|_| panic!(),
        )
        .unwrap();
    assert_eq!(check["state"], "available");
    assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET")));
    assert!(r.state().read("worker-update", true).unwrap().is_null());
    assert_eq!(
        r.miner()
            .worker_update_with_wait(
                false,
                &|_| Err(Error("confirmation_declined")),
                &|_| {},
                &|_| panic!()
            )
            .unwrap_err()
            .0,
        "confirmation_declined"
    );
    assert_eq!(r.state().read("deployment", false).unwrap(), before);
    assert_eq!(r.status.borrow()["draining"], false);
}

#[test]
fn automatic_update_waits_for_jobs_and_can_be_rerun_without_reapplying() {
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    r.status.borrow_mut()["activeJobs"] = json!(1);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false)
        .unwrap();
    assert_eq!(out["stage"], "draining");
    assert!(r.envs.borrow().is_empty());
    r.status.borrow_mut()["activeJobs"] = json!(0);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            if stage == "attesting" {
                false
            } else {
                r.advance_update(stage, &envelope)
            }
        })
        .unwrap();
    assert_eq!(out["stage"], "attesting");
    assert_eq!(r.envs.borrow().len(), 1); // Disabled keys only; old attestation cannot activate.
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p.starts_with("PATCH") && p.ends_with("/compose_file"))
            .count(),
        1
    );
}

#[test]
fn automatic_update_does_not_replay_ambiguous_cloud_mutations() {
    for failure in ["/compose_file/provision", "/compose_file", "/envs"] {
        let r = Rig::new();
        let envelope = r.staged_update(true, true);
        *r.fail.borrow_mut() = failure.into();
        let result = r
            .miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
                r.advance_update(stage, &envelope)
            });
        assert!(result.is_err());
        *r.fail.borrow_mut() = String::new();
        r.calls.borrow_mut().clear();
        let result = r
            .miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
                r.advance_update(stage, &envelope)
            });
        // A lost activation response is settled from observed state first:
        // without worker evidence it rolls back, waits for an attestation newer
        // than the lost request, and only then releases the keys once more.
        assert_eq!(result.unwrap()["state"], "complete", "{failure}");
        if failure == "/envs" {
            let calls = r.calls.borrow();
            let patches: Vec<_> = calls
                .iter()
                .filter(|(p, _)| p.starts_with("PATCH") && p.ends_with("/envs"))
                .collect();
            assert_eq!(patches.len(), 1);
            assert_eq!(patches[0].1, "activate_intent");
        }
        assert!(
            failure == "/compose_file/provision"
                || failure == "/envs"
                || !r
                    .calls
                    .borrow()
                    .iter()
                    .any(|(p, _)| !p.starts_with("GET") && p.ends_with(failure)),
            "{failure}"
        );
    }
}

#[test]
fn automatic_update_rejects_tampered_or_wrong_target_packages_before_draining() {
    for field in [
        "appId",
        "minerId",
        "previousComposeHash",
        "osImageHash",
        "expiresAt",
        "signature",
    ] {
        let r = Rig::new();
        let mut envelope = r.staged_update(true, true);
        if field == "signature" {
            envelope["signature"] = json!("bad");
        } else {
            envelope["value"][field] = match field {
                "expiresAt" => json!(now() - 1),
                "minerId" => json!("other"),
                "appId" => json!("2".repeat(40)),
                _ => json!("f".repeat(64)),
            };
            envelope = r.sign(&envelope["value"]);
        }
        r.status.borrow_mut()["update"]["release"] = envelope;
        assert!(
            r.miner()
                .worker_update_with_wait(
                    false,
                    &|_| panic!("invalid update must not confirm"),
                    &|_| {},
                    &|_| panic!()
                )
                .is_err(),
            "{field}"
        );
        assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET")));
        assert!(r.state().read("worker-update", true).unwrap().is_null());
    }
}

#[test]
fn legacy_provision_intent_recovers_only_an_unchanged_bound_worker() {
    for changed in [
        "none",
        "processing",
        "compose",
        "app",
        "signature",
        "release_target",
    ] {
        let r = Rig::new();
        let mut envelope = r.staged_update(false, true);
        if changed == "signature" {
            envelope["signature"] = json!("bad");
        }
        if changed == "release_target" {
            envelope["value"]["appId"] = json!("2".repeat(40));
            envelope = r.sign(&envelope["value"]);
        }
        let mut d = r.state().read("deployment", false).unwrap();
        d["phase"] = json!("update_provision_intent");
        d["pendingInvitation"] = envelope;
        r.state().write("deployment", &d).unwrap();
        match changed {
            "processing" => r.info.borrow_mut()["status"] = json!("processing"),
            "compose" => r.info.borrow_mut()["compose_hash"] = json!("f".repeat(64)),
            "app" => r.info.borrow_mut()["app_id"] = json!("2".repeat(40)),
            _ => {}
        }
        r.calls.borrow_mut().clear();
        let result = r.miner().reconcile();
        if changed == "none" {
            assert_eq!(result.unwrap()["resolved"], true);
            assert_eq!(r.phase(), "drained");
            assert_eq!(
                r.state().read("deployment", false).unwrap()["providerConfigured"],
                true
            );
        } else {
            assert!(result.is_err(), "{changed}");
            assert_eq!(r.state().read("deployment", false).unwrap(), d);
        }
        assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET")));
    }
}

#[test]
fn stop_can_safely_recover_provision_without_repeating_update() {
    let r = Rig::new();
    let envelope = r.staged_update(false, true);
    let mut d = r.state().read("deployment", false).unwrap();
    d["phase"] = json!("update_provision_intent");
    d["pendingInvitation"] = envelope;
    r.state().write("deployment", &d).unwrap();
    r.calls.borrow_mut().clear();
    assert_eq!(
        r.miner().stop(false, &|_| Ok(())).unwrap()["phase"],
        "stopping"
    );
    assert!(
        !r.calls
            .borrow()
            .iter()
            .any(|(p, _)| p.ends_with("/compose_file/provision") || p.ends_with("/compose_file"))
    );
}

#[test]
fn emergency_stop_survives_coordinator_outage_and_interrupted_update() {
    let r = Rig::new();
    let envelope = r.staged_update(false, true);
    let mut d = r.state().read("deployment", false).unwrap();
    d["phase"] = json!("update_commit_intent");
    d["pendingInvitation"] = envelope;
    r.state().write("deployment", &d).unwrap();
    *r.fail.borrow_mut() = "/drain".into();
    assert_eq!(
        r.miner().stop(false, &|_| Ok(())).unwrap_err().0,
        "coordinator_unavailable_use_stop_force"
    );
    assert_eq!(
        r.miner().stop_mode(false, true, &|_| Ok(())).unwrap()["phase"],
        "stopping"
    );
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p.ends_with("/shutdown"))
            .count(),
        1
    );
}

#[test]
fn labels_do_not_modify_signed_config_or_cloud() {
    let r = Rig::new();
    let before = r.state().read("config", false).unwrap();
    assert_eq!(
        r.miner().label(Some("studio-worker-1")).unwrap()["label"],
        "studio-worker-1"
    );
    assert_eq!(r.state().read("config", false).unwrap(), before);
    assert!(r.calls.borrow().is_empty());
    assert!(r.miner().label(Some("bad\nname")).is_err());
}

#[test]
fn deletion_requires_exact_app_confirmation_and_does_not_repeat_uncertain_requests() {
    let r = Rig::new();
    r.deploy().unwrap();
    r.calls.borrow_mut().clear();
    assert!(
        r.miner()
            .delete(&"2".repeat(40), &|_| panic!(
                "wrong app must fail before confirmation"
            ))
            .is_err()
    );
    assert!(r.calls.borrow().is_empty());
    assert!(
        r.miner()
            .delete(&"1".repeat(40), &|_| Err(Error("cancelled")))
            .is_err()
    );
    assert!(
        !r.calls
            .borrow()
            .iter()
            .any(|(p, _)| p.starts_with("DELETE"))
    );
    assert_eq!(
        r.miner().delete(&"1".repeat(40), &|_| Ok(())).unwrap()["phase"],
        "deleting"
    );
    r.miner()
        .delete(&"1".repeat(40), &|_| panic!("must not repeat deletion"))
        .unwrap();
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p.starts_with("DELETE"))
            .count(),
        1
    );
}

#[test]
fn workers_deployed_elsewhere_are_listed_deletable_and_replaceable() {
    let r = Rig::new();
    let other = "3".repeat(40);
    let own = r.info.borrow().clone();
    let vm = json!({"app_id":other,"name":r.inv["request"]["name"],"status":"running","compose_hash":"ff".repeat(32)});
    r.cvms.borrow_mut().push(vm.clone());
    *r.info.borrow_mut() = vm.clone();
    let status = r.miner().status().unwrap();
    assert_eq!(status["cloudWorkloads"][0]["appId"], other.as_str());
    assert_eq!(status["cloudWorkloads"][0]["sameRelease"], false);
    assert_eq!(r.deploy().unwrap_err().0, "existing_cloud_workload");
    r.calls.borrow_mut().clear();
    let deletes = |r: &Rig| {
        r.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p.starts_with("DELETE"))
            .count()
    };
    assert_eq!(
        r.miner()
            .delete(&"4".repeat(40), &|_| panic!("unknown VM must fail first"))
            .unwrap_err()
            .0,
        "cloud_workload_not_found"
    );
    assert!(
        r.miner()
            .delete(&other, &|_| Err(Error("cancelled")))
            .is_err()
    );
    assert_eq!(deletes(&r), 0);
    let out = r
        .miner()
        .delete(&other, &|q| {
            assert!(q.contains("did not deploy"));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (out["phase"].as_str(), out["recordedHere"].as_bool()),
        (Some("deleting"), Some(false))
    );
    assert_eq!(deletes(&r), 1);
    assert_eq!(
        r.state().read("deployment", false).unwrap()["phase"],
        "deleting"
    );
    r.miner()
        .delete(&other, &|_| panic!("must not repeat deletion"))
        .unwrap();
    assert_eq!(deletes(&r), 1);
    assert_eq!(r.deploy().unwrap_err().0, "deployment_already_recorded");
    *r.fail.borrow_mut() = "gone".into();
    assert_eq!(
        r.miner().delete(&other, &|_| panic!()).unwrap()["phase"],
        "deleted"
    );
    *r.fail.borrow_mut() = String::new();
    // Once Phala has removed it, this computer deploys its own worker.
    r.cvms.borrow_mut().clear();
    *r.info.borrow_mut() = own;
    r.deploy().unwrap();
    assert!(r.miner().status().unwrap()["cloudWorkloads"].is_null());
    // A confirmed deletion frees deploy again.
    let mut d = r.state().read("deployment", false).unwrap();
    d["phase"] = json!("deleted");
    r.state().write("deployment", &d).unwrap();
    r.deploy().unwrap();
    assert_eq!(r.phase(), "awaiting_attestation");
}

#[test]
fn logs_show_only_worker_events_without_forwarding_cloud_keys() {
    let r = Rig::new();
    r.deploy().unwrap();
    let out = r.miner().logs().unwrap();
    assert_eq!(out["events"].as_array().unwrap().len(), 1);
    assert_eq!(out["events"][0]["reason"], "http_403");
    assert!(!out.to_string().contains("do-not-print"));
}

#[test]
fn automatic_update_unavailable_and_current_are_not_claims_of_readiness() {
    let r = Rig::new();
    let envelope = r.staged_update(false, true);
    r.status.borrow_mut()["update"] = json!({"state":"unavailable"});
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| panic!(), &|_| {}, &|_| panic!())
        .unwrap();
    assert_eq!(out["state"], "unavailable");
    assert_eq!(out["ok"], false);
    let mut config = r.state().read("config", false).unwrap();
    config["invitation"] = envelope.clone();
    r.state().write("config", &config).unwrap();
    r.info.borrow_mut()["compose_hash"] = envelope["value"]["composeHash"].clone();
    r.status.borrow_mut()["update"] = json!({"state":"current","release":envelope});
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| panic!(), &|_| {}, &|_| panic!())
        .unwrap();
    assert_eq!(out["state"], "current");
    assert!(out["next"].as_str().unwrap().contains("health"));
    assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET")));
}

#[test]
fn automatic_update_requires_fresh_session_and_never_overrides_admin_disable() {
    for disabled in [false, true] {
        let r = Rig::new();
        let envelope = r.staged_update(true, true);
        let out = r
            .miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
                if stage == "attesting" {
                    r.advance_update(stage, &envelope);
                    if disabled {
                        r.status.borrow_mut()["enabled"] = json!(false);
                    } else {
                        r.status.borrow_mut()["sessionExpires"] = json!(now() - 1);
                    }
                    return false;
                }
                r.advance_update(stage, &envelope)
            })
            .unwrap();
        assert_eq!(out["stage"], "attesting");
        let resumed = r
            .miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false);
        if disabled {
            assert_eq!(resumed.unwrap_err().0, "worker_update_miner_disabled");
        } else {
            assert_eq!(resumed.unwrap()["stage"], "attesting");
        }
        assert_eq!(r.envs.borrow().len(), 1);
        assert_eq!(r.status.borrow()["draining"], true);
        assert!(!r.calls.borrow().iter().any(|(p, _)| p.ends_with("/resume")));
    }
}

#[test]
fn automatic_update_graceful_interrupt_saves_state_and_releases_profile_lock() {
    // A separate test-process is used so the signal handler cannot affect any
    // concurrently running lifecycle test.
    const MARKER: &str = "EVERYFRAME_WORKER_UPDATE_SIGNAL_TEST";
    if std::env::var_os(MARKER).is_some() {
        let r = Rig::new();
        r.staged_update(true, true);
        let out = r
            .miner()
            .worker_update(false, &|_| Ok(()), &|message| {
                if message.starts_with("Applying") {
                    unsafe {
                        libc::raise(libc::SIGINT);
                    }
                }
            })
            .unwrap();
        assert_eq!(out["state"], "waiting");
        assert_ne!(
            r.state().read("worker-update", false).unwrap()["stage"],
            "complete"
        );
        assert!(!r.path().join("operation.lock").exists());
        assert!(
            r.calls
                .borrow()
                .iter()
                .filter(|(p, _)| p.starts_with("PATCH") && p.ends_with("/compose_file"))
                .count()
                <= 1
        );
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "automatic_update_graceful_interrupt_saves_state_and_releases_profile_lock",
            "--nocapture",
        ])
        .env(MARKER, "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
impl Http for Rig {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        if u.host_str() == Some("logs.phala.network") {
            assert!(r.headers.is_empty());
            return Ok(b"raw do-not-print\n{\"event\":\"worker_cycle_failed\",\"reason\":\"http_403\",\"secret\":\"do-not-print\"}\n".to_vec());
        }
        let path = u.path();
        let body: Value = r
            .body
            .as_ref()
            .map(|b| serde_json::from_slice(b).unwrap())
            .unwrap_or(json!({}));
        self.calls
            .borrow_mut()
            .push((format!("{} {path}", r.method), self.phase()));
        if self.fail.borrow().as_str() == "history404" && path.ends_with("/v1/miner/jobs") {
            return Err(Error("http_404"));
        }
        if u.host_str() == Some("entrypoint-finney.opentensor.ai") {
            let result = match body["method"].as_str().unwrap() {
                "chain_getBlockHash" => self.trust["genesis"].clone(),
                "chain_getFinalizedHead" => json!(format!("0x{}", "a".repeat(64))),
                "state_getStorage" => {
                    let key = everyframe_miner::onboarding::uid_key(
                        117,
                        &everyframe_miner::hotkey::public_address(
                            self.inv["hotkey"].as_str().unwrap(),
                        )
                        .unwrap(),
                    );
                    if body["params"][0] == key {
                        json!("0x0000")
                    } else {
                        json!(format!(
                            "0x{}",
                            hex::encode(
                                everyframe_miner::hotkey::public_address(
                                    self.inv["hotkey"].as_str().unwrap()
                                )
                                .unwrap()
                            )
                        ))
                    }
                }
                _ => panic!("unexpected chain request"),
            };
            return Ok(json!({"jsonrpc":"2.0","id":1,"result":result})
                .to_string()
                .into_bytes());
        }
        if path.ends_with("/onboarding/challenge") {
            let issued = now();
            if body.get("recovery").is_some() {
                return Ok(self.sign(&json!({"kind":"everyframe-public-recovery-v1","recovery":body["recovery"],"chain":everyframe_miner::hotkey::chain(&self.trust).unwrap(),"audience":self.trust["coordinatorUrl"],"hotkey":body["hotkey"],"nonce":body["nonce"],"challenge":"a".repeat(48),"releaseHash":"b".repeat(64),"issuedAt":issued,"expiresAt":issued+120000})).to_string().into_bytes());
            }
            return Ok(self.sign(&json!({"kind":"everyframe-public-enrollment-v1","chain":everyframe_miner::hotkey::chain(&self.trust).unwrap(),"audience":self.trust["coordinatorUrl"],"hotkey":body["hotkey"],"nonce":body["nonce"],"challenge":"a".repeat(48),"releaseHash":"b".repeat(64),"issuedAt":issued,"expiresAt":issued+120000})).to_string().into_bytes());
        }
        if path.ends_with("/onboarding/enroll") {
            if body["challenge"]["value"]["kind"] == "everyframe-public-recovery-v1" {
                assert_eq!(
                    body["challenge"]["value"]["recovery"]["appId"],
                    self.status.borrow()["appId"]
                );
                let mut fresh = self.inv.clone();
                fresh["keyVersion"] = json!(self.inv["keyVersion"].as_i64().unwrap() + 1);
                fresh["appId"] = Value::Null;
                fresh["previousComposeHash"] = Value::Null;
                self.status.borrow_mut()["appId"] = json!("0".repeat(40));
                self.status.borrow_mut()["draining"] = json!(true);
                *self.recovered.borrow_mut() = Some(fresh.clone());
                return Ok(self.sign(&fresh).to_string().into_bytes());
            }
            if let Some(fresh) = self.recovered.borrow().as_ref() {
                return Ok(self.sign(fresh).to_string().into_bytes());
            }
            return Ok(self.sign(&self.inv).to_string().into_bytes());
        }
        if self.fail.borrow().as_str() == "auth" && path.ends_with("/v1/miner/status") {
            return Err(Error("http_401"));
        }
        if self.fail.borrow().as_str() == "gone" && path.contains("/cvms/app_") && r.method == "GET"
        {
            return Err(Error("http_404"));
        }
        // "reject:PATH" is a definitive Phala refusal (HTTP 422): not applied.
        if let Some(rejected) = self.fail.borrow().strip_prefix("reject:")
            && r.method != "GET"
            && path.ends_with(rejected)
        {
            return Err(Error("http_422"));
        }
        if !self.fail.borrow().is_empty() && path.ends_with(self.fail.borrow().as_str()) {
            return Err(Error("ambiguous_network_error"));
        }
        let v = if path.contains("/v1/miner/") {
            if self.inv["authMode"] == "hotkey-v1" {
                assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
                let h = &r
                    .headers
                    .iter()
                    .find(|(k, _)| k == "x-everyframe-auth")
                    .unwrap()
                    .1;
                let a: Value = serde_json::from_slice(&protocol::decode(h).unwrap()).unwrap();
                if a["certificate"]["value"]["keyVersion"]
                    != self.recovered.borrow().as_ref().unwrap_or(&self.inv)["keyVersion"]
                {
                    return Err(Error("http_401"));
                }
                assert_eq!(a["certificate"]["value"]["scope"], "console");
                let proof = protocol::verified(
                    &a["proof"],
                    a["certificate"]["value"]["delegateKey"].as_str().unwrap(),
                )
                .unwrap();
                assert_eq!(
                    proof["bodyHash"],
                    protocol::digest(&if r.body.is_some() {
                        body.clone()
                    } else {
                        Value::Null
                    })
                    .unwrap()
                );
            }
            let action = path.rsplit('/').next().unwrap();
            let nonce = if ["status", "worker-update", "jobs"].contains(&action) {
                u.query_pairs()
                    .find(|(k, _)| k == "nonce")
                    .unwrap()
                    .1
                    .to_string()
            } else {
                body["nonce"].as_str().unwrap().into()
            };
            let mut status = self.status.borrow_mut();
            if action == "bind-app" {
                assert_eq!(body["previousAppId"], status["appId"]);
                status["appId"] = body["appId"].clone();
                let mut inv = self.inv.clone();
                inv["appId"] = body["appId"].clone();
                status["deployment"] = self.sign(&inv);
            }
            if action == "balances" {
                let raw = body.to_string();
                assert!(!raw.contains("synthetic"));
                status["balances"] = body["balances"].clone();
            }
            if action == "drain" || action == "resume" {
                status["draining"] = json!(action == "drain")
            }
            if action == "offer" {
                assert_eq!(body["expectedRevision"], status["offers"][0]["revision"]);
                let rev = status["offers"][0]["revision"].as_i64().unwrap();
                status["offers"][0]["revision"] = json!(rev + 1);
                status["offers"][0]["discountBp"] = body["discountBp"].clone();
                status["offers"][0]["offered"] = body["active"].clone();
            }
            if action == "offer-batch" {
                for change in body["offers"].as_array().unwrap() {
                    let entry = status["offers"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|o| o["model"] == change["model"])
                        .unwrap();
                    if change["expectedRevision"] != entry["revision"] {
                        return Err(Error("offer_revision_conflict"));
                    }
                    if change["pricingHash"] != entry["pricingHash"] {
                        return Err(Error("offer_pricing_changed"));
                    }
                    entry["revision"] = json!(entry["revision"].as_i64().unwrap() + 1);
                    entry["discountBp"] = change["discountBp"].clone();
                    entry["offered"] = change["active"].clone();
                }
            }
            let mut out = status.clone();
            out["protocol"] = json!(1);
            out["nonce"] = json!(nonce);
            out["action"] = json!(action);
            out["at"] = json!(now());
            if action == "offer-batch" {
                out["offers"].as_array_mut().unwrap().retain(|o| {
                    body["offers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["model"] == o["model"])
                });
            }
            self.sign(&out)
        } else if path.ends_with("/account/billing") {
            assert_eq!(r.method, "GET");
            json!({"credits":{"currency":"USD","current_balance":12.345678}})
        } else if path.ends_with("/billing/user_balance") {
            assert_eq!(r.method, "GET");
            json!(12.345678)
        } else if path.ends_with("/auth/me") {
            assert_eq!(r.method, "GET");
            json!({"credits":{"balance":"2","granted_balance":"3.25","is_post_paid":false}})
        } else if path.ends_with("/instance-types") {
            json!({"result":[{"items":[{"id":"tdx.small","requires_gpu":false,"hourly_rate":"0.05"}]}]})
        } else if path.ends_with("/os-images") {
            json!({"items":[{"name":"dstack-0.5.9","is_dev":false,"requires_gpu":false,"os_image_hash":self.inv["osImageHash"]}]})
        } else if path.ends_with("/cvms/paginated") {
            json!({"items":self.cvms.borrow().clone()})
        } else if path.ends_with("/cvms/provision") {
            assert_eq!(self.phase(), "provision_intent");
            json!({"app_id":self.info.borrow()["app_id"],"compose_hash":self.inv["composeHash"],"os_image_hash":self.inv["osImageHash"],"app_env_encrypt_pubkey":hex::encode(PublicKey::from(&StaticSecret::from([0x11;32])).as_bytes()),"kms_info":{"slug":"test-kms"}})
        } else if path.contains("/pubkey/") {
            json!({"public_key":hex::encode(PublicKey::from(&StaticSecret::from([0x11;32])).as_bytes())})
        } else if path.ends_with("/compose_file/provision") {
            assert_eq!(self.phase(), "update_provision_intent");
            assert_eq!(
                body["update_env_vars"], true,
                "Required to change provider allowlist"
            );
            let mut compose = body.clone();
            compose.as_object_mut().unwrap().remove("update_env_vars");
            json!({"compose_hash":cloud::compose_hash(&compose).unwrap()})
        } else if path.ends_with("/envs")
            || path.ends_with("/compose_file")
            || path.ends_with("/cvms") && r.method == "POST"
        {
            assert!(!body.to_string().contains("synthetic-provider-key"));
            let env = decrypt(
                body["encrypted_env"].as_str().unwrap(),
                &StaticSecret::from([0x11; 32]),
            );
            assert!(!env.to_string().contains("stale-registry-key"));
            self.envs.borrow_mut().push(env);
            if path.ends_with("/envs") {
                assert_eq!(self.phase(), "activate_intent");
                json!({"status":"in_progress","allowed_envs_changed":false})
            } else if path.ends_with("/compose_file") {
                assert_eq!(self.phase(), "update_commit_intent");
                json!({})
            } else {
                assert_eq!(self.phase(), "commit_intent");
                self.info.borrow().clone()
            }
        } else if path.ends_with("/shutdown") {
            assert_eq!(self.phase(), "stop_intent");
            json!({})
        } else if path.ends_with("/start") {
            assert_eq!(self.phase(), "start_intent");
            json!({})
        } else if path.ends_with("/composition") {
            json!({"containers":[{"log_endpoint":"https://logs.phala.network/worker?token=do-not-print"}]})
        } else if path.contains("/cvms/app_") {
            self.info.borrow().clone()
        } else {
            panic!("unexpected fixture route {path}")
        };
        Ok(v.to_string().into_bytes())
    }
}
#[test]
fn original_phala_encryption_vector_decrypts() {
    let f: Value = serde_json::from_str(include_str!("fixtures/wire-vectors.json")).unwrap();
    let clear = decrypt(
        f["encryption"]["encryptedHex"].as_str().unwrap(),
        &StaticSecret::from([0x11; 32]),
    );
    assert_eq!(clear["env"], f["encryption"]["environment"]);
}
#[test]
fn public_replacement_preflights_support_and_saves_new_signed_binding() {
    use everyframe_miner::hotkey;
    use schnorrkel::{ExpansionMode, MiniSecretKey};
    use std::os::unix::fs::PermissionsExt;
    let mut r = Rig::new();
    r.dir = common::tempdir();
    let seed = [7u8; 32];
    let pair = MiniSecretKey::from_bytes(&seed)
        .unwrap()
        .expand_to_keypair(ExpansionMode::Ed25519);
    let address = hotkey::address(&pair.public.to_bytes());
    let path = r.dir.path().join("hotkey");
    std::fs::write(
        &path,
        json!({"secretSeed":hex::encode(seed),"ss58Address":address}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    r.inv["kind"] = json!("everyframe-miner-deployment-v2");
    r.inv["authMode"] = json!("hotkey-v1");
    r.inv["publicOnboarding"] = json!(true);
    r.inv["keyVersion"] = json!(1);
    r.inv["hotkey"] = json!(address);
    r.inv.as_object_mut().unwrap().remove("tokenHash");
    r.inv["request"]["compose_file"]["allowed_envs"] = json!(["MINER_ID", "MINER_AUTH", "FAL_KEY"]);
    r.inv["composeHash"] = json!(cloud::compose_hash(&r.inv["request"]["compose_file"]).unwrap());
    r.info.borrow_mut()["compose_hash"] = r.inv["composeHash"].clone();
    r.status.borrow_mut()["composeHash"] = r.inv["composeHash"].clone();
    r.credentials.as_object_mut().unwrap().remove("MINER_TOKEN");
    r.credentials["CONSOLE_AUTH"] = hotkey::create(&path, &r.inv, "console").unwrap();
    r.credentials["WORKER_AUTH"] = hotkey::create(&path, &r.inv, "worker").unwrap();
    r.miner().init(&r.sign(&r.inv), &r.credentials).unwrap();
    let old = "7".repeat(40);
    r.state()
        .write("deployment", &json!({"phase":"deleted","appId":old}))
        .unwrap();
    r.status.borrow_mut()["appId"] = json!(old);
    assert_eq!(r.deploy().unwrap_err().0, "replacement_not_supported");
    assert!(
        r.calls
            .borrow()
            .iter()
            .all(|(call, _)| call.starts_with("GET "))
    );
    r.status.borrow_mut()["replacementSupported"] = json!(true);
    assert_eq!(r.deploy().unwrap()["phase"], "awaiting_attestation");
    let config = r.state().read("config", false).unwrap();
    assert_eq!(
        config["invitation"]["value"]["appId"],
        r.info.borrow()["app_id"]
    );
    assert_eq!(
        r.state().read("deployment", false).unwrap()["replacesAppId"],
        old
    );
}

#[test]
fn hotkey_bootstrap_encrypts_only_worker_delegate_never_token_or_hotkey() {
    use everyframe_miner::hotkey;
    use schnorrkel::{ExpansionMode, MiniSecretKey};
    use std::os::unix::fs::PermissionsExt;
    let mut r = Rig::new();
    r.dir = common::tempdir();
    let seed = [7u8; 32];
    let pair = MiniSecretKey::from_bytes(&seed)
        .unwrap()
        .expand_to_keypair(ExpansionMode::Ed25519);
    let address = hotkey::address(&pair.public.to_bytes());
    let path = r.dir.path().join("hotkey");
    std::fs::write(
        &path,
        json!({"secretSeed":hex::encode(seed),"ss58Address":address}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    r.inv["kind"] = json!("everyframe-miner-deployment-v2");
    r.inv["authMode"] = json!("hotkey-v1");
    r.inv["keyVersion"] = json!(1);
    r.inv["hotkey"] = json!(address);
    r.inv.as_object_mut().unwrap().remove("tokenHash");
    r.inv["request"]["compose_file"]["allowed_envs"] = json!(["MINER_ID", "MINER_AUTH", "FAL_KEY"]);
    r.inv["composeHash"] = json!(cloud::compose_hash(&r.inv["request"]["compose_file"]).unwrap());
    r.info.borrow_mut()["compose_hash"] = r.inv["composeHash"].clone();
    r.status.borrow_mut()["composeHash"] = r.inv["composeHash"].clone();
    r.credentials.as_object_mut().unwrap().remove("MINER_TOKEN");
    r.credentials["CONSOLE_AUTH"] = hotkey::create(&path, &r.inv, "console").unwrap();
    r.credentials["WORKER_AUTH"] = hotkey::create(&path, &r.inv, "worker").unwrap();
    r.miner().init(&r.sign(&r.inv), &r.credentials).unwrap();
    assert_eq!(r.deploy().unwrap()["phase"], "awaiting_attestation");
    r.miner().activate(&|_| Ok(())).unwrap();
    assert!(r.miner().resume(&|_| Ok(())).is_err());
    r.status.borrow_mut()["attestation"]["at"] = json!(now() + 1000);
    r.miner().resume(&|_| Ok(())).unwrap();
    for env in r.envs.borrow().iter() {
        let text = env.to_string();
        assert!(!text.contains("MINER_TOKEN"));
        assert!(!text.contains(&hex::encode(seed)));
        assert!(!text.contains(r.credentials["CONSOLE_AUTH"]["secret"].as_str().unwrap()));
        let entry = env["env"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["key"] == "MINER_AUTH")
            .unwrap();
        let auth: Value = serde_json::from_str(entry["value"].as_str().unwrap()).unwrap();
        hotkey::validate(&auth, &r.inv, "worker").unwrap();
    }
}
#[test]
fn activation_reports_each_readiness_failure_without_releasing_keys() {
    let cases = [
        ("/enabled", json!(false), "miner_disabled"),
        ("/appId", json!("2".repeat(40)), "deployment_app_mismatch"),
        (
            "/composeHash",
            json!("e".repeat(64)),
            "deployment_compose_mismatch",
        ),
        (
            "/attestation/state",
            json!("pending"),
            "attestation_pending",
        ),
        (
            "/attestation/state",
            json!("rejected"),
            "attestation_rejected",
        ),
        ("/attestation", Value::Null, "attestation_unavailable"),
        (
            "/attestation/state",
            json!("unknown-secret"),
            "attestation_unavailable",
        ),
        (
            "/attestation/at",
            Value::Null,
            "attestation_timestamp_invalid",
        ),
        (
            "/attestation/at",
            json!("secret"),
            "attestation_timestamp_invalid",
        ),
        ("/attestation/at", json!(120000), "attestation_clock_skew"),
        ("/attestation/at", json!(-600001), "attestation_stale"),
        ("/attested", json!(false), "attestation_session_not_ready"),
        ("/online", json!(false), "worker_offline"),
    ];
    for (pointer, value, code) in cases {
        let r = Rig::new();
        r.deploy().unwrap();
        // Compute relative timestamps at the point of use. Earlier cases can
        // take >90s on a loaded runner, making a precomputed future timestamp
        // fall inside the permitted 30s clock-skew window before it is tested.
        let value = if ["attestation_clock_skew", "attestation_stale"].contains(&code) {
            json!(now() + value.as_i64().unwrap())
        } else {
            value
        };
        *r.status.borrow_mut().pointer_mut(pointer).unwrap() = value;
        r.calls.borrow_mut().clear();
        let err = r
            .miner()
            .activate(&|_| panic!("must reject {code} at {pointer} before confirmation"))
            .unwrap_err();
        assert_eq!(err.0, code, "{pointer}");
        let diagnostic = everyframe_miner::diagnostics::value(err.0);
        assert_eq!(diagnostic["code"], code);
        assert!(!diagnostic["next"].as_str().unwrap().is_empty());
        assert!(!diagnostic.to_string().contains("secret"));
        assert_eq!(r.envs.borrow().len(), 1); // Bootstrap only; no provider-key release.
        assert!(
            r.calls
                .borrow()
                .iter()
                .all(|(call, _)| call.starts_with("GET "))
        );
        assert_eq!(
            r.state().read("deployment", true).unwrap()["phase"],
            "awaiting_attestation"
        );
    }
}

#[test]
fn activation_rechecks_readiness_after_confirmation_without_releasing_keys() {
    let r = Rig::new();
    r.deploy().unwrap();
    let err = r
        .miner()
        .activate(&|_| {
            r.status.borrow_mut()["online"] = json!(false);
            Ok(())
        })
        .unwrap_err();
    assert_eq!(err.0, "worker_offline");
    assert_eq!(r.envs.borrow().len(), 1);
    assert_eq!(
        r.state().read("deployment", true).unwrap()["phase"],
        "awaiting_attestation"
    );
}

#[test]
fn doctor_explains_automatic_attestation_and_current_readiness() {
    let r = Rig::new();
    r.deploy().unwrap();
    for (state, expected) in [
        ("pending", "verification is in progress"),
        ("rejected", "failed automatic security verification"),
        ("accepted", "Automatic attestation accepted"),
    ] {
        r.status.borrow_mut()["attestation"]["state"] = json!(state);
        let out = r.miner().doctor().unwrap();
        let check = out["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Workload attestation")
            .unwrap();
        assert_eq!(check["ok"], state == "accepted");
        assert!(check["detail"].as_str().unwrap().contains(expected));
        assert!(!out.to_string().contains("Fresh reviewed evidence"));
        assert!(!out.to_string().contains("Operator controls"));
    }
    r.status.borrow_mut()["attestation"]["at"] = json!(now() - 600001);
    let out = r.miner().doctor().unwrap();
    let check = out["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Workload attestation")
        .unwrap();
    assert_eq!(check["ok"], false);
    assert!(check["detail"].as_str().unwrap().contains("too old"));
}

#[test]
fn bootstrap_activation_restart_and_shutdown() {
    let r = Rig::new();
    assert_eq!(r.deploy().unwrap()["phase"], "awaiting_attestation");
    assert!(
        r.envs.borrow()[0]
            .to_string()
            .contains(invitation::DISABLED)
    );
    assert!(
        !r.envs.borrow()[0]
            .to_string()
            .contains("synthetic-provider-key")
    );
    assert!(r.deploy().is_err());
    r.status.borrow_mut()["attestation"]["state"] = json!("pending");
    assert!(r.miner().activate(&|_| Ok(())).is_err());
    r.status.borrow_mut()["attestation"]["state"] = json!("accepted");
    assert_eq!(
        r.miner().activate(&|_| Ok(())).unwrap()["phase"],
        "awaiting_readmission"
    );
    assert!(
        r.envs.borrow()[1]
            .to_string()
            .contains("synthetic-provider-key")
    );
    assert!(r.miner().resume(&|_| Ok(())).is_err());
    r.status.borrow_mut()["attestation"]["at"] = json!(now() + 1000);
    assert_eq!(r.miner().resume(&|_| Ok(())).unwrap()["phase"], "running");
    r.status.borrow_mut()["activeJobs"] = json!(1);
    assert_eq!(
        r.miner().stop(false, &|_| Ok(())).unwrap()["phase"],
        "draining"
    );
    assert!(
        !r.calls
            .borrow()
            .iter()
            .any(|(c, _)| c.ends_with("/shutdown"))
    );
    r.status.borrow_mut()["activeJobs"] = json!(0);
    assert_eq!(
        r.miner().stop(false, &|_| Ok(())).unwrap()["phase"],
        "stopping"
    );
    r.miner().stop(false, &|_| Ok(())).unwrap();
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|(c, _)| c.ends_with("/shutdown"))
            .count(),
        1
    );
    r.info.borrow_mut()["status"] = json!("stopped");
    assert_eq!(r.miner().reconcile().unwrap()["phase"], "stopped");
    assert_eq!(
        r.miner().start(0.06, &|_| Ok(())).unwrap()["phase"],
        "starting"
    );
    r.info.borrow_mut()["status"] = json!("running");
    assert_eq!(
        r.miner().reconcile().unwrap()["phase"],
        "awaiting_readmission"
    );
}

#[test]
fn managed_keys_survive_init_and_only_apply_after_confirmation_and_idle_drain() {
    use everyframe_miner::onboarding;
    let r = Rig::new();
    onboarding::save_keys(&r.state(), &json!({"FAL_KEY":"synthetic-replacement-key", "MINIMAX_API_KEY":"synthetic-other-provider"}), None).unwrap();
    r.miner().init(&r.sign(&r.inv), &json!({})).unwrap();
    assert_eq!(
        r.state().read("credentials", false).unwrap()["FAL_KEY"],
        "synthetic-replacement-key"
    );
    assert!(r.miner().apply_api_keys(&|_| Ok(())).is_err());
    r.deploy().unwrap();
    r.miner().activate(&|_| Ok(())).unwrap();
    r.status.borrow_mut()["attestation"]["at"] = json!(now() + 1000);
    r.miner().resume(&|_| Ok(())).unwrap();
    let env_count = r.envs.borrow().len();
    onboarding::save_keys(
        &r.state(),
        &json!({"FAL_KEY":"synthetic-rotated-key"}),
        None,
    )
    .unwrap();
    assert_eq!(r.envs.borrow().len(), env_count);
    assert!(
        r.miner()
            .apply_api_keys(&|_| Err(Error("cancelled")))
            .is_err()
    );
    assert_eq!(r.envs.borrow().len(), env_count);
    r.status.borrow_mut()["activeJobs"] = json!(1);
    assert_eq!(
        r.miner().apply_api_keys(&|_| Ok(())).unwrap_err(),
        Error("draining_in_progress")
    );
    assert_eq!(r.envs.borrow().len(), env_count);
    r.status.borrow_mut()["activeJobs"] = json!(0);
    r.status.borrow_mut()["attestation"]["state"] = json!("pending");
    assert!(r.miner().apply_api_keys(&|_| Ok(())).is_err());
    r.status.borrow_mut()["attestation"]["state"] = json!("accepted");
    r.status.borrow_mut()["attestation"]["at"] = json!(now());
    assert_eq!(
        r.miner().apply_api_keys(&|_| Ok(())).unwrap()["phase"],
        "awaiting_readmission"
    );
    assert!(
        r.envs
            .borrow()
            .last()
            .unwrap()
            .to_string()
            .contains("synthetic-rotated-key")
    );
    assert!(
        !r.envs
            .borrow()
            .last()
            .unwrap()
            .to_string()
            .contains("synthetic-other-provider")
    );
    assert!(r.miner().resume(&|_| Ok(())).is_err());
    r.status.borrow_mut()["attestation"]["at"] = json!(now() + 1000);
    assert!(r.miner().resume(&|_| Ok(())).is_ok());
}
#[test]
fn ambiguous_cloud_mutations_keep_durable_intents() {
    for (path, phase) in [
        ("/cvms/provision", "provision_intent"),
        ("/cvms", "commit_intent"),
    ] {
        let r = Rig::new();
        *r.fail.borrow_mut() = path.into();
        assert!(r.deploy().is_err());
        assert_eq!(r.phase(), phase);
        assert!(r.deploy().is_err());
        assert_eq!(
            r.calls
                .borrow()
                .iter()
                .filter(|(c, _)| c == &format!("POST /api/v1{path}"))
                .count(),
            1
        );
    }
}
#[test]
fn limits_and_cancel_never_mutate_cloud() {
    for limit in [0., -1., f64::NAN, f64::INFINITY, 101., 0.01] {
        let r = Rig::new();
        assert!(r.miner().deploy(limit, &|_| Ok(())).is_err());
        assert_eq!(r.phase(), "");
        assert!(!r.calls.borrow().iter().any(|(c, _)| c.starts_with("POST")));
    }
    let r = Rig::new();
    assert!(
        r.miner()
            .deploy(0.06, &|_| Err(Error("cancelled")))
            .is_err()
    );
    assert_eq!(r.phase(), "");
}
#[test]
fn updated_image_requires_new_admission() {
    let r = Rig::new();
    r.deploy().unwrap();
    let mut inv = r.inv.clone();
    inv["release"] = json!("test-release-2");
    inv["appId"] = r.info.borrow()["app_id"].clone();
    inv["previousComposeHash"] = inv["composeHash"].clone();
    let compose = &mut inv["request"]["compose_file"];
    compose["docker_compose_file"] = json!(
        compose["docker_compose_file"]
            .as_str()
            .unwrap()
            .replace(&"a".repeat(64), &"b".repeat(64))
    );
    inv["composeHash"] = json!(cloud::compose_hash(&inv["request"]["compose_file"]).unwrap());
    let envelope = r.sign(&inv);
    assert_eq!(
        r.miner().update(&envelope, &|_| Ok(())).unwrap()["phase"],
        "updating"
    );
    assert!(r.miner().reconcile().is_err());
    r.info.borrow_mut()["compose_hash"] = inv["composeHash"].clone();
    assert_eq!(
        r.miner().reconcile().unwrap()["phase"],
        "awaiting_attestation"
    );
    assert_eq!(
        r.state().read("config", false).unwrap()["invitation"],
        envelope
    );
    assert_eq!(
        r.state().read("deployment", false).unwrap()["providerConfigured"],
        false
    );
}
#[test]
fn offers_use_revision_and_earnings_are_read_only() {
    let r = Rig::new();
    let v = r
        .miner()
        .offer("fixture/video", Some("15"), false, &|_| Ok(()))
        .unwrap();
    assert_eq!(v["offers"][0]["discountBp"], 1500);
    assert_eq!(v["offers"][0]["revision"], 1);
    assert_eq!(
        r.miner()
            .offer("fixture/video", None, true, &|_| Ok(()))
            .unwrap()["offers"][0]["offered"],
        false
    );
    assert_eq!(r.miner().earnings().unwrap()["payoutsImplemented"], false);
    assert!(r.envs.borrow().is_empty());
}
#[test]
fn unavailable_offer_pricing_has_actionable_error_without_changing_discounts() {
    for price in [Value::Null, json!("123456"), json!(0), json!(-1)] {
        for batch in [false, true] {
            let r = Rig::new();
            let mut unavailable = r.status.borrow()["offers"][0].clone();
            unavailable["model"] = json!("fixture/unpriced");
            unavailable["baseMinerRewardMicrousd"] = price.clone();
            r.status.borrow_mut()["offers"]
                .as_array_mut()
                .unwrap()
                .push(unavailable);
            r.status.borrow_mut()["batchOffersSupported"] = json!(true);
            r.calls.borrow_mut().clear();
            let no_review =
                |_: &str| -> Result<()> { panic!("unavailable pricing must fail before review") };
            let error = if batch {
                r.miner().offer_batch(
                    "fixture/video,fixture/unpriced",
                    Some("5"),
                    false,
                    &no_review,
                )
            } else {
                r.miner()
                    .offer("fixture/unpriced", Some("5"), false, &no_review)
            }
            .unwrap_err();
            assert_eq!(error.0, "offer_pricing_unavailable");
            let diagnostic = everyframe_miner::cli_errors::diagnostic(&error, None);
            assert_eq!(
                diagnostic["message"],
                "Pricing is temporarily unavailable for a selected model. No discounts were changed."
            );
            assert!(
                diagnostic["next"]
                    .as_str()
                    .unwrap()
                    .contains("coordinator needs to refresh")
            );
            assert!(r.calls.borrow().iter().all(|c| c.0.starts_with("GET ")));
            assert!(
                r.status.borrow()["offers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|o| o["revision"] == 0)
            );
        }
    }
}
#[test]
fn batches_review_once_and_use_one_status_plus_one_post_for_many_models() {
    let r = Rig::new();
    let template = r.status.borrow()["offers"][0].clone();
    let rows: Vec<_> = (0..64)
        .map(|i| {
            let mut v = template.clone();
            v["model"] = json!(format!("fixture/{i}"));
            v
        })
        .collect();
    let models = rows
        .iter()
        .map(|v| v["model"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join(",");
    r.status.borrow_mut()["offers"] = json!(rows);
    r.status.borrow_mut()["batchOffersSupported"] = json!(true);
    r.calls.borrow_mut().clear();
    let reviews = RefCell::new(Vec::new());
    let out = r
        .miner()
        .offer_batch(&models, Some("12.5"), false, &|q| {
            reviews.borrow_mut().push(q.to_string());
            Ok(())
        })
        .unwrap();
    assert_eq!(reviews.borrow().len(), 1);
    assert!(reviews.borrow()[0].contains("Review ID:"));
    assert_eq!(out["offers"].as_array().unwrap().len(), 64);
    assert!(
        out["offers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["revision"] == 1 && o["discountBp"] == 1250)
    );
    assert_eq!(r.calls.borrow().len(), 2);
    assert!(r.calls.borrow()[1].0.ends_with("/offer-batch"));
    assert!(r.envs.borrow().is_empty());
}
#[test]
fn batch_decline_changed_quotes_and_uncertain_posts_are_not_retried() {
    let r = Rig::new();
    r.status.borrow_mut()["batchOffersSupported"] = json!(true);
    let questions = RefCell::new(Vec::new());
    let capture = |q: &str| {
        questions.borrow_mut().push(q.to_string());
        Err(Error("cancelled"))
    };
    assert!(
        r.miner()
            .offer_batch("fixture/video", Some("10"), false, &capture)
            .is_err()
    );
    r.status.borrow_mut()["offers"][0]["pricingHash"] = json!("e".repeat(64));
    assert!(
        r.miner()
            .offer_batch("fixture/video", Some("10"), false, &capture)
            .is_err()
    );
    assert_ne!(
        questions.borrow()[0],
        questions.borrow()[1],
        "same reference price with a changed schedule needs review"
    );
    assert!(r.calls.borrow().iter().all(|c| c.0.starts_with("GET ")));
    *r.fail.borrow_mut() = "offer-batch".into();
    r.calls.borrow_mut().clear();
    assert_eq!(
        r.miner()
            .offer_batch("fixture/video", Some("10"), false, &|_| Ok(()))
            .unwrap_err()
            .0,
        "ambiguous_network_error"
    );
    assert_eq!(
        r.calls
            .borrow()
            .iter()
            .filter(|c| c.0.ends_with("/offer-batch"))
            .count(),
        1
    );
    assert_eq!(r.status.borrow()["offers"][0]["revision"], 0);
}
#[test]
fn batch_input_and_legacy_coordinator_fail_without_mutations_and_jobs_are_read_only() {
    let r = Rig::new();
    for ids in [
        "",
        "fixture/video,fixture/video",
        "fixture/video,",
        "bad\nmodel",
    ] {
        assert_eq!(
            r.miner()
                .offer_batch(ids, Some("10"), false, &|_| Ok(()))
                .unwrap_err()
                .0,
            "invalid_offer_batch"
        );
    }
    assert!(r.calls.borrow().is_empty());
    assert_eq!(
        r.miner()
            .offer_batch("fixture/video", Some("10"), false, &|_| Ok(()))
            .unwrap_err()
            .0,
        "batch_offers_not_supported"
    );
    r.status.borrow_mut()["jobsByModel"] =
        json!([{"model":"fixture/video","completed":42,"active":0,"failed":1,"cancelled":0}]);
    assert_eq!(r.miner().jobs().unwrap()["jobsByModel"][0]["completed"], 42);
    assert!(r.calls.borrow().iter().all(|c| c.0.starts_with("GET ")));
}

#[test]
fn individual_history_is_signed_scoped_get_and_does_not_change_offers_or_deployments() {
    let r = Rig::new();
    r.status.borrow_mut()["history"] = json!({"version":1,"currency":"USD","unit":"microusd","rows":[{"jobId":"job-fixture","state":"accepted","reviewState":"review"}],"nextCursor":null,"limit":20,"filters":{"status":"all","model":null}});
    let before = r.state().read("config", false).unwrap();
    let offers = r.status.borrow()["offers"].clone();
    r.calls.borrow_mut().clear();
    let out = r
        .miner()
        .job_history(json!({"limit":"20","status":"all"}))
        .unwrap();
    assert_eq!(out["historyAvailable"], true);
    assert_eq!(out["history"]["rows"][0]["reviewState"], "review");
    assert_eq!(r.calls.borrow().len(), 1);
    assert!(r.calls.borrow()[0].0.starts_with("GET "));
    assert!(r.calls.borrow()[0].0.ends_with("/v1/miner/jobs"));
    assert_eq!(r.state().read("config", false).unwrap(), before);
    assert_eq!(r.status.borrow()["offers"], offers);
    assert!(r.miner().job_history(json!({"minerId":"other"})).is_err());
    assert_eq!(r.calls.borrow().len(), 1);
    r.status.borrow_mut()["history"]["version"] = json!(99);
    assert_eq!(
        r.miner().job_history(json!({})).unwrap_err().0,
        "invalid_job_history"
    );
}

#[test]
fn history_on_an_older_coordinator_falls_back_explicitly_to_model_counts() {
    let r = Rig::new();
    r.status.borrow_mut()["jobsByModel"] = json!([{"model":"fixture/video","completed":3}]);
    *r.fail.borrow_mut() = "history404".into();
    r.calls.borrow_mut().clear();
    let out = r.miner().job_history(json!({"status":"failed"})).unwrap();
    assert_eq!(out["historyAvailable"], false);
    assert_eq!(out["jobsByModel"][0]["completed"], 3);
    assert!(
        out["note"]
            .as_str()
            .unwrap()
            .contains("Only all-time model counts")
    );
    assert_eq!(r.calls.borrow().len(), 2);
    assert!(r.calls.borrow().iter().all(|(p, _)| p.starts_with("GET ")));
}
#[test]
fn altered_expired_or_wrong_network_invitations_fail() {
    let r = Rig::new();
    let mut bad = r.sign(&r.inv);
    bad["value"]["minerId"] = json!("other");
    assert!(r.miner().validate(&bad, false).is_err());
    for field in ["genesis", "coordinatorUrl", "hotkey"] {
        let mut inv = r.inv.clone();
        inv[field] = json!("wrong");
        assert!(r.miner().validate(&r.sign(&inv), false).is_err());
    }
    let mut inv = r.inv.clone();
    inv["expiresAt"] = json!(now() - 1);
    assert!(r.miner().validate(&r.sign(&inv), false).is_err());
    assert!(r.miner().validate(&r.sign(&inv), true).is_ok());
}

impl Rig {
    /// An `activate` whose PATCH /envs response was lost: activate_intent.
    fn interrupted_activation(&self) {
        self.deploy().unwrap();
        *self.fail.borrow_mut() = "/envs".into();
        assert!(self.miner().activate(&|_| Ok(())).is_err());
        *self.fail.borrow_mut() = String::new();
        assert_eq!(self.phase(), "activate_intent");
        self.calls.borrow_mut().clear();
    }
    fn mutations(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .filter(|(p, _)| !p.starts_with("GET") && !p.contains("/v1/miner/"))
            .map(|(p, _)| p.clone())
            .collect()
    }
}

#[test]
fn reconcile_confirms_an_interrupted_activation_the_worker_proves_it_received() {
    let r = Rig::new();
    r.interrupted_activation();
    {
        // Restarted worker: attested after the request, advertising a model
        // backed by the released FAL key.
        let mut status = r.status.borrow_mut();
        status["attestation"]["at"] = json!(now() + 5);
        status["modelCatalog"] = json!([{"model":"fixture/video","provider":"fal","workerAvailable":true,"offerable":true,"blockers":[]}]);
    }
    let out = r.miner().reconcile().unwrap();
    assert_eq!(out["resolved"], true);
    assert_eq!(out["activation"], "confirmed");
    assert_eq!(r.phase(), "awaiting_readmission");
    let d = r.state().read("deployment", false).unwrap();
    assert_eq!(d["providerConfigured"], true);
    assert!(r.mutations().is_empty(), "{:?}", r.mutations());
    // The profile is unblocked: resume works without stop --force.
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert_eq!(r.miner().resume(&|_| Ok(())).unwrap()["phase"], "running");
    assert!(r.mutations().is_empty());
}

#[test]
fn reconcile_rolls_back_an_unconfirmed_activation_so_it_can_be_requested_again() {
    for evidence in ["none", "disabled_provider_only", "stale_attestation"] {
        let r = Rig::new();
        r.interrupted_activation();
        match evidence {
            // Worker reports a model, but not one backed by a released key.
            "disabled_provider_only" => {
                r.status.borrow_mut()["modelCatalog"] =
                    json!([{"model":"x/y","provider":"runway","workerAvailable":true}])
            }
            // Keys visible, but no attestation after the lost request.
            "stale_attestation" => {
                let mut status = r.status.borrow_mut();
                status["attestation"]["at"] = json!(now() - 60000);
                status["modelCatalog"] =
                    json!([{"model":"fixture/video","provider":"fal","workerAvailable":true}]);
            }
            _ => {}
        }
        let out = r.miner().reconcile().unwrap();
        assert_eq!(out["resolved"], true, "{evidence}");
        assert_eq!(out["activation"], "rolled_back", "{evidence}");
        assert!(out["next"].as_str().unwrap().contains("everycli activate"));
        assert_eq!(r.phase(), "awaiting_attestation", "{evidence}");
        assert_ne!(
            r.state().read("deployment", false).unwrap()["providerConfigured"],
            true
        );
        assert!(r.mutations().is_empty(), "{evidence}");
        // Not stuck: activate can be requested again, once, deliberately.
        r.status.borrow_mut()["attestation"]["at"] = json!(now());
        assert_eq!(
            r.miner().activate(&|_| Ok(())).unwrap()["phase"],
            "awaiting_readmission"
        );
        assert_eq!(
            r.mutations(),
            vec![format!("PATCH /api/v1/cvms/app_{}/envs", "1".repeat(40))]
        );
    }
    // A VM found stopped rolls back to stopped, which start can handle.
    let r = Rig::new();
    r.interrupted_activation();
    r.info.borrow_mut()["status"] = json!("stopped");
    assert_eq!(r.miner().reconcile().unwrap()["phase"], "stopped");
    assert_eq!(r.phase(), "stopped");
}

#[test]
fn worker_update_settles_an_interrupted_activation_instead_of_failing() {
    // Fresh worker-update on a profile left in activate_intent.
    let r = Rig::new();
    r.interrupted_activation();
    let mut d = r.state().read("deployment", false).unwrap();
    d["operationAt"] = json!(now() - 1000);
    r.state().write("deployment", &d).unwrap();
    let out = r
        .miner()
        .worker_update_with_wait(true, &|_| panic!(), &|_| {}, &|_| panic!())
        .unwrap_err();
    assert_eq!(out.0, "reconciliation_required");
    assert_eq!(r.phase(), "activate_intent"); // --check never settles.
    r.status.borrow_mut()["update"] = json!({"state":"unavailable"});
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| panic!())
        .unwrap();
    assert_eq!(out["state"], "unavailable");
    assert_eq!(r.phase(), "awaiting_attestation");
    assert!(r.mutations().is_empty());
}

#[test]
fn rejected_compose_patch_is_restored_by_reconcile_and_closed_by_worker_update() {
    // Manual --release flow, then reconcile.
    let r = Rig::new();
    let envelope = r.staged_update(false, true);
    *r.fail.borrow_mut() = "reject:/compose_file".into();
    let error = r.miner().update(&envelope, &|_| Ok(())).unwrap_err();
    assert_eq!(error.0, "phala_http_422");
    assert_eq!(r.phase(), "update_commit_intent");
    let d = r.state().read("deployment", false).unwrap();
    assert_eq!(d["commitRejected"]["code"], "phala_http_422");
    *r.fail.borrow_mut() = String::new();
    r.calls.borrow_mut().clear();
    let out = r.miner().reconcile().unwrap();
    assert_eq!(out["resolved"], true);
    assert_eq!(out["update"], "rolled_back");
    assert_eq!(out["rejectedCode"], "phala_http_422");
    assert_eq!(r.phase(), "drained");
    let d = r.state().read("deployment", false).unwrap();
    assert!(d["pendingInvitation"].is_null() && d["commitRejected"].is_null());
    assert_eq!(d["updateRejected"]["code"], "phala_http_422");
    assert_eq!(d["providerConfigured"], true);
    assert!(r.mutations().is_empty());
    // Not stuck: a later automatic update installs normally.
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");

    // Automatic flow: the rejection closes the update with a clear result.
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    *r.fail.borrow_mut() = "reject:/compose_file".into();
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "rejected");
    assert_eq!(out["ok"], false);
    assert_eq!(out["rejectedCode"], "phala_http_422");
    assert!(out["next"].as_str().unwrap().contains("everycli resume"));
    assert_eq!(r.phase(), "drained");
    let flow = r.state().read("worker-update", false).unwrap();
    assert_eq!(flow["stage"], "complete");
    assert_eq!(flow["aborted"], "rejected");
    assert_eq!(
        r.mutations()
            .iter()
            .filter(|p| p.starts_with("PATCH") && p.ends_with("/compose_file"))
            .count(),
        1
    );
    assert_eq!(r.status.borrow()["draining"], true); // never auto-resumed
    *r.fail.borrow_mut() = String::new();
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");
}

#[test]
fn worker_update_abort_is_a_noop_when_idle() {
    let r = Rig::new();
    let out = r.miner().worker_update_abort().unwrap();
    assert_eq!(out["state"], "nothing_pending");
    assert_eq!(out["ok"], true);
    assert!(!everyframe_miner::cli::result_failed("update", &out));
    r.deploy().unwrap();
    r.calls.borrow_mut().clear();
    let before = r.state().read("deployment", false).unwrap();
    let out = r.miner().worker_update_abort().unwrap();
    assert_eq!(out["state"], "nothing_pending");
    assert_eq!(out["aborted"], false);
    assert!(!everyframe_miner::cli::result_failed("update", &out));
    assert_eq!(r.state().read("deployment", false).unwrap(), before);
    assert!(r.state().read("worker-update", true).unwrap().is_null());
    assert!(r.calls.borrow().is_empty());
}

#[test]
fn worker_update_abort_cancels_a_draining_update_and_allows_a_fresh_one() {
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    r.status.borrow_mut()["activeJobs"] = json!(1);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false)
        .unwrap();
    assert_eq!(out["state"], "waiting");
    assert!(
        out["next"]
            .as_str()
            .unwrap()
            .contains("worker-update --abort")
    );
    r.calls.borrow_mut().clear();
    let out = r.miner().worker_update_abort().unwrap();
    assert_eq!(out["state"], "aborted");
    assert_eq!(out["aborted"], true);
    assert_eq!(out["phase"], "running");
    assert!(
        out["reason"]
            .as_str()
            .unwrap()
            .contains("journal was closed")
    );
    assert!(out["next"].as_str().unwrap().contains("everycli resume"));
    assert!(r.calls.borrow().is_empty()); // local only, no cloud or coordinator call
    let flow = r.state().read("worker-update", false).unwrap();
    assert_eq!(flow["stage"], "complete");
    assert_eq!(flow["aborted"], "aborted");
    assert_eq!(flow["abortedFromStage"], "draining");
    // Aborting twice is harmless.
    assert_eq!(
        r.miner().worker_update_abort().unwrap()["state"],
        "nothing_pending"
    );
    // A fresh update starts from discovery again.
    r.status.borrow_mut()["activeJobs"] = json!(0);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");
}

#[test]
fn worker_update_abort_restores_an_unobserved_commit_and_adopts_it_if_it_lands_later() {
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    *r.fail.borrow_mut() = "/compose_file".into(); // lost response, outcome unknown
    assert!(
        r.miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false)
            .is_err()
    );
    *r.fail.borrow_mut() = String::new();
    assert_eq!(r.phase(), "update_commit_intent");
    // v0.5.8 behaviour preserved: reconcile cannot tell, and says how to cancel.
    assert_eq!(r.miner().reconcile().unwrap_err().0, "update_not_observed");
    r.calls.borrow_mut().clear();
    let out = r.miner().worker_update_abort().unwrap();
    assert_eq!(out["state"], "aborted");
    assert_eq!(out["phase"], "drained");
    assert!(r.mutations().is_empty());
    let d = r.state().read("deployment", false).unwrap();
    assert!(d["pendingInvitation"].is_null());
    assert_eq!(d["abandonedInvitation"], envelope);
    assert_eq!(
        r.state().read("worker-update", false).unwrap()["stage"],
        "complete"
    );
    // Still manageable while the old release runs.
    assert!(r.miner().status().unwrap()["cloudError"].is_null());
    // If Phala applies the lost request later, reconcile adopts it.
    r.info.borrow_mut()["compose_hash"] = envelope["value"]["composeHash"].clone();
    assert!(r.miner().status().unwrap()["cloudError"].is_null());
    let out = r.miner().reconcile().unwrap();
    assert_eq!(out["lateUpdateAdopted"], true);
    assert_eq!(r.phase(), "awaiting_attestation");
    let config = r.state().read("config", false).unwrap();
    assert_eq!(config["invitation"], envelope);
    assert!(r.state().read("deployment", false).unwrap()["abandonedInvitation"].is_null());
}

#[test]
fn worker_update_abort_keeps_an_update_that_already_reached_the_vm() {
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    *r.fail.borrow_mut() = "/compose_file".into();
    assert!(
        r.miner()
            .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false)
            .is_err()
    );
    *r.fail.borrow_mut() = String::new();
    r.info.borrow_mut()["compose_hash"] = envelope["value"]["composeHash"].clone();
    r.calls.borrow_mut().clear();
    let out = r.miner().worker_update_abort().unwrap();
    assert_eq!(out["state"], "aborted");
    assert_eq!(out["phase"], "awaiting_attestation");
    assert!(
        out["reason"]
            .as_str()
            .unwrap()
            .contains("already reached the VM")
    );
    assert!(out["next"].as_str().unwrap().contains("everycli activate"));
    assert_eq!(
        r.state().read("config", false).unwrap()["invitation"],
        envelope
    );
    assert!(r.mutations().is_empty());
}

#[test]
fn expired_release_mid_update_stops_with_recovery_steps() {
    let expire = |r: &Rig| {
        let mut flow = r.state().read("worker-update", false).unwrap();
        let mut inv = flow["release"]["value"].clone();
        inv["issuedAt"] = json!(now() - 20000);
        inv["expiresAt"] = json!(now() - 1);
        flow["release"] = r.sign(&inv);
        r.state().write("worker-update", &flow).unwrap();
    };
    // Before installation: stop, nothing changed, --abort recovers.
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    r.status.borrow_mut()["activeJobs"] = json!(1);
    r.miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|_| false)
        .unwrap();
    expire(&r);
    r.status.borrow_mut()["activeJobs"] = json!(0);
    r.calls.borrow_mut().clear();
    for check in [true, false] {
        let out = r
            .miner()
            .worker_update_with_wait(check, &|_| Ok(()), &|_| {}, &|_| panic!("must not wait"))
            .unwrap();
        assert_eq!(out["state"], "release_expired");
        assert!(
            out["reason"]
                .as_str()
                .unwrap()
                .contains("before it was installed")
        );
        assert!(
            out["next"]
                .as_str()
                .unwrap()
                .contains("everycli worker-update --abort")
        );
        assert!(everyframe_miner::cli::result_failed("update", &out));
    }
    assert!(r.mutations().is_empty());
    assert_eq!(r.miner().worker_update_abort().unwrap()["state"], "aborted");
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");

    // After installation: rerunning finishes; the signature is still verified.
    let r = Rig::new();
    let envelope = r.staged_update(true, true);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            stage != "attesting" && r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["stage"], "attesting");
    expire(&r);
    let out = r
        .miner()
        .worker_update_with_wait(false, &|_| Ok(()), &|_| {}, &|stage| {
            r.advance_update(stage, &envelope)
        })
        .unwrap();
    assert_eq!(out["state"], "complete");
}
