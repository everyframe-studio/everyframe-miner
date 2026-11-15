//! Explicit no-generation canary. Never runs in ordinary CI or on a user miner.
use everyframe_miner::{
    Error, Result, invitation,
    miner::Miner,
    network::{Http, PublicHttp, Request},
    state::State,
};
use serde_json::{Value, json};
use std::{cell::Cell, fs, path::PathBuf};

struct Fault {
    applied: bool,
    fired: Cell<bool>,
    cloud_writes: Cell<usize>,
}
impl Http for Fault {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        if !r.url.starts_with("https://subnet.everyframe.studio/") && r.method != "GET" {
            self.cloud_writes.set(self.cloud_writes.get() + 1);
            return Err(Error("unexpected_cloud_mutation"));
        }
        if r.url.ends_with("/worker-auto-update-policy") && !self.fired.replace(true) {
            if self.applied {
                PublicHttp.bytes(r)?;
            }
            return Err(Error("coordinator_request_failed"));
        }
        PublicHttp.bytes(r)
    }
}

#[test]
#[ignore = "requires the explicitly approved isolated updater canary; no generations"]
fn isolated_updater_policy_recovery() {
    assert_eq!(
        std::env::var("EVERYFRAME_UPDATER_CANARY_APPROVED").as_deref(),
        Ok("no-generation-0.10")
    );
    let root = PathBuf::from(std::env::var("EVERYFRAME_UPDATER_CANARY_DIR").unwrap());
    let context: Value =
        serde_json::from_slice(&fs::read(root.join("context.json")).unwrap()).unwrap();
    assert!(
        context["id"]
            .as_str()
            .unwrap()
            .starts_with("updater-recovery-20261114-")
    );
    assert_eq!(context["maxGenerations"], 0);
    let profile = PathBuf::from(context["profile"].as_str().unwrap());
    let original = Miner {
        state: State {
            directory: profile.clone(),
        },
        trust: invitation::trust("mainnet").unwrap(),
        http: &PublicHttp,
    };
    let before = original.worker_auto_settings(None, &|_| panic!()).unwrap();
    assert_eq!(before["automaticUpdates"]["enabled"], true);
    let status = original.status().unwrap();
    assert_eq!(status["coordinator"]["draining"], false);

    // Model a second management copy that requires full enrollment, while
    // preserving the working profile and the VM's existing capsule.
    let scratch = tempfile::tempdir().unwrap();
    let state = State {
        directory: scratch.path().join("profile"),
    };
    for name in ["config", "deployment", "credentials", "worker-auto-update"] {
        let mut value = original.state.read(name, true).unwrap();
        if name == "credentials" {
            value.as_object_mut().unwrap().remove("UPDATER_SEED");
        }
        if name == "deployment" {
            // The remote controller completed the enrollment while the laptop
            // retained its pre-readmission phase. Use a recovered canary copy.
            assert_eq!(status["coordinator"]["attestation"]["state"], "accepted");
            assert!(
                status["coordinator"]["attestation"]["at"].as_i64().unwrap()
                    > value["operationAt"].as_i64().unwrap()
            );
            value["phase"] = json!("running");
        }
        state.write(name, &value).unwrap();
    }
    let fault = Fault {
        applied: false,
        fired: false.into(),
        cloud_writes: 0.into(),
    };
    let recovering = Miner {
        state,
        trust: invitation::trust("mainnet").unwrap(),
        http: &fault,
    };
    let error = recovering.enroll_updater(&|_| Ok(())).unwrap_err();
    assert_eq!(error.0, "updater_enrollment_restored");
    assert_eq!(fault.cloud_writes.get(), 0);
    let recovered = original.status().unwrap();
    assert_eq!(recovered["coordinator"]["draining"], false);
    assert_eq!(
        recovered["coordinator"]["automaticUpdates"]["enabled"],
        true
    );
    assert_eq!(
        recovered["coordinator"]["automaticUpdates"]["capsuleId"],
        before["automaticUpdates"]["capsuleId"]
    );

    let lost = Fault {
        applied: true,
        fired: false.into(),
        cloud_writes: 0.into(),
    };
    let miner = Miner {
        state: State {
            directory: profile.clone(),
        },
        trust: invitation::trust("mainnet").unwrap(),
        http: &lost,
    };
    let disabled = miner
        .worker_auto_settings(Some(false), &|_| Ok(()))
        .unwrap();
    assert_eq!(disabled["automaticUpdates"]["enabled"], false);
    assert_eq!(lost.cloud_writes.get(), 0);
    let lost = Fault {
        applied: true,
        fired: false.into(),
        cloud_writes: 0.into(),
    };
    let miner = Miner {
        state: State { directory: profile },
        trust: invitation::trust("mainnet").unwrap(),
        http: &lost,
    };
    let enabled = miner.enroll_updater(&|_| Ok(())).unwrap();
    assert_eq!(enabled["state"], "enabled");
    assert_eq!(lost.cloud_writes.get(), 0);
    let final_status = original.status().unwrap();
    assert_eq!(final_status["coordinator"]["draining"], false);
    assert_eq!(
        final_status["coordinator"]["automaticUpdates"]["enabled"],
        true
    );
    fs::write(
        root.join("recovery-proof.json"),
        json!({"passed":true,"rejectedEnableRestored":true,
        "lostDisableReplyReconciled":true,"lostEnableReplyReconciled":true,"cloudWrites":0,
        "draining":false,"automaticUpdates":true,"minerId":context["id"]})
        .to_string(),
    )
    .unwrap();
}
