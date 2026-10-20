//! Bounded liveness independent of the serial, potentially billable worker loop.
//! Never submits work, renews a session, changes offers or signs job receipts.
use crate::{Error, Result, need, protocol, worker::Runtime};
use serde_json::{Value, json};
use std::{
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Guard {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
impl Guard {
    pub fn active(&self) -> bool {
        self.thread.is_some()
    }

    pub fn spawn(
        interval: Duration,
        lifetime: Duration,
        mut send: impl FnMut() + Send + 'static,
    ) -> Result<Self> {
        need(
            !interval.is_zero() && !lifetime.is_zero() && lifetime <= Duration::from_secs(1800),
            "invalid_heartbeat_window",
        )?;
        let (stop, receive) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("worker-heartbeat".into())
            .spawn(move || {
                let started = Instant::now();
                loop {
                    let remaining = lifetime.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        break;
                    }
                    match receive.recv_timeout(interval.min(remaining)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if started.elapsed() >= lifetime {
                                break;
                            }
                            send();
                        }
                    }
                }
            })
            .map_err(|_| Error("heartbeat_thread_unavailable"))?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // The only production callback is a heartbeat POST with a 5 s timeout.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn send(
    runtime: &dyn Runtime,
    session: &Value,
    signing: &ed25519_dalek::SigningKey,
    coordinator_key: &str,
) -> Result<()> {
    let path = match session.get("heartbeatPath") {
        None => "/v1/action", // Explicit compatibility with older coordinators.
        Some(p) if p == "/v1/heartbeat" => "/v1/heartbeat",
        _ => return Err(Error("invalid_heartbeat_path")),
    };
    let nonce = protocol::id();
    let envelope = protocol::signed(
        &json!({"sessionId":session["sessionId"],"nonce":nonce,
        "time":runtime.now(),"action":"heartbeat","payload":{}}),
        signing,
    )?;
    let response = protocol::verified(&runtime.post(path, &envelope)?, coordinator_key)?;
    need(
        response["sessionId"] == session["sessionId"]
            && response["requestNonce"] == nonce
            && response["result"]["ok"] == true,
        "invalid_heartbeat_acknowledgment",
    )
}
