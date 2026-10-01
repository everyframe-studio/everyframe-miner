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
}
impl Rig {
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
impl Http for Rig {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        let path = u.path();
        let body: Value = r
            .body
            .as_ref()
            .map(|b| serde_json::from_slice(b).unwrap())
            .unwrap_or(json!({}));
        self.calls
            .borrow_mut()
            .push((format!("{} {path}", r.method), self.phase()));
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
            let nonce = if action == "status" {
                u.query_pairs()
                    .find(|(k, _)| k == "nonce")
                    .unwrap()
                    .1
                    .to_string()
            } else {
                body["nonce"].as_str().unwrap().into()
            };
            let mut status = self.status.borrow_mut();
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
            let mut out = status.clone();
            out["protocol"] = json!(1);
            out["nonce"] = json!(nonce);
            out["action"] = json!(action);
            out["at"] = json!(now());
            self.sign(&out)
        } else if path.ends_with("/account/billing") {
            assert_eq!(r.method, "GET");
            json!({"credits":{"currency":"USD","current_balance":12.345678}})
        } else if path.ends_with("/auth/me") {
            assert_eq!(r.method, "GET");
            json!({"credits":{"balance":"2","granted_balance":"3.25","is_post_paid":false}})
        } else if path.ends_with("/instance-types") {
            json!({"result":[{"items":[{"id":"tdx.small","requires_gpu":false,"hourly_rate":"0.05"}]}]})
        } else if path.ends_with("/os-images") {
            json!({"items":[{"name":"dstack-0.5.9","is_dev":false,"requires_gpu":false,"os_image_hash":self.inv["osImageHash"]}]})
        } else if path.ends_with("/cvms/paginated") {
            json!({"items":[]})
        } else if path.ends_with("/cvms/provision") {
            assert_eq!(self.phase(), "provision_intent");
            json!({"app_id":self.info.borrow()["app_id"],"compose_hash":self.inv["composeHash"],"os_image_hash":self.inv["osImageHash"],"app_env_encrypt_pubkey":hex::encode(PublicKey::from(&StaticSecret::from([0x11;32])).as_bytes()),"kms_info":{"slug":"test-kms"}})
        } else if path.contains("/pubkey/") {
            json!({"public_key":hex::encode(PublicKey::from(&StaticSecret::from([0x11;32])).as_bytes())})
        } else if path.ends_with("/compose_file/provision") {
            assert_eq!(self.phase(), "update_provision_intent");
            json!({"compose_hash":cloud::compose_hash(&body).unwrap()})
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
