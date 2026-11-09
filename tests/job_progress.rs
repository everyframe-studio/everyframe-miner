use everyframe_miner::{
    Error, Result, cli, job_history,
    protocol::{self, Keys},
    worker::Runtime,
    worker_heartbeat::{self, Guard},
    worker_progress::Reporter,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Transport {
    worker: Keys,
    coordinator: Keys,
    payloads: Mutex<Vec<Value>>,
}
impl Runtime for Transport {
    fn quote(&self, _: &[u8]) -> Result<Value> {
        Err(Error("unused"))
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        assert_eq!(path, "/v1/heartbeat");
        assert!(body.to_string().len() <= 2048);
        let v = protocol::verified(body, &self.worker.signing_key())?;
        self.payloads.lock().unwrap().push(v["payload"].clone());
        protocol::signed(
            &json!({"sessionId":v["sessionId"],"requestNonce":v["nonce"],"result":{"ok":true,"progress":{"recorded":true}}}),
            &self.coordinator.signing,
        )
    }
}
#[test]
fn job_progress_is_signed_bounded_and_sent_only_after_coordinator_advertises_support() {
    let t = Transport {
        worker: Keys::default(),
        coordinator: Keys::default(),
        payloads: Mutex::new(vec![]),
    };
    let p = json!({"jobId":"a".repeat(48),"attemptId":"b".repeat(48),"runId":"c".repeat(48),"sequence":1,"phase":"provider_queued","at":0,"status":"IN_QUEUE"});
    let mut session = json!({"sessionId":"d".repeat(48),"heartbeatPath":"/v1/heartbeat"});
    worker_heartbeat::send_progress(
        &t,
        &session,
        &t.worker.signing,
        &t.coordinator.signing_key(),
        Some(&p),
    )
    .unwrap();
    assert_eq!(t.payloads.lock().unwrap()[0], json!({}));
    session["progressPolicy"] = json!(1);
    worker_heartbeat::send_progress(
        &t,
        &session,
        &t.worker.signing,
        &t.coordinator.signing_key(),
        Some(&p),
    )
    .unwrap();
    assert_eq!(t.payloads.lock().unwrap()[1]["progress"], p);
}
#[test]
fn progress_wakes_a_long_interval_heartbeat_and_filters_sensitive_or_unbounded_details() {
    let values = Arc::new(Mutex::new(vec![]));
    let output = values.clone();
    let guard = Guard::spawn_reporting(
        Duration::from_secs(10),
        Duration::from_secs(30),
        true,
        move |p| {
            if let Some(v) = p {
                output.lock().unwrap().push(v.clone());
            }
            true
        },
    )
    .unwrap();
    let work = json!({"jobId":"a".repeat(48),"attemptId":"b".repeat(48)});
    let mut p = Reporter::new(&work, &guard);
    p.emit("provider_poll_retry",json!({"code":"http_502","retries":1,"url":"https://secret/?key=private","prompt":"private","bytes":-1}),100000);
    p.emit(
        "provider_poll_retry",
        json!({"code":"http_502","retries":2}),
        100001,
    );
    p.emit(
        "provider_poll_retry",
        json!({"code":"http_502","retries":3}),
        130001,
    );
    let start = Instant::now();
    while values.lock().unwrap().len() < 2 && start.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let all = values.lock().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1]["sequence"], 2);
    assert_eq!(all[1]["retries"], 3);
    assert!(!all[0].to_string().contains("private"));
    assert!(all[0]["bytes"].is_null());
}
#[test]
fn dropping_a_backlogged_progress_guard_does_not_wait_for_all_queued_events() {
    let guard = Guard::spawn_reporting(
        Duration::from_secs(10),
        Duration::from_secs(30),
        true,
        move |_| {
            std::thread::sleep(Duration::from_millis(10));
            false
        },
    )
    .unwrap();
    for i in 0..1000 {
        guard.report(json!({"sequence":i}));
    }
    let start = Instant::now();
    drop(guard);
    assert!(start.elapsed() < Duration::from_millis(500));
}
#[test]
fn exact_job_lookup_is_read_only_and_rejects_paths_or_other_command_flags() {
    let words = vec!["jobs".to_string(), "--job-id".to_string(), "a".repeat(48)];
    let a = cli::parse(&words).unwrap();
    cli::validate_args(&a).unwrap();
    assert_eq!(job_history::query(&a).unwrap()["jobId"], "a".repeat(48));
    assert!(job_history::validate_query(&json!({"jobId":"../../other"})).is_err());
    assert!(cli::parse(&["resume".into(), "--job-id".into(), "a".repeat(48)]).is_err());
}
