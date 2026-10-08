use everyframe_miner::{
    Error, Result, models,
    protocol::{self, Keys},
    providers::{Output, Provider},
    worker::{Runtime, Worker},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Options {
    submit_error: bool,
    denied: bool,
    bad_grant: bool,
    replay: bool,
    submitted_failures: u32,
    complete_failures: u32,
    failed: bool,
    pending: bool,
    bad_chain: bool,
}
struct Fixture {
    key: Keys,
    work: Value,
    clock: Cell<i64>,
    worker_key: RefCell<String>,
    encryption_key: RefCell<String>,
    calls: RefCell<Vec<String>>,
    receipts: RefCell<Vec<Value>>,
    submits: Cell<u32>,
    submitted: Cell<u32>,
    complete: Cell<u32>,
    options: Options,
}
#[derive(Clone)]
struct Handle(Rc<Fixture>);
impl std::ops::Deref for Handle {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.0
    }
}
impl Runtime for Handle {
    fn now(&self) -> i64 {
        self.clock.get()
    }
    fn sleep(&self, ms: u64) {
        self.clock.set(self.clock.get() + ms as i64)
    }
    fn quote(&self, data: &[u8]) -> Result<Value> {
        assert_eq!(data.len(), 64);
        Ok(json!({"synthetic":true}))
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let value = match path {
            "/v1/challenge" => {
                let mut v = json!({"id":"challenge","nonce":"nonce","minerId":"fixture-miner","expires":9999999});
                if self.options.bad_chain {
                    v["chain"] = json!({"network":"wrong"});
                }
                v
            }
            "/v1/attest" => {
                assert_eq!(body["evidence"]["synthetic"], true);
                *self.worker_key.borrow_mut() = body["signingKey"].as_str().unwrap().into();
                *self.encryption_key.borrow_mut() = body["encryptionKey"].as_str().unwrap().into();
                json!({"sessionId":"session","challengeId":"challenge","signingKey":body["signingKey"],"expires":9999999})
            }
            "/v1/action" => {
                let request = protocol::verified(body, &self.worker_key.borrow())?;
                let action = request["action"].as_str().unwrap();
                self.calls.borrow_mut().push(action.into());
                let result = match action {
                    "claim" => {
                        assert_eq!(request["payload"]["durationPolicy"], 1);
                        json!({"work":protocol::seal(&self.work,&self.encryption_key.borrow(),&format!("session:{}",request["nonce"].as_str().unwrap()))?})
                    }
                    "start" => {
                        json!({"permitted":!self.options.denied,"attemptId":self.work["attemptId"],"inputHash":if self.options.bad_grant {json!("wrong")} else {self.work["inputHash"].clone()},"grantNonce":"grant"})
                    }
                    "submitted" => {
                        self.submitted.set(self.submitted.get() + 1);
                        if self.submitted.get() <= self.options.submitted_failures {
                            return Err(Error("lost_ack"));
                        }
                        json!({})
                    }
                    "complete" => {
                        let receipt = protocol::verified(
                            &request["payload"]["receipt"],
                            &self.worker_key.borrow(),
                        )?;
                        self.receipts.borrow_mut().push(request["payload"].clone());
                        self.complete.set(self.complete.get() + 1);
                        if self.complete.get() <= self.options.complete_failures {
                            return Err(Error("lost_ack"));
                        }
                        json!({"jobId":receipt["jobId"],"outputHash":receipt["outputHash"]})
                    }
                    "unknown" | "failed" | "heartbeat" => json!({}),
                    _ => panic!("unexpected action {action}"),
                };
                json!({"sessionId":"session","requestNonce":if self.options.replay {json!("other")}else{request["nonce"].clone()},"result":result})
            }
            _ => panic!("unexpected path"),
        };
        protocol::signed(&value, &self.key.signing)
    }
}
impl Provider for Handle {
    fn models(&self) -> Vec<String> {
        vec![models::DEFAULT_MODEL.into()]
    }
    fn submit(&mut self, _: &Value) -> Result<String> {
        self.submits.set(self.submits.get() + 1);
        if self.options.submit_error {
            Err(Error("ambiguous"))
        } else {
            Ok("provider-ref".into())
        }
    }
    fn poll(&mut self, _: &str, _: &Value) -> Result<String> {
        Ok(if self.options.failed {
            "FAILED"
        } else if self.options.pending {
            "IN_PROGRESS"
        } else {
            "COMPLETED"
        }
        .into())
    }
    fn result(&mut self, _: &str, _: &Value) -> Result<Output> {
        Ok(Output {
            media: b"synthetic media".to_vec(),
            response_hash: protocol::sha("response"),
        })
    }
}
fn fixture(options: Options, patch: Value) -> (Worker<Handle, Handle>, Rc<Fixture>) {
    let spec = models::spec(&json!({"prompt":"A red boat","seed":42})).unwrap();
    let mut work = json!({"minerId":"fixture-miner","jobId":"job","attemptId":"attempt","state":"offered","spec":spec,"inputHash":protocol::digest(&spec).unwrap(),"deadline":10000});
    for (k, v) in patch.as_object().unwrap() {
        work[k] = v.clone();
    }
    let f = Rc::new(Fixture {
        key: Keys::default(),
        work,
        clock: Cell::new(1000),
        worker_key: RefCell::default(),
        encryption_key: RefCell::default(),
        calls: RefCell::default(),
        receipts: RefCell::default(),
        submits: Cell::new(0),
        submitted: Cell::new(0),
        complete: Cell::new(0),
        options,
    });
    (
        Worker {
            miner_id: "fixture-miner".into(),
            coordinator_key: f.key.signing_key(),
            chain: Value::Null,
            keys: Keys::default(),
            session: Value::Null,
            runtime: Handle(f.clone()),
            provider: Handle(f.clone()),
        },
        f,
    )
}
#[test]
fn completes_with_bound_signed_receipt() {
    let (mut w, f) = fixture(Options::default(), json!({}));
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(f.submits.get(), 1);
    let receipts = f.receipts.borrow();
    let r = &receipts[0]["receipt"]["value"];
    for (k, v) in [
        ("minerId", json!("fixture-miner")),
        ("grantNonce", json!("grant")),
        ("providerRef", json!("provider-ref")),
        ("inputHash", f.work["inputHash"].clone()),
        ("bytes", json!(15)),
        ("outputHash", json!(protocol::sha("synthetic media"))),
    ] {
        assert_eq!(r[k], v, "{k}");
    }
}
#[test]
fn ambiguous_submit_is_not_retried() {
    let (mut w, f) = fixture(
        Options {
            submit_error: true,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "unknown");
    assert_eq!(f.submits.get(), 1);
    assert!(f.calls.borrow().iter().any(|s| s == "unknown"));
}
#[test]
fn longer_work_preserves_length_in_signed_receipt_without_duplicate_submission() {
    for duration in [10, 15] {
        let spec =
            models::spec(&json!({"prompt":"A cinematic journey","seed":42,"duration":duration}))
                .unwrap();
        let hash = protocol::digest(&spec).unwrap();
        let (mut worker, fixture) =
            fixture(Options::default(), json!({"spec":spec,"inputHash":hash}));
        assert_eq!(worker.step().unwrap()["state"], "accepted");
        assert_eq!(fixture.submits.get(), 1);
        assert_eq!(
            fixture.receipts.borrow()[0]["receipt"]["value"]["inputHash"],
            hash
        );
    }
}
#[test]
fn starting_and_unknown_never_submit() {
    for state in ["starting", "unknown"] {
        let (mut w, f) = fixture(Options::default(), json!({"state":state}));
        assert_eq!(w.step().unwrap()["state"], "unknown");
        assert_eq!(f.submits.get(), 0);
    }
}
#[test]
fn submitted_work_resumes_without_another_charge() {
    let (mut w, f) = fixture(
        Options::default(),
        json!({"state":"submitted","providerRef":"old-ref","grantNonce":"old-grant"}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(f.submits.get(), 0);
    assert_eq!(
        f.receipts.borrow()[0]["receipt"]["value"]["providerRef"],
        "old-ref"
    );
}
#[test]
fn metadata_and_completion_retries_are_idempotent() {
    let (mut w, f) = fixture(
        Options {
            submitted_failures: 2,
            complete_failures: 2,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(f.submits.get(), 1);
    assert_eq!(f.submitted.get(), 3);
    let r = f.receipts.borrow();
    assert_eq!(r.len(), 3);
    assert_eq!(r[0], r[1]);
    assert_eq!(r[1], r[2]);
}
#[test]
fn denied_and_invalid_grants_never_charge() {
    let (mut w, f) = fixture(
        Options {
            denied: true,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "reconcile");
    assert_eq!(f.submits.get(), 0);
    let (mut w, f) = fixture(
        Options {
            bad_grant: true,
            ..Default::default()
        },
        json!({}),
    );
    assert!(w.step().is_err());
    assert_eq!(f.submits.get(), 0);
}
#[test]
fn replay_chain_and_assignment_mismatches_fail_closed() {
    for o in [
        Options {
            replay: true,
            ..Default::default()
        },
        Options {
            bad_chain: true,
            ..Default::default()
        },
    ] {
        let (mut w, f) = fixture(o, json!({}));
        assert!(w.step().is_err());
        assert_eq!(f.submits.get(), 0);
    }
    for p in [
        json!({"minerId":"other"}),
        json!({"inputHash":"wrong"}),
        json!({"deadline":999}),
    ] {
        let (mut w, f) = fixture(Options::default(), p);
        assert!(w.step().is_err());
        assert_eq!(f.submits.get(), 0);
    }
}
#[test]
fn provider_failure_and_deadline_stop_polling() {
    for (o, state) in [
        (
            Options {
                failed: true,
                ..Default::default()
            },
            "failed",
        ),
        (
            Options {
                pending: true,
                ..Default::default()
            },
            "deadline_exceeded",
        ),
    ] {
        let (mut w, f) = fixture(o, json!({}));
        assert_eq!(w.step().unwrap()["state"], state);
        assert_eq!(f.submits.get(), 1);
        assert!(f.receipts.borrow().is_empty());
    }
}
