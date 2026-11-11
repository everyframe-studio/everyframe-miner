use everyframe_miner::{
    worker_diagnostic,
    worker_trace::{self, Cache, MAX_EVENTS, MAX_JOBS, RETENTION_MS},
};
use serde_json::{Value, json};
fn work(n: usize) -> Value {
    json!({"jobId":format!("{n:048x}"),"attemptId":"b".repeat(48),"inputHash":"c".repeat(64)})
}
#[test]
fn traces_are_job_bound_redacted_and_bounded() {
    let mut c = Cache::default();
    let w = work(1);
    for n in 0..1000 {
        c.record(&w,&json!({"phase":"provider_processing","code":"http_502","prompt":"PRIVATE","url":"https://secret/?key=PRIVATE","body":"PRIVATE"}),1000+n);
    }
    let d = c
        .snapshot(
            w["jobId"].as_str().unwrap(),
            "b".repeat(48).as_str(),
            "c".repeat(64).as_str(),
            2000,
        )
        .unwrap();
    assert_eq!(d["events"].as_array().unwrap().len(), MAX_EVENTS);
    assert_eq!(d["droppedEvents"], 936);
    assert!(!d.to_string().contains("PRIVATE"));
    assert!(d.to_string().len() <= 32768);
    assert!(
        c.snapshot(
            w["jobId"].as_str().unwrap(),
            &"d".repeat(48),
            &"c".repeat(64),
            2000
        )
        .is_none()
    );
    assert!(
        c.snapshot(
            w["jobId"].as_str().unwrap(),
            &"b".repeat(48),
            &"d".repeat(64),
            2000
        )
        .is_none()
    );
    assert!(
        c.snapshot(
            w["jobId"].as_str().unwrap(),
            &"b".repeat(48),
            &"c".repeat(64),
            2000 + RETENTION_MS
        )
        .is_none()
    );
}
#[test]
fn cache_has_a_hard_job_limit_and_preserves_terminal_error() {
    let mut c = Cache::default();
    for n in 0..MAX_JOBS + 1 {
        c.record(&work(n), &json!({"phase":"submitting"}), 1000);
    }
    assert!(
        c.snapshot(
            &format!("{:048x}", 0),
            &"b".repeat(48),
            &"c".repeat(64),
            1000
        )
        .is_none()
    );
    let w = work(MAX_JOBS);
    let spec = json!({"model":"minimax/h3-max-turbo/text-to-video"});
    c.record(&w,&json!({"phase":"worker_failed","diagnostic":worker_diagnostic::report("output_retrieval","untrusted_destination",&spec,None)}),1001);
    let d = c
        .snapshot(
            w["jobId"].as_str().unwrap(),
            &"b".repeat(48),
            &"c".repeat(64),
            1001,
        )
        .unwrap();
    assert_eq!(
        d["events"][1]["diagnostic"]["code"],
        "untrusted_destination"
    );
}
#[test]
fn requests_cannot_read_files_urls_or_other_data() {
    let mut r = json!({"requestId":"a".repeat(48),"jobId":"b".repeat(48),"attemptId":"c".repeat(48),"inputHash":"d".repeat(64),"expiresAt":10000});
    assert!(worker_trace::validate_request(&r, 1000).is_ok());
    let reply = worker_trace::answer(&r, 1000).unwrap();
    assert_eq!(reply["state"], "not_retained");
    assert!(reply["trace"].is_null());
    r["path"] = json!("/proc/self/environ");
    assert!(worker_trace::validate_request(&r, 1000).is_err());
    r.as_object_mut().unwrap().remove("path");
    r["jobId"] = json!("../../secret");
    assert!(worker_trace::validate_request(&r, 1000).is_err());
    r["jobId"] = json!("b".repeat(48));
    r["expiresAt"] = json!(1000);
    assert!(worker_trace::validate_request(&r, 1000).is_err());
}
