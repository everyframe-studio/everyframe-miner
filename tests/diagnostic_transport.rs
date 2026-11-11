use everyframe_miner::{
    Result, models,
    protocol::{self, Keys},
    providers::{Output, Provider},
    worker::{Runtime, Worker},
    worker_trace,
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

struct State {
    key: Keys,
    worker: RefCell<String>,
    requests: Vec<Value>,
    reports: RefCell<Vec<Value>>,
}
#[derive(Clone)]
struct Transport(Rc<State>);
impl Runtime for Transport {
    fn now(&self) -> i64 {
        1000
    }
    fn quote(&self, _: &[u8]) -> Result<Value> {
        Ok(json!({}))
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let v = match path {
            "/v1/challenge" => {
                json!({"id":"challenge","minerId":"fixture","nonce":"nonce","expires":10000000})
            }
            "/v1/attest" => {
                *self.0.worker.borrow_mut() = body["signingKey"].as_str().unwrap().into();
                json!({"sessionId":"session","challengeId":"challenge","signingKey":body["signingKey"],"expires":10000000,"diagnosticRequests":1})
            }
            "/v1/action" => {
                let b = protocol::verified(body, &self.0.worker.borrow())?;
                let result = match b["action"].as_str().unwrap() {
                    "claim" => {
                        assert_eq!(b["payload"]["diagnosticPolicy"], 1);
                        json!({"work":null,"diagnosticRequests":self.0.requests})
                    }
                    "diagnostic_report" => {
                        self.0.reports.borrow_mut().push(b["payload"].clone());
                        json!({"ok":true})
                    }
                    action => panic!("unexpected action {action}"),
                };
                json!({"sessionId":"session","requestNonce":b["nonce"],"result":result})
            }
            _ => panic!("unexpected endpoint"),
        };
        protocol::signed(&v, &self.0.key.signing)
    }
}
struct NoGeneration;
impl Provider for NoGeneration {
    fn models(&self) -> Vec<String> {
        vec![models::DEFAULT_MODEL.into()]
    }
    fn submit(&mut self, _: &Value) -> Result<String> {
        panic!("diagnostics must never submit a generation")
    }
    fn poll(&mut self, _: &str, _: &Value) -> Result<String> {
        panic!("diagnostics must never poll a provider")
    }
    fn result(&mut self, _: &str, _: &Value) -> Result<Output> {
        panic!("diagnostics must never fetch provider output")
    }
}
fn fixture(request: Value) -> (Worker<Transport, NoGeneration>, Rc<State>) {
    let state = Rc::new(State {
        key: Keys::default(),
        worker: RefCell::new(String::new()),
        requests: vec![request],
        reports: RefCell::new(vec![]),
    });
    (
        Worker {
            miner_id: "fixture".into(),
            coordinator_key: state.key.signing_key(),
            chain: Value::Null,
            keys: Keys::default(),
            session: Value::Null,
            runtime: Transport(state.clone()),
            provider: NoGeneration,
            provider_pause: Default::default(),
        },
        state,
    )
}
fn request(n: usize) -> Value {
    json!({"requestId":format!("{n:048x}"),"jobId":format!("{:048x}",n+100),"attemptId":format!("{:048x}",n+200),"inputHash":"c".repeat(64),"expiresAt":10000})
}
#[test]
fn signed_request_returns_only_the_matching_cached_trace_without_provider_access() {
    let r = request(7);
    let w = json!({"jobId":r["jobId"],"attemptId":r["attemptId"],"inputHash":r["inputHash"]});
    worker_trace::record(
        &w,
        &json!({"phase":"worker_failed","code":"untrusted_destination","prompt":"PRIVATE"}),
        1000,
    );
    let (mut worker, state) = fixture(r.clone());
    assert_eq!(worker.step().unwrap()["state"], "idle");
    let reports = state.reports.borrow();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["state"], "available");
    assert_eq!(reports[0]["requestId"], r["requestId"]);
    assert_eq!(
        reports[0]["trace"]["events"][0]["code"],
        "untrusted_destination"
    );
    assert!(!reports[0].to_string().contains("PRIVATE"));
}
#[test]
fn unknown_history_has_an_explicit_signed_unavailable_response() {
    let (mut worker, state) = fixture(request(8));
    assert_eq!(worker.step().unwrap()["state"], "idle");
    assert_eq!(state.reports.borrow()[0]["state"], "not_retained");
    assert!(state.reports.borrow()[0]["trace"].is_null());
}
#[test]
fn requests_with_file_or_command_fields_are_not_executed() {
    let mut r = request(9);
    r["path"] = json!("/proc/self/environ");
    let (mut worker, state) = fixture(r);
    assert_eq!(worker.step().unwrap()["state"], "idle");
    assert!(state.reports.borrow().is_empty());
}
