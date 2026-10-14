mod common;

use everyframe_miner::{
    Error, Result, balances, cloud, invitation, models,
    network::{Http, Request, safe_url},
    protocol::digest,
    providers::{Provider, Router},
    state::State,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::Write,
    process::{Command, Stdio},
};

const MODEL: &str = "direct/vivix/vivix-w1";
const KEY: &str = "synthetic-vivix-private-key";

#[test]
fn worker_preflight_rejects_zero_w_without_paid_submission() {
    let api = Api::new(vec![
        json!({"code":0,"data":{"balances":[{"scope":"A","available":1000},{"scope":"W","available":0}]}}),
    ]);
    let mut router = Router::new(&api, credentials());
    assert_eq!(
        router
            .submit_before(&spec(5), everyframe_miner::now() + 60000)
            .unwrap_err()
            .0,
        "vivix_billing_required"
    );
    assert_eq!(api.calls.borrow().len(), 1);
    assert_eq!(api.calls.borrow()[0].method, "GET");
}

struct Api {
    replies: RefCell<VecDeque<Result<Value>>>,
    calls: RefCell<Vec<Request>>,
}
impl Api {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: RefCell::new(replies.into_iter().map(Ok).collect()),
            calls: RefCell::default(),
        }
    }
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts)?;
        if r.limit == models::MAX_MEDIA {
            assert!(
                r.headers.is_empty(),
                "Never send the API key to a media host"
            );
            assert!(r.redirect_hosts.is_empty());
            assert_eq!(r.hosts, ["static.vivi-x.ai", "clipgen-us.vivi-x.ai"]);
            self.calls.borrow_mut().push(r);
            return Ok(b"synthetic MP4 bytes".to_vec());
        }
        assert_eq!(r.hosts, ["api.vivix.ai"]);
        assert!(
            r.headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                    && value == &format!("Bearer {KEY}"))
        );
        self.calls.borrow_mut().push(r);
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected network request")?
            .to_string()
            .into_bytes())
    }
}
fn spec(duration: i64) -> Value {
    models::spec(
        &json!({"model":MODEL,"prompt":"A cinematic coastal journey","seed":0,"duration":duration}),
    )
    .unwrap()
}
fn credentials() -> Value {
    json!({"VIVIX_API_KEY":KEY})
}
fn envelope(data: Value) -> Value {
    json!({"code":0,"message":"success","data":data})
}
fn status(state: &str) -> Value {
    envelope(
        json!({"job_id":"job_test","status":state,"duration":5,"aspect_ratio":"16:9","video_url":"https://static.vivi-x.ai/clips/job_test.mp4"}),
    )
}

#[test]
fn vivix_contract_durations_and_seed_validate_before_network() {
    // A new candidate must not lengthen every existing worker's session-renewal horizon.
    assert_eq!(models::max_job_ms(), 480000);
    assert_eq!(
        models::info(MODEL).unwrap()["dimensions"],
        json!([[1264, 720], [1280, 720]])
    );
    for duration in 5..=15 {
        let spec = spec(duration);
        models::check(&spec).unwrap();
        assert_eq!(spec["input"]["duration"], duration);
        let api = Api::new(vec![envelope(json!({"results":[{"job_id":"job_test"}]}))]);
        let reference = Router::new(&api, credentials()).submit(&spec).unwrap();
        assert_eq!(reference, "vivix_job_test");
        let calls = api.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, "https://api.vivix.ai/v1/videos/jobs");
        let body: Value = serde_json::from_slice(calls[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"model":"vivix-w1","seed":0,"jobs":[{"prompt":"A cinematic coastal journey","duration":duration,"aspect_ratio":"16:9","output_format":"mp4"}]})
        );
    }
    let default =
        models::spec(&json!({"model":MODEL,"prompt":"A cinematic coastal journey","seed":0}))
            .unwrap();
    assert_eq!(digest(&default).unwrap(), digest(&spec(5)).unwrap());
    for duration in [json!(4), json!(16), json!(5.5), json!("5"), json!(null)] {
        assert!(
            models::spec(&json!({"model":MODEL,"prompt":"test","seed":0,"duration":duration}))
                .is_err()
        );
    }
    for seed in [json!(-1), json!(2147483648_i64), json!(null)] {
        assert!(models::spec(&json!({"model":MODEL,"prompt":"test","seed":seed})).is_err());
    }
    for (key, value) in [
        ("resolution", json!("1080p")),
        ("aspect_ratio", json!("9:16")),
        ("ref_images", json!([])),
        ("output_format", json!("webm")),
    ] {
        let mut changed = spec(5);
        changed["input"][key] = value;
        let api = Api::new(vec![]);
        assert!(Router::new(&api, credentials()).submit(&changed).is_err());
        assert!(api.calls.borrow().is_empty());
    }
}

#[test]
fn vivix_state_mapping_and_resume_use_saved_id_without_resubmission() {
    for (state, expected) in [
        ("queued", "IN_QUEUE"),
        ("processing", "IN_PROGRESS"),
        ("succeeded", "COMPLETED"),
        ("failed", "FAILED"),
    ] {
        let api = Api::new(vec![status(state)]);
        assert_eq!(
            Router::new(&api, credentials())
                .poll("vivix_job_test", &spec(5))
                .unwrap(),
            expected
        );
        assert_eq!(api.calls.borrow()[0].method, "GET");
        assert_eq!(
            api.calls.borrow()[0].url,
            "https://api.vivix.ai/v1/videos/jobs/job_test"
        );
    }
    let api = Api::new(vec![status("new-unknown-state")]);
    assert_eq!(
        Router::new(&api, credentials())
            .poll("vivix_job_test", &spec(5))
            .unwrap_err()
            .0,
        "invalid_provider_status"
    );
}

#[test]
fn vivix_downloads_only_completed_output_without_credentials() {
    let api = Api::new(vec![status("succeeded")]);
    let result = Router::new(&api, credentials())
        .result("vivix_job_test", &spec(5))
        .unwrap();
    assert_eq!(result.media, b"synthetic MP4 bytes");
    assert_eq!(result.response_hash.len(), 64);
    assert_eq!(api.calls.borrow().len(), 2);
    for state in ["queued", "processing", "failed"] {
        let api = Api::new(vec![status(state)]);
        assert!(
            Router::new(&api, credentials())
                .result("vivix_job_test", &spec(5))
                .is_err()
        );
        assert_eq!(api.calls.borrow().len(), 1);
    }
}

#[test]
fn vivix_result_urls_fail_closed() {
    for url in [
        Value::Null,
        json!("https://169.254.169.254/latest/meta-data"),
        json!("https://static.vivi-x.ai.evil.test/video.mp4"),
        json!("https://clipgen-us.vivi-x.ai.evil.test/video.mp4"),
        json!("https://unreviewed.vivi-x.ai/video.mp4"),
        json!("http://static.vivi-x.ai/video.mp4"),
        json!("https://user:pass@static.vivi-x.ai/video.mp4"),
        json!("https://static.vivi-x.ai:8443/video.mp4"),
        json!("https://api.vivix.ai/private"),
    ] {
        let mut response = status("succeeded");
        response["data"]["video_url"] = url;
        let api = Api::new(vec![response]);
        assert!(
            Router::new(&api, credentials())
                .result("vivix_job_test", &spec(5))
                .is_err()
        );
        assert_eq!(api.calls.borrow().len(), 1);
    }
}

#[test]
fn vivix_authenticated_result_cdn_is_downloaded_without_credentials() {
    let mut response = status("succeeded");
    response["data"]["video_url"] = json!("https://clipgen-us.vivi-x.ai/clips/job_test.mp4");
    let api = Api::new(vec![response]);
    Router::new(&api, credentials())
        .result("vivix_job_test", &spec(5))
        .unwrap();
    assert!(api.calls.borrow()[1].headers.is_empty());
    assert!(api.calls.borrow()[1].redirect_hosts.is_empty());
}

#[test]
fn vivix_identifiers_and_echo_fields_are_bound() {
    for r in [
        "fal_job_test",
        "vivix_../secret",
        "vivix_job?key=x",
        "vivix_",
    ] {
        let api = Api::new(vec![]);
        assert!(Router::new(&api, credentials()).poll(r, &spec(5)).is_err());
        assert!(api.calls.borrow().is_empty());
    }
    for (key, value) in [
        ("job_id", json!("other")),
        ("duration", json!(10)),
        ("aspect_ratio", json!("9:16")),
        ("model", json!("vivix-w1-ultrafast")),
    ] {
        let mut response = status("succeeded");
        response["data"][key] = value;
        let api = Api::new(vec![response]);
        assert!(
            Router::new(&api, credentials())
                .poll("vivix_job_test", &spec(5))
                .is_err()
        );
    }
}

#[test]
fn vivix_rejections_never_expose_raw_messages_or_retry_post() {
    for (code, reason) in [
        (10001, "vivix_auth_rejected"),
        (10003, "vivix_auth_rejected"),
        (10004, "vivix_billing_required"),
        (10005, "vivix_billing_required"),
        (10008, "vivix_capacity_limited"),
        (30008, "vivix_capacity_limited"),
        (20004, "vivix_invalid_parameters"),
        (20005, "vivix_job_not_found"),
        (30001, "vivix_content_rejected"),
        (50001, "vivix_unavailable"),
        (50003, "vivix_unavailable"),
        (99999, "provider_rejected"),
    ] {
        let api = Api::new(vec![json!({"code":code,"message":KEY,"data":null})]);
        assert_eq!(
            Router::new(&api, credentials())
                .submit(&spec(5))
                .unwrap_err()
                .0,
            reason
        );
        assert_eq!(api.calls.borrow().len(), 1);
    }
    let api = Api::new(vec![]);
    api.replies
        .borrow_mut()
        .push_back(Err(Error("request_timeout")));
    assert_eq!(
        Router::new(&api, credentials())
            .submit(&spec(5))
            .unwrap_err()
            .0,
        "request_timeout"
    );
    assert_eq!(api.calls.borrow().len(), 1);
}

#[test]
fn vivix_malformed_acceptance_is_not_a_reason_to_resubmit() {
    for data in [
        json!({}),
        json!({"code":"0","data":{}}),
        json!({"code":0,"data":null}),
        envelope(json!({"results":[]})),
        envelope(json!({"results":[{},{}]})),
        envelope(json!({"results":[{"job_id":"../bad"}]})),
        envelope(json!({"results":[{}]})),
    ] {
        let api = Api::new(vec![data]);
        assert!(Router::new(&api, credentials()).submit(&spec(5)).is_err());
        assert_eq!(api.calls.borrow().len(), 1);
    }
}

#[test]
fn vivix_requires_its_own_key_and_respects_signed_environment() {
    for creds in [
        json!({}),
        json!({"VIVIX_API_KEY":invitation::DISABLED}),
        json!({"FAL_KEY":KEY}),
    ] {
        let api = Api::new(vec![]);
        let mut router = Router::new(&api, creds);
        assert!(!router.models().contains(&MODEL.into()));
        assert!(router.submit(&spec(5)).is_err());
        assert!(api.calls.borrow().is_empty());
    }
    assert_eq!(
        Router::new(&Api::new(vec![]), credentials()).models(),
        [
            MODEL,
            "studio/vivix/vivix-w1",
            "studio/vivix/vivix-w1-fast",
            "studio/vivix/vivix-w1-ultrafast"
        ]
    );
    let mut inv = json!({"minerId":"test-miner","request":{"compose_file":{"allowed_envs":["MINER_ID","MINER_TOKEN","VIVIX_API_KEY"],"docker_compose_file":"services:\n  worker:\n    image: ghcr.io/everyframe-studios/everyframe-miner:fixture\n","pre_launch_script":invitation::PUBLIC_SCRIPT}}});
    let creds =
        json!({"MINER_TOKEN":"synthetic-token","VIVIX_API_KEY":KEY,"FAL_KEY":"synthetic-fal-key"});
    assert_eq!(
        cloud::environment(&inv, &creds, false).unwrap()["VIVIX_API_KEY"],
        invitation::DISABLED
    );
    assert_eq!(
        cloud::environment(&inv, &creds, true).unwrap()["VIVIX_API_KEY"],
        KEY
    );
    inv["request"]["compose_file"]["allowed_envs"] = json!(["MINER_ID", "MINER_TOKEN", "FAL_KEY"]);
    assert!(
        cloud::environment(&inv, &creds, true)
            .unwrap()
            .get("VIVIX_API_KEY")
            .is_none()
    );
    let summary = invitation::provider_summary(&inv, &creds);
    let vivix = summary
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["provider"] == "vivix")
        .unwrap();
    assert_eq!(vivix["configured"], true);
    assert_eq!(vivix["releaseAllows"], false);
    assert_eq!(vivix["usable"], false);
}

#[test]
fn vivix_balance_is_native_w_credits_not_usd_or_a_credits() {
    let api = Api::new(vec![envelope(
        json!({"balances":[{"scope":"A","available":6750},{"scope":"W","available":130}]}),
    )]);
    let rows = balances::collect(&api, &credentials());
    assert_eq!(api.calls.borrow().len(), 1);
    assert_eq!(api.calls.borrow()[0].method, "GET");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["provider"], "vivix");
    assert_eq!(rows[0]["status"], "ok");
    assert!(rows[0]["remainingUsd"].is_null());
    assert_eq!(
        rows[0]["nativeBalance"],
        json!({"unit":"W_CREDITS","remainingMicrounits":130000000})
    );
    assert!(!json!(rows).to_string().contains(KEY));
}

#[test]
fn vivix_key_can_be_saved_and_removed_via_top_level_cli_without_echo() {
    let dir = common::tempdir();
    let state = State::new("mainnet", dir.path().join("profile").to_str()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args([
            "set-api-keys",
            "--provider",
            "vivix",
            "--stdin",
            "--json",
            "--state-dir",
        ])
        .arg(&state.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{KEY}\n").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(KEY));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(KEY));
    assert_eq!(
        state.read("credentials", false).unwrap()["VIVIX_API_KEY"],
        KEY
    );
    let output = Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args([
            "remove-api-key",
            "--provider",
            "vivix",
            "--yes",
            "--json",
            "--state-dir",
        ])
        .arg(&state.directory)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        state
            .read("credentials", false)
            .unwrap()
            .get("VIVIX_API_KEY")
            .is_none()
    );
}
