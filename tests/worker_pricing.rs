use everyframe_miner::{
    Error, Result,
    network::{Http, Request},
    protocol::{self, Keys},
    worker::Runtime,
    worker_balances::BalanceSession,
    worker_pricing::Reporter,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
const MODEL: &str = "studio/media/fal/xai/grok-imagine-image";
struct Api(Cell<usize>);
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        self.0.set(self.0.get() + 1);
        assert!(!r.url.contains("queue.fal"));
        Ok(if r.method=="GET"{json!({"has_more":false,"prices":[{"endpoint_id":"xai/grok-imagine-image","unit":"images","currency":"USD"}]})}else{json!({"estimate_type":"unit_price","total_cost":"0.01","currency":"USD"})}.to_string().into_bytes())
    }
}
struct Transport {
    key: Keys,
    worker: String,
    clock: Cell<i64>,
    calls: RefCell<Vec<Value>>,
    fail: Cell<bool>,
    conflict: Cell<bool>,
    plan: RefCell<Value>,
}
impl Runtime for Transport {
    fn now(&self) -> i64 {
        self.clock.get()
    }
    fn quote(&self, _: &[u8]) -> Result<Value> {
        panic!("price telemetry must not request an attestation quote")
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        assert_eq!(path, "/v1/action");
        let v = protocol::verified(body, &self.worker)?;
        self.calls.borrow_mut().push(v.clone());
        let action = v["action"].as_str().unwrap();
        assert!(["price_plan", "prices"].contains(&action));
        if action == "prices" {
            if self.conflict.get() {
                return Err(Error("http_409"));
            }
            if self.fail.get() {
                return Err(Error("ack_lost"));
            }
        }
        protocol::signed(
            &json!({"sessionId":v["sessionId"],"requestNonce":v["nonce"],"result":if action=="price_plan"{json!({"ok":true,"providers":*self.plan.borrow()})}else{json!({"ok":true})}}),
            &self.key.signing,
        )
    }
}
fn fixture() -> (Api, Transport, BalanceSession) {
    let worker = Keys::default();
    (
        Api(Cell::new(0)),
        Transport {
            key: Keys::default(),
            worker: worker.signing_key(),
            clock: Cell::new(1000000),
            calls: RefCell::default(),
            fail: Cell::new(false),
            conflict: Cell::new(false),
            plan: RefCell::new(
                json!([{"provider":"fal","revision":1,"models":[{"model":MODEL,"apiModel":"xai/grok-imagine-image"}]}]),
            ),
        },
        BalanceSession {
            id: "session".into(),
            expires: 3000000,
            signing: worker.signing,
            automatic_offers: true,
        },
    )
}
#[test]
fn legacy_sessions_and_empty_policies_never_call_providers() {
    let (api, runtime, mut session) = fixture();
    let mut reporter = Reporter::default();
    let key = runtime.key.signing_key();
    session.automatic_offers = false;
    assert!(
        !reporter
            .tick(&api, &runtime, &json!({}), Some(&session), &key)
            .unwrap()
    );
    assert!(runtime.calls.borrow().is_empty());
    session.automatic_offers = true;
    *runtime.plan.borrow_mut() = json!([]);
    assert!(
        !reporter
            .tick(&api, &runtime, &json!({}), Some(&session), &key)
            .unwrap()
    );
    assert_eq!(runtime.calls.borrow().len(), 1);
    assert_eq!(api.0.get(), 0);
}
#[test]
fn lost_ack_reuses_observation_without_rechecking_prices_or_generating() {
    let (api, runtime, session) = fixture();
    let mut reporter = Reporter::default();
    let key = runtime.key.signing_key();
    let creds = json!({"FAL_KEY":"generation"});
    runtime.fail.set(true);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .is_err()
    );
    assert_eq!(api.0.get(), 2);
    let first = runtime.calls.borrow()[1]["payload"].clone();
    runtime.clock.set(runtime.now() + 15001);
    runtime.fail.set(false);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(runtime.calls.borrow()[2]["payload"], first);
    assert_eq!(api.0.get(), 2);
    assert!(
        !reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .unwrap()
    );
}
#[test]
fn changed_session_drops_pending_prices_and_refetches_signed_plan() {
    let (api, runtime, mut session) = fixture();
    let mut reporter = Reporter::default();
    let key = runtime.key.signing_key();
    let creds = json!({"FAL_KEY":"generation"});
    runtime.fail.set(true);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .is_err()
    );
    session.id = "next-session".into();
    runtime.fail.set(false);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(api.0.get(), 4);
    assert_eq!(runtime.calls.borrow()[2]["action"], "price_plan");
    assert_eq!(runtime.calls.borrow()[3]["sessionId"], "next-session");
}
#[test]
fn validate_whole_plan_before_provider_calls() {
    let (api, runtime, session) = fixture();
    runtime.plan.borrow_mut().as_array_mut().unwrap().push(json!({"provider":"fal","revision":1,"models":[{"model":MODEL,"apiModel":"xai/grok-imagine-image"}]}));
    let mut reporter = Reporter::default();
    assert!(
        reporter
            .tick(
                &api,
                &runtime,
                &json!({"FAL_KEY":"generation"}),
                Some(&session),
                &runtime.key.signing_key()
            )
            .is_err()
    );
    assert_eq!(api.0.get(), 0);
    assert_eq!(runtime.calls.borrow().len(), 1);
}
#[test]
fn stale_pending_data_is_rechecked_instead_of_resending_expired_prices() {
    let (api, runtime, session) = fixture();
    let mut reporter = Reporter::default();
    let key = runtime.key.signing_key();
    let creds = json!({"FAL_KEY":"generation"});
    runtime.fail.set(true);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .is_err()
    );
    runtime.clock.set(runtime.now() + 300001);
    runtime.fail.set(false);
    assert!(
        reporter
            .tick(&api, &runtime, &creds, Some(&session), &key)
            .unwrap()
    );
    assert_eq!(api.0.get(), 4);
    assert_eq!(runtime.calls.borrow()[2]["action"], "price_plan");
}
