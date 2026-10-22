use everyframe_miner::{
    Error, Result, balances,
    key_check::{self, Verdict},
    models,
    network::{Http, Request, safe_url},
    providers::{Provider, Router},
    submission,
};
use serde_json::{Value, json};
use std::cell::RefCell;

const MODEL: &str = "direct/kling/kling-3.0";
struct Api {
    reply: Value,
    calls: RefCell<Vec<Request>>,
    fail: Option<&'static str>,
}
impl Api {
    fn new(reply: Value) -> Self {
        Self {
            reply,
            calls: RefCell::new(vec![]),
            fail: None,
        }
    }
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts)?;
        self.calls.borrow_mut().push(r);
        if let Some(code) = self.fail {
            return Err(Error(code));
        }
        Ok(serde_json::to_vec(&self.reply).unwrap())
    }
}
fn spec() -> Value {
    models::spec(&json!({"model":MODEL,"prompt":"A sailboat."})).unwrap()
}
fn credentials() -> Value {
    json!({"KLING_API_KEY":"fixture-kling-secret"})
}
fn task(status: &str) -> Value {
    json!({"code":0,"data":[{"id":"job123","status":status}]})
}

#[test]
fn direct_contract_is_bounded_and_cannot_be_used_as_arbitrary_provider_proxy() {
    for d in 3..=15 {
        let s = models::spec(&json!({"model":MODEL,"prompt":"A sailboat.","duration":d})).unwrap();
        assert_eq!(s["input"]["duration"], d);
        models::check(&s).unwrap();
    }
    for d in [json!(2), json!(16), json!(3.5), json!("5")] {
        assert!(models::spec(&json!({"model":MODEL,"prompt":"A sailboat.","duration":d})).is_err());
    }
    assert!(models::spec(&json!({"model":MODEL,"prompt":"x".repeat(3073)})).is_err());
    for field in [
        "callback_url",
        "url",
        "model",
        "resolution",
        "audio",
        "multi_shot",
    ] {
        let mut bad = spec();
        bad["input"][field] = json!("unapproved");
        assert!(models::check(&bad).is_err());
    }
}

#[test]
fn status_is_terminal_and_bound_to_exact_task() {
    for (status, expected) in [
        ("submitted", "IN_QUEUE"),
        ("processing", "IN_PROGRESS"),
        ("succeeded", "COMPLETED"),
    ] {
        let http = Api::new(task(status));
        assert_eq!(
            Router::new(&http, credentials())
                .poll("kling_job123", &spec())
                .unwrap(),
            expected
        );
        let calls = http.calls.borrow();
        assert_eq!(calls[0].method, "GET");
        assert_eq!(
            calls[0].url,
            "https://api-singapore.klingai.com/tasks?task_ids=job123"
        );
    }
    let http = Api::new(task("failed"));
    assert_eq!(
        Router::new(&http, credentials())
            .poll("kling_job123", &spec())
            .unwrap_err()
            .0,
        "provider_generation_failed"
    );
    for bad in [
        json!({"code":0,"data":[]}),
        json!({"code":0,"data":[{"id":"foreign","status":"succeeded"}]}),
        task("mystery"),
        json!({"data":[]}),
    ] {
        let http = Api::new(bad);
        assert!(
            Router::new(&http, credentials())
                .poll("kling_job123", &spec())
                .is_err()
        );
    }
}

#[test]
fn output_is_single_video_and_cdn_receives_no_credentials() {
    for url in [
        "https://evil.invalid/out.mp4",
        "https://127.0.0.1/out.mp4",
        "https://p1-kling.klingai.com.evil.invalid/out.mp4",
    ] {
        let mut t = task("succeeded");
        t["data"][0]["outputs"] = json!([{"type":"video","url":url}]);
        let http = Api::new(t);
        assert!(
            Router::new(&http, credentials())
                .result("kling_job123", &spec())
                .is_err()
        );
        assert_eq!(http.calls.borrow().len(), 1);
    }
    for outputs in [
        json!([]),
        json!([{"type":"image","url":"https://p1-kling.klingai.com/a"}]),
        json!([{}, {}]),
    ] {
        let mut t = task("succeeded");
        t["data"][0]["outputs"] = outputs;
        let http = Api::new(t);
        assert!(
            Router::new(&http, credentials())
                .result("kling_job123", &spec())
                .is_err()
        );
        assert_eq!(http.calls.borrow().len(), 1);
    }
    let mut t = task("succeeded");
    t["data"][0]["outputs"] =
        json!([{"type":"video","url":"https://p1-kling.klingai.com/video.mp4"}]);
    let http = Api::new(t);
    Router::new(&http, credentials())
        .result("kling_job123", &spec())
        .unwrap();
    let calls = http.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].headers.is_empty());
    assert!(calls[1].redirect_hosts.is_empty());
}

#[test]
fn deadline_missing_key_and_ambiguous_submit_do_not_create_duplicate_charges() {
    let http = Api::new(json!({"code":0,"data":{"id":"job123"}}));
    assert_eq!(
        Router::new(&http, credentials())
            .submit_before(&spec(), 0)
            .unwrap_err()
            .0,
        "submission_deadline_elapsed"
    );
    assert!(Router::new(&http, json!({})).submit(&spec()).is_err());
    assert!(http.calls.borrow().is_empty());
    let mut http = Api::new(json!({}));
    http.fail = Some("request_timeout");
    assert_eq!(
        Router::new(&http, credentials())
            .submit(&spec())
            .unwrap_err()
            .0,
        "request_timeout"
    );
    assert_eq!(http.calls.borrow().len(), 1);
    assert_eq!(submission::rejected("request_timeout"), None);
    for code in [1101, 1102] {
        let body = serde_json::to_vec(&json!({"code":code,"message":"secret"})).unwrap();
        assert_eq!(
            submission::rejected(submission::kling_http_error(429, &body).0),
            Some("provider_billing_required")
        );
        assert_eq!(
            submission::rejected(submission::kling_http_error(503, &body).0),
            None
        );
        assert_eq!(
            submission::rejected(submission::kling_http_error(408, &body).0),
            None
        );
    }
    assert_eq!(
        submission::rejected(submission::kling_error(1301).0),
        Some("provider_content_rejected")
    );
    assert_eq!(submission::rejected(submission::kling_error(5000).0), None);
}

#[test]
fn key_probe_is_read_only_and_balance_never_invents_usd() {
    let empty = Api::new(json!({"code":0,"data":{"code":0,"msg":"success"}}));
    assert_eq!(
        key_check::check(&empty, "kling", "fixture-kling-secret").0,
        Verdict::Valid
    );
    for (v, want) in [
        (
            json!({"code":0,"data":{"code":0,"resource_pack_subscribe_infos":[]}}),
            Verdict::Valid,
        ),
        (json!({"code":1002,"message":"secret"}), Verdict::Rejected),
        (json!({"code":0,"data":{}}), Verdict::Unverified),
    ] {
        let http = Api::new(v);
        assert_eq!(
            key_check::check(&http, "kling", "fixture-kling-secret").0,
            want
        );
        let calls = http.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert!(
            calls[0]
                .url
                .starts_with("https://api-singapore.klingai.com/account/costs?")
        );
    }
    let http = Api::new(json!({}));
    let rows = balances::collect(&http, &credentials());
    assert_eq!(rows[0]["provider"], "kling");
    assert_eq!(rows[0]["status"], "unsupported");
    assert!(rows[0]["remainingUsd"].is_null());
    assert!(http.calls.borrow().is_empty());
    assert!(
        rows[0]["diagnostic"]["next"]
            .as_str()
            .unwrap()
            .contains("12 hours")
    );
}
