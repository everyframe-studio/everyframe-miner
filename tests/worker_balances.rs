use everyframe_miner::{
    Error, Result, balances,
    network::{Http, Request},
    protocol::{self, Keys},
    worker::Runtime,
    worker_balances::{BalanceSession, CHECK_INTERVAL_MS, RETRY_INTERVAL_MS, Reporter},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

struct Api {
    calls: RefCell<Vec<Request>>,
    fail: Cell<bool>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert_eq!(r.method, "GET");
        assert_eq!(r.timeout, 5);
        assert_eq!(r.limit, 64000);
        assert!(r.redirect_hosts.is_empty());
        assert!(r.body.is_none());
        let user_balance = r.url == "https://rest.alpha.fal.ai/billing/user_balance";
        self.calls.borrow_mut().push(r);
        if self.fail.get() {
            return Err(Error("PRIVATE_UPSTREAM_FAILURE"));
        }
        if user_balance {
            return Ok(b"42.5".to_vec());
        }
        Ok(json!({"credits":{"currency":"USD","current_balance":42.5},"data":{"total_credits":50,"total_usage":7.5}}).to_string().into_bytes())
    }
}
struct Transport {
    key: Keys,
    worker_key: String,
    clock: Cell<i64>,
    calls: RefCell<Vec<Value>>,
    fail: Cell<bool>,
    wrong_nonce: Cell<bool>,
}
impl Runtime for Transport {
    fn now(&self) -> i64 {
        self.clock.get()
    }
    fn quote(&self, _: &[u8]) -> Result<Value> {
        panic!("Balance reporter must not generate quotes")
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        assert_eq!(path, "/v1/action");
        let v = protocol::verified(body, &self.worker_key)?;
        assert_eq!(v["action"], "balances");
        self.calls.borrow_mut().push(v.clone());
        if self.fail.get() {
            return Err(Error("ack_lost"));
        }
        protocol::signed(
            &json!({"sessionId":v["sessionId"],"requestNonce":if self.wrong_nonce.get(){json!("wrong")}else{v["nonce"].clone()},"result":{"ok":true}}),
            &self.key.signing,
        )
    }
}
fn fixture() -> (Api, Transport, BalanceSession) {
    let keys = Keys::default();
    let now = everyframe_miner::now();
    let session = BalanceSession {
        id: "session-fixture".into(),
        expires: now + 2_000_000,
        signing: keys.signing.clone(),
    };
    (
        Api {
            calls: RefCell::default(),
            fail: Cell::new(false),
        },
        Transport {
            key: Keys::default(),
            worker_key: keys.signing_key(),
            clock: Cell::new(now),
            calls: RefCell::default(),
            fail: Cell::new(false),
            wrong_nonce: Cell::new(false),
        },
        session,
    )
}
#[test]
fn deployed_keys_only_and_matching_credential_identity_without_leaking_keys() {
    let (api, _, _) = fixture();
    let creds = json!({"FAL_KEY":"generation-secret-key","FAL_ADMIN_KEY":"DO-NOT-DEPLOY-ADMIN","OPENROUTER_MANAGEMENT_KEY":"DO-NOT-DEPLOY-MANAGEMENT","PHALA_CLOUD_API_KEY":"DO-NOT-DEPLOY-HOSTING","VIVIX_API_KEY":"vivix-secret-key"});
    let reports = balances::worker_reports(&api, &creds);
    assert_eq!(reports.len(), 2);
    assert_eq!(api.calls.borrow().len(), 2);
    assert_eq!(
        api.calls.borrow()[0].headers[0].1,
        "Key generation-secret-key"
    );
    assert_eq!(reports[0]["remainingMicrousd"], 42_500_000);
    assert_eq!(reports[1]["status"], "unavailable");
    assert_eq!(
        reports[0]["credentialRef"],
        protocol::sha("everyframe-worker-balance-credential-v1\0fal\0generation-secret-key")
    );
    assert!(!json!(&reports).to_string().contains("secret-key"));
    assert!(!json!(&reports).to_string().contains("DEPLOY"));
    assert!(
        balances::worker_reports(&api, &json!({"FAL_ADMIN_KEY":"billing-secret-only"})).is_empty()
    );
}
#[test]
fn automatic_interval_signed_binding_no_requests_without_fresh_session() {
    let (api, transport, mut session) = fixture();
    let mut reporter = Reporter::default();
    let creds = json!({"FAL_KEY":"generation-secret-key"});
    let key = transport.key.signing_key();
    assert!(!reporter.tick(&api, &transport, &creds, None, &key).unwrap());
    assert!(api.calls.borrow().is_empty());
    session.expires = transport.now() + 9999;
    assert!(
        !reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    session.expires = transport.now() + 2_000_000;
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    assert!(
        !reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(api.calls.borrow().len(), 1);
    transport.clock.set(transport.now() + CHECK_INTERVAL_MS - 1);
    assert!(
        !reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    transport.clock.set(transport.now() + 1);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(api.calls.borrow().len(), 2);
    assert_eq!(transport.calls.borrow()[0]["sessionId"], session.id);
    assert_ne!(
        transport.calls.borrow()[0]["nonce"],
        transport.calls.borrow()[1]["nonce"]
    );
}
#[test]
fn failed_provider_replaces_success_and_lost_ack_retries_metadata_only() {
    let (api, transport, session) = fixture();
    let mut reporter = Reporter::default();
    let creds = json!({"FAL_KEY":"generation-secret-key"});
    let key = transport.key.signing_key();
    transport.fail.set(true);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .is_err()
    );
    assert!(
        !reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    transport.fail.set(false);
    transport.clock.set(transport.now() + RETRY_INTERVAL_MS);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(api.calls.borrow().len(), 1);
    assert_ne!(
        transport.calls.borrow()[0]["nonce"],
        transport.calls.borrow()[1]["nonce"]
    );
    api.fail.set(true);
    transport.clock.set(transport.now() + CHECK_INTERVAL_MS);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    let calls = transport.calls.borrow();
    let rows = &calls.last().unwrap()["payload"]["balances"];
    assert_eq!(rows[0]["status"], "unavailable");
    assert_eq!(rows[0]["remainingMicrousd"], Value::Null);
    assert!(!rows.to_string().contains("PRIVATE_UPSTREAM"));
}
#[test]
fn renewal_rebinds_cached_report_and_replayed_ack_is_not_accepted() {
    let (api, transport, mut session) = fixture();
    let mut reporter = Reporter::default();
    let creds = json!({"FAL_KEY":"generation-secret-key"});
    let key = transport.key.signing_key();
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    session.id = "new-attested-session".into();
    transport.clock.set(transport.now() + RETRY_INTERVAL_MS);
    transport.wrong_nonce.set(true);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .is_err()
    );
    transport.wrong_nonce.set(false);
    transport.clock.set(transport.now() + RETRY_INTERVAL_MS);
    assert!(
        reporter
            .tick(&api, &transport, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(
        api.calls.borrow().len(),
        2,
        "The one-minute refresh also occurs while retrying a rejected acknowledgment"
    );
    assert_eq!(
        transport.calls.borrow().last().unwrap()["sessionId"],
        "new-attested-session"
    );
}
#[test]
fn revoked_worker_snapshot_is_stale_in_cli_even_with_recent_provider_check() {
    let now = everyframe_miner::now();
    let rows = json!([{"provider":"fal","currency":"USD","remainingMicrousd":10_000_000,"status":"ok","checkedAt":now,"source":"attested-worker","attestationCurrent":false,"stale":true}]);
    let merged = balances::merge(&rows, vec![], now);
    assert_eq!(merged[0]["source"], "worker");
    assert_eq!(merged[0]["stale"], true);
}
