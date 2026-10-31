use everyframe_miner::{
    Result, balances,
    key_check::{self, Verdict},
    models,
    network::{Http, Request, safe_url},
    protocol::sha,
    providers::{Provider, Router},
    studio_video, submission,
};
use serde_json::{Value, json};
use std::cell::RefCell;

const MODEL: &str = "studio/pika/pika-h3-flex";
struct Api {
    reply: Value,
    calls: RefCell<Vec<Request>>,
    slow_reference: bool,
}
impl Api {
    fn new(reply: Value) -> Self {
        Self {
            reply,
            calls: RefCell::new(vec![]),
            slow_reference: false,
        }
    }
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts)?;
        let url = url::Url::parse(&r.url).unwrap();
        let host = url.host_str().unwrap().to_string();
        if host == "api.dev.pika.art" && !url.path().starts_with("/catalog/") {
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "X-API-Key" && v == "pika-fixture-secret")
            );
        } else {
            assert!(
                r.headers.is_empty(),
                "credentials cannot reach references or CDN"
            );
        }
        assert!(r.redirect_hosts.is_empty());
        self.calls.borrow_mut().push(r);
        if host == "api.everyframe.studio" {
            if self.slow_reference {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            return Ok(b"fixture".to_vec());
        }
        if host == "cdn.pika.art" {
            return Ok(b"output-video-fixture".to_vec());
        }
        Ok(self.reply.to_string().into_bytes())
    }
}
fn spec() -> Value {
    models::spec(
        &json!({"model":MODEL,"prompt":"A sailboat at sunrise","seed":42,"duration":5,
        "resolution":"768p","aspect":"16:9","references":[],"promptExpansion":"balanced"}),
    )
    .unwrap()
}
fn keys() -> Value {
    json!({"PIKA_API_KEY":"pika-fixture-secret"})
}
fn asset(role: &str) -> Value {
    let kind = if ["first", "last"].contains(&role) {
        "image"
    } else {
        role
    };
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_{role}?t={}","a".repeat(32)),
        "sha256":sha(b"fixture"),"bytes":7,"mime":match kind { "video"=>"video/mp4","audio"=>"audio/mpeg",_=>"image/png" },
        "role":role,"width":if kind == "audio" {0} else {1280},"height":if kind == "audio" {0} else {720},
        "durationMs":if kind == "image" {0} else {2000}})
}
fn job(state: &str) -> Value {
    json!({"id":"media_job123","status":state,
    "model_id":"pika/pika-h3-flex/text-to-video"})
}

#[test]
fn text_keyframes_and_references_map_to_exact_pika_routes() {
    let mut s = spec();
    let (path, b) = studio_video::body(&s, &[]).unwrap();
    assert_eq!(path, "/v1/media/pika/pika-h3-flex/text-to-video");
    assert_eq!(
        b,
        json!({"prompt":s["input"]["prompt"],"seed":42,"duration":5,"aspect_ratio":"16:9",
        "resolution":"768p","rewrite":true,"output_mode":"video"})
    );
    s["input"]["references"] = json!([asset("first"), asset("last")]);
    s["input"]["promptExpansion"] = json!("disabled");
    let (path, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/v1/media/pika/pika-h3-flex/image-to-video");
    assert_eq!(b["image"], "first");
    assert_eq!(b["last_frame_image"], "last");
    assert_eq!(b["rewrite"], false);
    s["input"]["references"] = json!([asset("audio"), asset("image"), asset("video")]);
    let (path, b) =
        studio_video::body(&s, &["sound".into(), "image".into(), "video".into()]).unwrap();
    assert_eq!(path, "/v1/media/pika/pika-h3-flex/reference-to-video");
    assert_eq!(
        b["references"],
        json!([{"type":"audio","url":"sound"},{"type":"image","url":"image"},{"type":"video","url":"video"}])
    );
    assert!(b.get("webhook_url").is_none());
}

#[test]
fn flexible_contract_rejects_unpriced_or_unsafe_input_before_any_submission() {
    for resolution in ["480p", "768p", "1080p", "2k", "4k"] {
        for duration in 5..=15 {
            let mut s = spec();
            s["input"]["resolution"] = json!(resolution);
            s["input"]["duration"] = json!(duration);
            models::check(&s).unwrap();
        }
    }
    for (field, v) in [
        ("duration", json!(16)),
        ("resolution", json!("8k")),
        ("aspect", json!("auto")),
        ("webhook_url", json!("https://attacker.test")),
        ("output_mode", json!("audio-stems")),
    ] {
        let mut s = spec();
        s["input"][field] = v;
        assert!(models::check(&s).is_err());
    }
    for refs in [
        vec![asset("audio")],
        vec![asset("first"), asset("image")],
        vec![asset("last")],
        vec![asset("image"); 10],
    ] {
        let mut s = spec();
        s["input"]["references"] = json!(refs);
        assert!(models::check(&s).is_err());
    }
    for (field, v) in [
        ("bytes", json!(24 * 1024 * 1024 + 1)),
        ("durationMs", json!(1999)),
        ("height", json!(16000)),
        ("url", json!("https://attacker.test/reference.mp4")),
    ] {
        let mut a = asset("video");
        a[field] = v;
        let mut s = spec();
        s["input"]["references"] = json!([a]);
        assert!(models::check(&s).is_err());
    }
    let mut s = spec();
    let mut a = asset("video");
    a["durationMs"] = json!(8000);
    s["input"]["references"] = json!([a.clone(), a]);
    assert!(models::check(&s).is_err());
}

#[test]
fn verified_references_and_deadline_are_checked_before_the_paid_post() {
    let mut s = spec();
    s["input"]["references"] = json!([asset("first")]);
    let http = Api::new(job("queued"));
    let r = Router::new(&http, keys())
        .submit_before(&s, everyframe_miner::now() + 1000)
        .unwrap();
    assert_eq!(r, "pika_media_job123");
    let calls = http.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[1].method, "POST");
    assert!(calls[1].submit_deadline.is_some());
    drop(calls);
    s["input"]["references"][0]["sha256"] = json!("0".repeat(64));
    let http = Api::new(job("queued"));
    assert_eq!(
        Router::new(&http, keys()).submit(&s).unwrap_err().0,
        "reference_integrity_mismatch"
    );
    assert_eq!(http.calls.borrow().len(), 1);
    s["input"]["references"][0] = asset("first");
    let mut http = Api::new(job("queued"));
    http.slow_reference = true;
    assert_eq!(
        Router::new(&http, keys())
            .submit_before(&s, everyframe_miner::now() + 5)
            .unwrap_err()
            .0,
        "submission_deadline_elapsed"
    );
    assert_eq!(http.calls.borrow().len(), 1);
}

#[test]
fn polling_is_bound_and_failures_are_terminal_without_exposing_messages() {
    for (status, expected) in [
        ("queued", "IN_QUEUE"),
        ("running", "IN_PROGRESS"),
        ("completed", "COMPLETED"),
    ] {
        let http = Api::new(job(status));
        assert_eq!(
            Router::new(&http, keys())
                .poll("pika_media_job123", &spec())
                .unwrap(),
            expected
        );
        assert_eq!(
            http.calls.borrow()[0].url,
            "https://api.dev.pika.art/v1/media/jobs/media_job123"
        );
    }
    for (code, expected) in [
        ("content_moderation", "provider_content_rejected"),
        ("provider_timeout", "provider_generation_failed"),
        ("insufficient_balance", "provider_billing_required"),
        ("invalid_input", "provider_invalid_request"),
    ] {
        let mut j = job("failed");
        j["error"] = json!({"code":code,"message":"secret fixture says insufficient balance"});
        let http = Api::new(j.clone());
        assert_eq!(
            Router::new(&http, keys())
                .poll("pika_media_job123", &spec())
                .unwrap_err()
                .0,
            expected
        );
        assert!(!http.calls.borrow().iter().any(|r| r.method == "POST"));
        if code == "insufficient_balance" {
            let http = Api::new(j);
            let e = Router::new(&http, keys()).submit(&spec()).unwrap_err();
            assert_eq!(submission::rejected(e.0), Some("provider_billing_required"));
        }
    }
    for (k, v) in [
        ("id", json!("foreign")),
        ("model_id", json!("foreign/model")),
        ("status", json!("unknown")),
    ] {
        let mut j = job("completed");
        j[k] = v;
        let http = Api::new(j);
        assert!(
            Router::new(&http, keys())
                .poll("pika_media_job123", &spec())
                .is_err()
        );
    }
    let http = Api::new(job("queued"));
    assert!(
        Router::new(&http, keys())
            .poll("fal_media_job123", &spec())
            .is_err()
    );
    assert!(http.calls.borrow().is_empty());
}

#[test]
fn result_download_is_credential_free_and_strictly_limited_to_pika_cdn() {
    for url in [
        "https://cdn.pika.art/output.mp4",
        "https://cdn.pika.art.attacker.test/out.mp4",
        "http://cdn.pika.art/out.mp4",
        "https://127.0.0.1/out.mp4",
    ] {
        let mut j = job("completed");
        j["output"] = json!({"media_type":"video","video":{"url":url}});
        let http = Api::new(j);
        let result = Router::new(&http, keys()).result("pika_media_job123", &spec());
        if url == "https://cdn.pika.art/output.mp4" {
            assert_eq!(result.unwrap().media, b"output-video-fixture");
            assert_eq!(http.calls.borrow().len(), 2);
        } else {
            assert!(result.is_err());
            assert_eq!(http.calls.borrow().len(), 1);
        }
    }
}

fn balance() -> Value {
    json!({"unit":"micro_usd","currency":"usd","balance_micro_usd":1_234_567,"postpaid":null})
}
#[test]
fn balance_uses_exact_spendable_prepaid_or_active_postpaid_funds() {
    let mut v = balance();
    assert_eq!(balances::pika_remaining(&v).unwrap(), 1_234_567);
    v["postpaid"] = json!({"active":true,"cycle":{"remaining_micro_usd":0}});
    assert_eq!(balances::pika_remaining(&v).unwrap(), 0);
    v["postpaid"]["active"] = json!(false);
    assert_eq!(balances::pika_remaining(&v).unwrap(), 1_234_567);
    v["postpaid"]["active"] = json!(true);
    v["postpaid"]["cycle"] = json!({});
    assert!(balances::pika_remaining(&v).is_err());
    let http = Api::new(balance());
    assert_eq!(
        key_check::check(&http, "pika", "pika-fixture-secret").0,
        Verdict::Valid
    );
    let reports = balances::worker_reports(&http, &keys());
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["provider"], "pika");
    assert_eq!(reports[0]["remainingMicrousd"], 1_234_567);
    assert!(!reports[0].to_string().contains("secret"));
    let http = Api::new(json!({"message":"API key accepted"}));
    assert_eq!(
        key_check::check(&http, "pika", "pika-fixture-secret").0,
        Verdict::Unverified
    );
}

#[test]
fn only_definitive_client_rejection_is_safe_to_fail_over() {
    let body=br#"{"id":"media_job123","status":"failed","error":{"code":"insufficient_balance","message":"secret"}}"#;
    assert_eq!(
        submission::rejected(submission::pika_http_error(403, body).0),
        Some("provider_billing_required")
    );
    for code in [408, 500, 502, 503, 504] {
        assert_eq!(
            submission::rejected(submission::pika_http_error(code, body).0),
            None
        );
    }
    assert_eq!(submission::rejected("request_timeout"), None);
    assert_eq!(
        submission::rejected(
            submission::pika_http_error(403, br#"{"message":"insufficient balance"}"#).0
        ),
        Some("provider_invalid_request")
    );
    assert_eq!(
        submission::rejected(submission::pika_error("provider_error").0),
        None
    );
}
