use everyframe_miner::{
    Error, execution_diagnostics as d,
    network::{Http, PublicHttp, Request},
};
use serde_json::{Value, json};
fn spec() -> Value {
    json!({"model":"studio/wavespeed/bytedance/seedream-v5.5","input":{"prompt":"PRIVATE_PROMPT","parameters":{"resolution":"1k","output_format":"png","secret":"PRIVATE_KEY"},"references":[{"role":"image","url":"https://private/asset?token=PRIVATE_TOKEN","mime":"image/png","bytes":200,"width":512,"height":512}]}})
}
#[test]
fn network_failure_metadata_is_job_scoped_and_never_contains_urls_or_keys() {
    let _job = d::begin(&spec(), true);
    d::phase("output_retrieval");
    let result = PublicHttp.bytes(Request::get(
        "https://wrong.invalid/PRIVATE_PATH?token=PRIVATE_TOKEN",
        &["cdn.wavespeed.ai"],
    ));
    assert_eq!(result.unwrap_err().0, "unapproved_destination");
    let snapshot = d::snapshot().unwrap();
    assert_eq!(snapshot["network"]["total"], 1);
    assert_eq!(snapshot["network"]["failed"], 1);
    let request = &snapshot["network"]["requests"][0];
    assert_eq!(request["host"], "wrong.invalid");
    assert_eq!(request["phase"], "output_retrieval");
    assert_eq!(request["errorCode"], "unapproved_destination");
    assert!(!snapshot.to_string().contains("PRIVATE"));
    assert!(d::sanitize(&snapshot).is_some());
}
#[test]
fn response_metadata_json_shape_output_and_bounds_are_retained() {
    let _job = d::begin(&spec(), true);
    for _ in 0..20 {
        let r = Request::get(
            "https://api.wavespeed.ai/secret?id=TOKEN",
            &["api.wavespeed.ai"],
        );
        let mut span = d::HttpSpan::new(&r);
        span.dns(3);
        span.response(
            200,
            Some("application/json; charset=utf-8"),
            Some(44),
            Some("req_123"),
            Some("12"),
        );
        span.finish(&Ok(b"{\"data\":{\"outputs\":[\"PRIVATE_URL\"]}}".to_vec()));
        d::json_result(&Ok(
            json!({"data":{"outputs":["PRIVATE_URL"]},"PRIVATE_FIELD":"PRIVATE_BODY"}),
        ));
    }
    d::output(b"result", &"e".repeat(64));
    let s = d::snapshot().unwrap();
    assert_eq!(s["network"]["requests"].as_array().unwrap().len(), 12);
    assert_eq!(s["network"]["dropped"], 8);
    assert_eq!(s["output"]["bytes"], 6);
    assert_eq!(s["network"]["requests"][11]["outputCount"], 1);
    assert!(!s.to_string().contains("PRIVATE"));
    assert!(d::sanitize(&s).is_some());
    let mut injected = s.clone();
    injected["runtime"]["environment"] = json!({"KEY":"PRIVATE"});
    assert!(d::sanitize(&injected).is_none());
}
#[test]
fn disabled_or_finished_jobs_leave_no_observation_context() {
    {
        let _job = d::begin(&spec(), false);
        assert!(d::snapshot().is_none());
    }
    {
        let _job = d::begin(&spec(), true);
        assert!(d::snapshot().is_some());
    }
    assert!(d::snapshot().is_none());
    {
        let _job = d::begin(&spec(), true);
        assert_eq!(d::snapshot().unwrap()["network"]["total"], 0);
    }
}
#[test]
fn malformed_json_records_category_without_echoing_response() {
    let _job = d::begin(&spec(), true);
    let span = d::HttpSpan::new(&Request::get(
        "https://api.wavespeed.ai/a",
        &["api.wavespeed.ai"],
    ));
    span.finish(&Err(Error("invalid_remote_json")));
    d::json_result(&serde_json::from_slice::<Value>(b"PRIVATE: not json"));
    let s = d::snapshot().unwrap();
    assert_eq!(s["network"]["requests"][0]["jsonError"], "syntax");
    assert!(!s.to_string().contains("PRIVATE"));
}

#[test]
fn request_id_that_echoes_a_credential_is_dropped() {
    let mut r = Request::get("https://api.wavespeed.ai/", &["api.wavespeed.ai"]);
    r.headers.push((
        "Authorization".into(),
        "Bearer SECRET_CREDENTIAL_123".into(),
    ));
    assert!(d::public_request_id(Some("prefix_SECRET_CREDENTIAL_123"), &r).is_none());
    assert!(d::public_request_id(Some("https://private/path"), &r).is_none());
    assert_eq!(
        d::public_request_id(Some("request_123"), &r),
        Some("request_123")
    );
}
