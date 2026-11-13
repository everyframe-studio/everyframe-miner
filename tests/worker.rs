use everyframe_miner::{
    Error, Result, models,
    protocol::{self, Keys},
    providers::{Output, Provider},
    worker::{Runtime, Worker, complete_timeout_secs},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Options {
    submit_error: bool,
    budget_policy: bool,
    media_policy: bool,
    budget_report: Option<Value>,
    submit_code: Option<&'static str>,
    report_failures: u32,
    failed_report_failures: u32,
    failed_report_error: Option<&'static str>,
    denied: bool,
    bad_grant: bool,
    replay: bool,
    submitted_failures: u32,
    complete_failures: u32,
    complete_error: Option<&'static str>,
    failed: bool,
    poll_error: Option<&'static str>,
    result_error: Option<&'static str>,
    pending: bool,
    bad_chain: bool,
    threaded_heartbeat: bool,
    blocking_ms: u64,
    retired_models: Option<Value>,
    /// Refusal tag on submit_code / poll_error failures.
    refusal: Option<&'static str>,
    /// Behave like a coordinator without refusal support.
    refuse_refusal_field: bool,
    provider_message: Option<&'static str>,
    refuse_provider_error_field: bool,
}
struct Fixture {
    key: Keys,
    work: Value,
    clock: Cell<i64>,
    worker_key: RefCell<String>,
    encryption_key: RefCell<String>,
    calls: RefCell<Vec<String>>,
    receipts: RefCell<Vec<Value>>,
    failure_reports: RefCell<Vec<Value>>,
    rejections: RefCell<Vec<Value>>,
    submits: Cell<u32>,
    submitted: Cell<u32>,
    reports: Cell<u32>,
    complete: Cell<u32>,
    claim_models: RefCell<Vec<Value>>,
    retirements: RefCell<Vec<Value>>,
    media_policies: RefCell<Vec<Value>>,
    heartbeats: std::sync::Arc<std::sync::atomic::AtomicUsize>,
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
    fn keepalive(
        &self,
        _: &Value,
        _: &Keys,
        _: &str,
        _: i64,
    ) -> Result<everyframe_miner::worker_heartbeat::Guard> {
        if !self.options.threaded_heartbeat {
            return Ok(Default::default());
        }
        let count = self.heartbeats.clone();
        everyframe_miner::worker_heartbeat::Guard::spawn(
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(2),
            move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        )
    }
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
                let mut reply = json!({"sessionId":"session","challengeId":"challenge","signingKey":body["signingKey"],"expires":9999999});
                if self.options.media_policy {
                    reply["mediaPolicy"] = json!(1);
                }
                if self.options.budget_policy {
                    reply["providerBudgetPolicy"] = json!(1);
                }
                reply
            }
            "/v1/action" => {
                let request = protocol::verified(body, &self.worker_key.borrow())?;
                let action = request["action"].as_str().unwrap();
                self.calls.borrow_mut().push(action.into());
                let result = match action {
                    "media_policy" => json!({"mediaPolicy":{"revision":2}}),
                    "claim" => {
                        assert_eq!(request["payload"]["durationPolicy"], 1);
                        assert_eq!(
                            request["payload"].get("providerBudget"),
                            if self.options.budget_policy {
                                self.options.budget_report.as_ref()
                            } else {
                                None
                            }
                        );
                        self.claim_models
                            .borrow_mut()
                            .push(request["payload"]["models"].clone());
                        let mut reply = json!({"work":protocol::seal(&self.work,&self.encryption_key.borrow(),&format!("session:{}",request["nonce"].as_str().unwrap()))?});
                        if self.options.media_policy {
                            reply["mediaPolicy"] = json!({"revision":1});
                        }
                        if let Some(retired) = &self.options.retired_models {
                            reply["retiredModels"] = retired.clone();
                        }
                        reply
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
                        std::thread::sleep(std::time::Duration::from_millis(
                            self.options.blocking_ms,
                        ));
                        let receipt = protocol::verified(
                            &request["payload"]["receipt"],
                            &self.worker_key.borrow(),
                        )?;
                        self.receipts.borrow_mut().push(request["payload"].clone());
                        self.complete.set(self.complete.get() + 1);
                        if let Some(code) = self.options.complete_error {
                            return Err(Error(code));
                        }
                        if self.complete.get() <= self.options.complete_failures {
                            return Err(Error("lost_ack"));
                        }
                        json!({"jobId":receipt["jobId"],"outputHash":receipt["outputHash"]})
                    }
                    "submission_rejected" | "submission_uncertain" => {
                        self.rejections
                            .borrow_mut()
                            .push(request["payload"].clone());
                        if self.options.refuse_provider_error_field
                            && request["payload"].get("providerError").is_some()
                        {
                            return Err(Error("http_400"));
                        }
                        if self.options.refuse_refusal_field
                            && request["payload"].get("refusal").is_some()
                        {
                            return Err(Error("http_400"));
                        }
                        self.reports.set(self.reports.get() + 1);
                        if self.reports.get() <= self.options.report_failures {
                            return Err(Error("lost_ack"));
                        }
                        assert!(request["payload"]["reason"].is_string());
                        json!({"ok":true})
                    }
                    "failed" => {
                        let mut reports = self.failure_reports.borrow_mut();
                        reports.push(request["payload"].clone());
                        if self.options.refuse_provider_error_field
                            && request["payload"].get("providerError").is_some()
                        {
                            return Err(Error("http_400"));
                        }
                        if self.options.refuse_refusal_field
                            && request["payload"].get("refusal").is_some()
                        {
                            return Err(Error("http_400"));
                        }
                        if reports.len() <= self.options.failed_report_failures as usize {
                            return Err(Error(
                                self.options
                                    .failed_report_error
                                    .unwrap_or("request_timeout"),
                            ));
                        }
                        json!({})
                    }
                    "unknown" | "heartbeat" => json!({}),
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
    fn set_media_policy(&mut self, policy: &Value, _: &Value, _: i64) -> Result<()> {
        self.media_policies.borrow_mut().push(policy.clone());
        Ok(())
    }
    fn provider_budget(&self) -> Option<Value> {
        self.options.budget_report.clone()
    }
    fn models(&self) -> Vec<String> {
        vec![models::DEFAULT_MODEL.into()]
    }
    fn set_retirements(&mut self, retired: &Value) {
        self.retirements.borrow_mut().push(retired.clone());
    }
    fn submit(&mut self, _: &Value) -> Result<String> {
        std::thread::sleep(std::time::Duration::from_millis(self.options.blocking_ms));
        self.submits.set(self.submits.get() + 1);
        if let Some(code) = self.options.submit_code {
            Err(Error(code)
                .with_refusal(self.options.refusal)
                .with_provider_error(self.options.provider_message.and_then(|message| {
                    everyframe_miner::provider_error::extract(
                        &json!({"code":"NEW_PROVIDER_CODE","message":message}),
                        &[],
                    )
                })))
        } else if self.options.submit_error {
            Err(Error("ambiguous"))
        } else {
            Ok("provider-ref".into())
        }
    }
    fn poll(&mut self, _: &str, _: &Value) -> Result<String> {
        std::thread::sleep(std::time::Duration::from_millis(self.options.blocking_ms));
        if let Some(code) = self.options.poll_error {
            return Err(Error(code)
                .with_refusal(self.options.refusal)
                .with_provider_error(self.options.provider_message.and_then(|message| {
                    everyframe_miner::provider_error::extract(
                        &json!({"code":"NEW_PROVIDER_CODE","message":message}),
                        &[],
                    )
                })));
        }
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
        std::thread::sleep(std::time::Duration::from_millis(self.options.blocking_ms));
        if let Some(code) = self.options.result_error {
            if code == "unapproved_destination" {
                return everyframe_miner::network::safe_url(
                    "https://output.example.net/private/SECRET_PATH?token=SECRET_QUERY",
                    &["static.wavespeed.ai".into()],
                )
                .map(|_| unreachable!());
            }
            return Err(Error(code));
        }
        Ok(Output {
            media: b"synthetic media".to_vec(),
            response_hash: protocol::sha("response"),
        })
    }
}

#[test]
fn terminal_poll_and_result_failures_release_slot_without_waiting_for_deadline() {
    for options in [
        Options {
            poll_error: Some("http_422"),
            ..Default::default()
        },
        Options {
            result_error: Some("provider_no_media"),
            ..Default::default()
        },
        Options {
            result_error: Some("http_422"),
            ..Default::default()
        },
    ] {
        let (mut w, f) = fixture(options, json!({}));
        assert_eq!(w.step().unwrap()["state"], "failed");
        assert_eq!(f.submits.get(), 1);
        assert!(f.clock.get() < 2000);
        assert_eq!(
            f.calls
                .borrow()
                .iter()
                .filter(|a| a.as_str() == "failed")
                .count(),
            1
        );
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
        failure_reports: RefCell::default(),
        rejections: RefCell::default(),
        submits: Cell::new(0),
        submitted: Cell::new(0),
        reports: Cell::new(0),
        complete: Cell::new(0),
        claim_models: RefCell::default(),
        retirements: RefCell::default(),
        media_policies: RefCell::default(),
        heartbeats: Default::default(),
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
            provider_pause: Default::default(),
        },
        f,
    )
}
#[test]
fn failed_reports_preserve_reason_across_transport_and_protocol_errors() {
    for error in [
        "request_timeout",
        "connection_failed",
        "http_500",
        "response_replayed",
        "http_400",
    ] {
        let (mut w, f) = fixture(
            Options {
                poll_error: Some("provider_content_rejected"),
                failed_report_failures: 1,
                failed_report_error: Some(error),
                ..Default::default()
            },
            json!({}),
        );
        assert_eq!(w.step().unwrap()["state"], "failed");
        let reports = f.failure_reports.borrow();
        assert_eq!(reports.len(), 2, "{error}");
        assert_eq!(
            reports[0],
            json!({"attemptId":"attempt", "reason":"provider_content_rejected"})
        );
        assert_eq!(
            reports[1], reports[0],
            "{error} must not trigger a reasonless downgrade"
        );
        assert_eq!(f.submits.get(), 1);
        assert_eq!(f.clock.get(), 4000, "retry must back off");
    }
}

#[test]
fn failed_reports_reconcile_at_deadline_without_losing_reason_or_resubmitting() {
    let (mut w, f) = fixture(
        Options {
            poll_error: Some("provider_content_rejected"),
            failed_report_failures: u32::MAX,
            failed_report_error: Some("http_400"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "reconcile");
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.len(), 3);
    assert!(
        reports
            .iter()
            .all(|p| p["reason"] == "provider_content_rejected")
    );
    assert_eq!(f.submits.get(), 1);
    assert_eq!(f.clock.get(), 10000);
}

#[test]
fn terminal_provider_aliases_reach_signed_reports() {
    for (code, category) in [
        ("vivix_content_rejected", "provider_content_rejected"),
        ("vivix_invalid_parameters", "provider_invalid_request"),
        ("fal_billing_required", "provider_billing_required"),
        ("vivix_billing_required", "provider_billing_required"),
        ("http_422", "provider_invalid_request"),
    ] {
        for result_error in [false, true] {
            let options = Options {
                poll_error: (!result_error).then_some(code),
                result_error: result_error.then_some(code),
                ..Default::default()
            };
            let (mut w, f) = fixture(options, json!({}));
            assert_eq!(w.step().unwrap()["state"], "failed");
            assert_eq!(
                *f.failure_reports.borrow(),
                vec![json!({"attemptId":"attempt", "reason":category})]
            );
            assert_eq!(f.submits.get(), 1);
        }
    }
}
#[test]
fn signed_claim_reply_passes_coordinator_retirements_to_the_provider() {
    let (mut w, f) = fixture(Options::default(), json!({}));
    w.step().unwrap();
    assert!(
        f.retirements.borrow().is_empty(),
        "older coordinators send none"
    );
    let list = json!({"fal":["fal-ai/ltx-2/text-to-video"]});
    let (mut w, f) = fixture(
        Options {
            retired_models: Some(list.clone()),
            ..Options::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(*f.retirements.borrow(), vec![list]);
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
    assert!(f.calls.borrow().iter().any(|s| s == "submission_uncertain"));
}
#[test]
fn definitive_rejection_reports_immediately_without_resubmitting() {
    for code in [
        "http_401",
        "http_402",
        "http_429",
        "vivix_billing_required",
        "vivix_content_rejected",
        "vivix_invalid_parameters",
    ] {
        let (mut w, f) = fixture(
            Options {
                submit_code: Some(code),
                report_failures: 2,
                ..Default::default()
            },
            json!({}),
        );
        assert_eq!(w.step().unwrap()["state"], "rejected");
        assert_eq!(f.submits.get(), 1);
        assert_eq!(f.reports.get(), 3);
        assert_eq!(f.clock.get(), 3000);
    }
}
#[test]
fn ambiguous_provider_responses_preserve_uncertainty() {
    for code in [
        "http_408",
        "http_409",
        "http_500",
        "request_timeout",
        "invalid_provider_ref",
    ] {
        let (mut w, f) = fixture(
            Options {
                submit_code: Some(code),
                ..Default::default()
            },
            json!({}),
        );
        assert_eq!(w.step().unwrap()["state"], "unknown");
        assert_eq!(f.submits.get(), 1);
    }
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

#[test]
fn an_output_the_coordinator_refuses_is_reported_instead_of_resent_until_the_deadline() {
    let (mut w, f) = fixture(
        Options {
            complete_error: Some("http_400"),
            ..Default::default()
        },
        json!({}),
    );
    let out = w.step().unwrap();
    assert_eq!(out["state"], "failed");
    assert_eq!(f.complete.get(), 2, "one retry, then report");
    assert!(f.clock.get() < 5000, "well before the 10 s deadline");
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.last().unwrap()["reason"], "output_invalid");
}

#[test]
fn a_transient_upload_error_is_still_retried() {
    let (mut w, f) = fixture(
        Options {
            complete_failures: 3,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(f.complete.get(), 4);
}

#[test]
fn an_attempt_that_is_no_longer_ours_stops_uploading() {
    let (mut w, f) = fixture(
        Options {
            complete_error: Some("http_409"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "reconcile");
    assert_eq!(f.complete.get(), 1);
}

#[test]
fn a_billing_refusal_pauses_that_providers_models_for_fifteen_minutes() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("http_402"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "rejected");
    let offered = f.claim_models.borrow()[0].as_array().unwrap().len();
    assert!(offered > 0);
    w.claim().unwrap();
    let model = f.work["spec"]["model"].as_str().unwrap();
    let provider = models::info(model).unwrap()["provider"].as_str().unwrap();
    let paused = f.claim_models.borrow()[1].clone();
    assert!(
        paused
            .as_array()
            .unwrap()
            .iter()
            .all(|m| models::info(m.as_str().unwrap()).unwrap()["provider"] != provider),
        "no {provider} model is offered while paused"
    );
    f.clock.set(f.clock.get() + 15 * 60 * 1000 + 1);
    // The fixture's job is past its deadline by now; only the offer list matters.
    let _ = w.claim();
    assert_eq!(
        f.claim_models.borrow()[2].as_array().unwrap().len(),
        offered
    );
}

#[test]
fn unusable_bfl_response_stops_paid_work_until_operator_intervention() {
    for reason in ["bfl_polling_url_rejected", "bfl_delivery_url_rejected"] {
        let spec =
            models::spec(&json!({"model":"studio/native/bfl/flux-3-image","prompt":"A river"}))
                .unwrap();
        let (mut worker, f) = fixture(
            Options {
                poll_error: Some(reason),
                ..Default::default()
            },
            json!({"spec":spec,"inputHash":protocol::digest(&spec).unwrap()}),
        );
        assert_eq!(worker.step().unwrap()["state"], "failed");
        assert_eq!(f.submits.get(), 1);
        assert_eq!(
            f.submitted.get(),
            1,
            "accepted task reference is recorded before polling"
        );
        assert_eq!(worker.provider_pause.get("bfl"), Some(&i64::MAX));
        assert_eq!(
            f.failure_reports.borrow().last().unwrap()["reason"],
            "provider_unavailable"
        );
    }
}

#[test]
fn upload_timeout_grows_with_the_output() {
    assert_eq!(complete_timeout_secs(0), 180);
    assert_eq!(complete_timeout_secs(100_000_000), 520);
    assert_eq!(complete_timeout_secs(1_000_000_000), 900);
}

#[test]
fn heartbeats_survive_blocking_submit_poll_download_and_upload_then_stop() {
    let (mut w, f) = fixture(
        Options {
            threaded_heartbeat: true,
            blocking_ms: 60,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    let count = f.heartbeats.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        count >= 4,
        "must heartbeat independently of the blocked worker"
    );
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert_eq!(
        f.heartbeats.load(std::sync::atomic::Ordering::SeqCst),
        count,
        "guard must stop at terminal result"
    );
    assert_eq!(f.submits.get(), 1);
}

#[test]
fn account_refusals_during_poll_and_result_pause_the_provider_with_the_correct_reason() {
    for code in [
        "http_401",
        "vivix_auth_rejected",
        "provider_key_required",
        "http_402",
        "fal_billing_required",
    ] {
        for result in [false, true] {
            let options = if result {
                Options {
                    result_error: Some(code),
                    ..Default::default()
                }
            } else {
                Options {
                    poll_error: Some(code),
                    ..Default::default()
                }
            };
            let (mut w, f) = fixture(options, json!({}));
            assert_eq!(w.step().unwrap()["state"], "failed");
            assert_eq!(f.submits.get(), 1);
            let provider = models::info(models::DEFAULT_MODEL).unwrap()["provider"]
                .as_str()
                .unwrap();
            assert_eq!(
                w.provider_pause.get(provider).copied(),
                Some(f.clock.get() + 15 * 60 * 1000)
            );
            assert_eq!(
                f.failure_reports.borrow()[0]["reason"],
                everyframe_miner::submission::rejected(code).unwrap()
            );
        }
    }
}

#[test]
fn claim_renews_a_session_that_cannot_cover_a_long_studio_job() {
    let (mut worker, f) = fixture(Options::default(), json!({}));
    worker.enroll().unwrap();
    worker.session["expires"] = json!(f.clock.get() + 13 * 60 * 1000);
    worker.claim().unwrap();
    assert_eq!(worker.session["expires"], 9999999);
    assert_eq!(f.submits.get(), 0);
}

#[test]
fn a_rejected_submission_reports_its_refusal_code() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("http_400"),
            refusal: Some("real_person"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "rejected");
    assert_eq!(
        *f.rejections.borrow(),
        vec![
            json!({"attemptId":"attempt","reason":"provider_invalid_request","refusal":"real_person"})
        ]
    );
}

#[test]
fn an_uncertain_submission_never_claims_a_refusal() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("http_500"),
            refusal: Some("real_person"),
            ..Default::default()
        },
        json!({}),
    );
    w.step().unwrap();
    let rejections = f.rejections.borrow();
    assert!(!rejections.is_empty());
    assert!(
        rejections.iter().all(|r| r.get("refusal").is_none()),
        "{rejections:?}"
    );
}

#[test]
fn a_failed_generation_reports_its_refusal_code() {
    let (mut w, f) = fixture(
        Options {
            poll_error: Some("provider_content_rejected"),
            refusal: Some("copyright"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "failed");
    assert_eq!(
        *f.failure_reports.borrow(),
        vec![
            json!({"attemptId":"attempt","reason":"provider_content_rejected","refusal":"copyright"})
        ]
    );
}

#[test]
fn an_older_coordinator_still_gets_the_category_without_the_refusal() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("http_400"),
            refusal: Some("real_person"),
            refuse_refusal_field: true,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "rejected");
    let rejections = f.rejections.borrow();
    assert_eq!(rejections.len(), 2);
    assert_eq!(
        rejections[1],
        json!({"attemptId":"attempt","reason":"provider_invalid_request"})
    );

    let (mut w, f) = fixture(
        Options {
            poll_error: Some("provider_content_rejected"),
            refusal: Some("copyright"),
            refuse_refusal_field: true,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "failed");
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports[1],
        json!({"attemptId":"attempt","reason":"provider_content_rejected"})
    );
}

#[test]
fn unknown_provider_messages_are_signed_and_survive_report_transport_retries() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("http_422"),
            provider_message: Some("A new anchor restriction"),
            report_failures: 2,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "rejected");
    assert_eq!(f.submits.get(), 1);
    let reports = f.rejections.borrow();
    assert_eq!(reports.len(), 3);
    for report in reports.iter() {
        assert_eq!(report["reason"], "provider_invalid_request");
        assert_eq!(report["providerError"]["code"], "NEW_PROVIDER_CODE");
        assert_eq!(
            report["providerError"]["message"],
            "A new anchor restriction"
        );
    }
}
#[test]
fn failed_generations_preserve_provider_text_without_changing_failure_or_resubmitting() {
    let (mut w, f) = fixture(
        Options {
            poll_error: Some("provider_generation_failed"),
            provider_message: Some("Unknown provider policy"),
            failed_report_failures: 1,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "failed");
    assert_eq!(f.submits.get(), 1);
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.len(), 2);
    for report in reports.iter() {
        assert_eq!(
            report["providerError"]["message"],
            "Unknown provider policy"
        );
    }
}
#[test]
fn older_coordinators_can_decline_the_message_without_changing_the_failure_category() {
    for failed in [false, true] {
        let (mut w, f) = fixture(
            Options {
                submit_code: (!failed).then_some("http_422"),
                poll_error: failed.then_some("provider_generation_failed"),
                provider_message: Some("Unfamiliar rejection"),
                refuse_provider_error_field: true,
                refusal: Some("content_policy"),
                ..Default::default()
            },
            json!({}),
        );
        let _ = w.step().unwrap();
        assert_eq!(f.submits.get(), 1);
        let reports = if failed {
            f.failure_reports.borrow()
        } else {
            f.rejections.borrow()
        };
        assert_eq!(reports.len(), 2);
        assert!(reports[0].get("providerError").is_some());
        assert!(reports[1].get("providerError").is_none());
        assert_eq!(reports[0]["reason"], reports[1]["reason"]);
        assert_eq!(reports[1]["refusal"], "content_policy");
    }
}
#[test]
fn uncertain_submissions_keep_provider_diagnostics_without_claiming_a_confirmed_refusal() {
    let (mut w, f) = fixture(
        Options {
            submit_code: Some("request_timeout"),
            provider_message: Some("A provider explanation on an uncertain transport"),
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "unknown");
    for report in f.rejections.borrow().iter() {
        assert!(report.get("providerError").is_some());
        assert!(report.get("refusal").is_none());
    }
}

#[test]
fn budget_report_is_negotiated_and_budget_failures_are_backward_compatible() {
    for supported in [false, true] {
        let (mut w, f) = fixture(
            Options {
                budget_policy: supported,
                budget_report: Some(json!({"provider":"openrouter","state":"blocked"})),
                submit_code: Some("provider_budget_exhausted"),
                ..Default::default()
            },
            json!({}),
        );
        let result = w.step().unwrap();
        let expected = if supported {
            "provider_budget_exhausted"
        } else {
            "provider_billing_required"
        };
        assert_eq!(result["reason"], expected);
        assert_eq!(f.rejections.borrow()[0]["reason"], expected);
        assert_eq!(f.submits.get(), 1);
        assert!(
            w.provider_pause.is_empty(),
            "The verified budget cache governs recovery, not a fixed 15-minute timer"
        );
    }
}
#[test]
fn post_submission_budget_failure_is_terminal_on_both_protocol_versions() {
    for supported in [false, true] {
        let (mut w, f) = fixture(
            Options {
                budget_policy: supported,
                poll_error: Some("provider_budget_exhausted"),
                ..Default::default()
            },
            json!({}),
        );
        assert_eq!(w.step().unwrap()["state"], "failed");
        assert_eq!(
            f.failure_reports.borrow()[0]["reason"],
            if supported {
                "provider_budget_exhausted"
            } else {
                "provider_billing_required"
            }
        );
        assert_eq!(f.submits.get(), 1);
        assert!(f.rejections.borrow().is_empty());
    }
}

#[test]
fn signed_failure_keeps_the_exact_output_error_when_debugging_is_advertised() {
    let (mut w, f) = fixture(
        Options {
            result_error: Some("untrusted_destination"),
            ..Default::default()
        },
        json!({}),
    );
    w.enroll().unwrap();
    w.session["workerDiagnostics"] = json!(1);
    assert_eq!(w.step().unwrap()["state"], "failed");
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["reason"], "provider_generation_failed");
    assert_eq!(
        reports[0]["workerDiagnostic"]["code"],
        "untrusted_destination"
    );
    assert_eq!(reports[0]["workerDiagnostic"]["phase"], "output_retrieval");
    assert_eq!(reports[0]["workerDiagnostic"]["providerCompleted"], true);
    assert_eq!(f.submits.get(), 1);
}

#[test]
fn debug_metadata_never_replays_a_provider_call_when_a_legacy_server_rejects_it() {
    let (mut w, f) = fixture(
        Options {
            result_error: Some("untrusted_destination"),
            failed_report_failures: 1,
            failed_report_error: Some("http_400"),
            ..Default::default()
        },
        json!({}),
    );
    w.enroll().unwrap();
    w.session["workerDiagnostics"] = json!(1);
    assert_eq!(w.step().unwrap()["state"], "failed");
    let reports = f.failure_reports.borrow();
    assert_eq!(reports.len(), 2);
    assert!(reports[0]["workerDiagnostic"].is_object());
    assert!(reports[1]["workerDiagnostic"].is_null());
    assert_eq!(reports[0]["reason"], reports[1]["reason"]);
    assert_eq!(f.submits.get(), 1);
}

#[test]
fn signed_destination_diagnostics_are_negotiated_and_redacted_end_to_end() {
    for enabled in [false, true] {
        let (mut worker, f) = fixture(
            Options {
                result_error: Some("unapproved_destination"),
                ..Default::default()
            },
            json!({}),
        );
        worker.enroll().unwrap();
        worker.session["workerDiagnostics"] = json!(1);
        if enabled {
            worker.session["destinationDiagnostics"] = json!(1);
        }
        assert_eq!(worker.step().unwrap()["state"], "failed");
        let reports = f.failure_reports.borrow();
        assert_eq!(reports.len(), 1);
        let d = &reports[0]["workerDiagnostic"];
        if enabled {
            assert_eq!(d["destination"]["host"], "output.example.net");
            assert_eq!(d["destination"]["rule"], "host_not_allowed");
        } else {
            assert!(d.get("destination").is_none());
        }
        assert!(!d.to_string().contains("SECRET"));
        assert_eq!(f.submits.get(), 1);
    }
}

#[test]
fn detailed_failed_and_successful_jobs_retain_safe_execution_context() {
    let (mut w, f) = fixture(
        Options {
            result_error: Some("unapproved_destination"),
            ..Default::default()
        },
        json!({"jobId":"a".repeat(48),"attemptId":"b".repeat(48)}),
    );
    w.enroll().unwrap();
    w.session["executionDiagnostics"] = json!(1);
    w.session["workerDiagnostics"] = json!(1);
    w.session["destinationDiagnostics"] = json!(1);
    assert_eq!(w.step().unwrap()["state"], "failed");
    let reports = f.failure_reports.borrow();
    let ex = &reports[0]["workerDiagnostic"]["execution"];
    assert_eq!(ex["input"]["model"], models::DEFAULT_MODEL);
    assert!(ex["runtime"]["scratchFreeBytes"].is_number());
    assert!(everyframe_miner::execution_diagnostics::sanitize(ex).is_some());
    assert!(!ex.to_string().contains("A red boat"));
    assert!(everyframe_miner::execution_diagnostics::snapshot().is_none());
    drop(reports);
    let (mut w, f) = fixture(
        Options::default(),
        json!({"jobId":"c".repeat(48),"attemptId":"d".repeat(48)}),
    );
    w.enroll().unwrap();
    w.session["executionDiagnostics"] = json!(1);
    assert_eq!(w.step().unwrap()["state"], "accepted");
    let trace = everyframe_miner::worker_trace::Cache::default();
    drop(trace);
    let request = json!({"requestId":"e".repeat(48),"jobId":"c".repeat(48),"attemptId":"d".repeat(48),"inputHash":f.work["inputHash"],"expiresAt":20000});
    let reply = everyframe_miner::worker_trace::answer(&request, 1000).unwrap();
    let event = reply["trace"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["phase"] == "output_accepted")
        .unwrap();
    assert_eq!(event["diagnostic"]["execution"]["output"]["bytes"], 15);
}

#[test]
fn in_flight_job_refreshes_cdn_policy_without_repeating_generation() {
    let (mut w, f) = fixture(
        Options {
            media_policy: true,
            ..Default::default()
        },
        json!({}),
    );
    assert_eq!(w.step().unwrap()["state"], "accepted");
    assert_eq!(f.submits.get(), 1);
    assert_eq!(
        *f.media_policies.borrow(),
        vec![json!({"revision":1}), json!({"revision":2})]
    );
    let calls = f.calls.borrow();
    let refresh = calls.iter().position(|a| a == "media_policy").unwrap();
    assert!(calls.iter().position(|a| a == "submitted").unwrap() < refresh);
    assert!(refresh < calls.iter().position(|a| a == "complete").unwrap());
}
