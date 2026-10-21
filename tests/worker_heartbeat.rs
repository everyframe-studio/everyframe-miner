use everyframe_miner::{
    Error, Result,
    protocol::{self, Keys},
    worker::Runtime,
    worker_heartbeat::{self, Guard},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Transport {
    coordinator: Keys,
    worker: Keys,
    calls: Mutex<Vec<(String, Value)>>,
    replay: bool,
}
impl Runtime for Transport {
    fn quote(&self, _: &[u8]) -> Result<Value> {
        Err(Error("unused"))
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let value = protocol::verified(body, &self.worker.signing_key())?;
        assert_eq!(value["action"], "heartbeat");
        assert_eq!(value["payload"], json!({}));
        self.calls
            .lock()
            .unwrap()
            .push((path.into(), value.clone()));
        protocol::signed(
            &json!({"sessionId":value["sessionId"],"requestNonce":if self.replay{json!("wrong")}else{value["nonce"].clone()},"result":{"ok":true}}),
            &self.coordinator.signing,
        )
    }
}
#[test]
fn heartbeat_is_signed_session_bound_replay_checked_and_uses_only_an_advertised_path() {
    let mut t = Transport {
        coordinator: Keys::default(),
        worker: Keys::default(),
        calls: Mutex::new(vec![]),
        replay: false,
    };
    let session = json!({"sessionId":"fixture","heartbeatPath":"/v1/heartbeat"});
    worker_heartbeat::send(
        &t,
        &session,
        &t.worker.signing,
        &t.coordinator.signing_key(),
    )
    .unwrap();
    worker_heartbeat::send(
        &t,
        &session,
        &t.worker.signing,
        &t.coordinator.signing_key(),
    )
    .unwrap();
    {
        let calls = t.calls.lock().unwrap();
        assert_eq!(calls[0].0, "/v1/heartbeat");
        assert_ne!(calls[0].1["nonce"], calls[1].1["nonce"]);
    }
    worker_heartbeat::send(
        &t,
        &json!({"sessionId":"fixture"}),
        &t.worker.signing,
        &t.coordinator.signing_key(),
    )
    .unwrap();
    assert_eq!(t.calls.lock().unwrap()[2].0, "/v1/action");
    assert_eq!(
        worker_heartbeat::send(
            &t,
            &json!({"sessionId":"fixture","heartbeatPath":"https://evil.test"}),
            &t.worker.signing,
            &t.coordinator.signing_key()
        )
        .unwrap_err()
        .0,
        "invalid_heartbeat_path"
    );
    assert_eq!(t.calls.lock().unwrap().len(), 3);
    t.replay = true;
    assert_eq!(
        worker_heartbeat::send(
            &t,
            &session,
            &t.worker.signing,
            &t.coordinator.signing_key()
        )
        .unwrap_err()
        .0,
        "invalid_heartbeat_acknowledgment"
    );
}

#[test]
fn heartbeat_window_is_bounded_and_drop_wakes_an_idle_thread() {
    let count = Arc::new(AtomicUsize::new(0));
    let sent = count.clone();
    let guard = Guard::spawn(
        Duration::from_millis(5),
        Duration::from_millis(30),
        move || {
            sent.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(80));
    let done = count.load(Ordering::SeqCst);
    assert!(done > 0);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(count.load(Ordering::SeqCst), done);
    drop(guard);
    let start = Instant::now();
    drop(Guard::spawn(Duration::from_secs(15), Duration::from_secs(60), || {}).unwrap());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(Guard::spawn(Duration::ZERO, Duration::from_secs(1), || {}).is_err());
    assert!(Guard::spawn(Duration::from_secs(1), Duration::from_secs(1801), || {}).is_err());
}
