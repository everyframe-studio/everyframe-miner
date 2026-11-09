//! Bounded liveness independent of the serial, potentially billable worker loop.
//! Never submits work, renews a session, changes offers or signs job receipts.
use crate::{Error, Result, need, protocol, worker::Runtime};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Guard {
    stop: Option<mpsc::SyncSender<Signal>>,
    thread: Option<JoinHandle<()>>,
    stopped: Option<Arc<AtomicBool>>,
    progress: bool,
}
enum Signal {
    Stop,
    Progress(Value),
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
        Self::spawn_reporting(interval, lifetime, false, move |_| {
            send();
            true
        })
    }
    pub fn report(&self, value: Value) {
        if self.progress
            && let Some(sender) = &self.stop
        {
            let _ = sender.try_send(Signal::Progress(value));
        }
    }
    pub fn spawn_reporting(
        interval: Duration,
        lifetime: Duration,
        progress: bool,
        mut send: impl FnMut(Option<&Value>) -> bool + Send + 'static,
    ) -> Result<Self> {
        need(
            !interval.is_zero() && !lifetime.is_zero() && lifetime <= Duration::from_secs(1800),
            "invalid_heartbeat_window",
        )?;
        let (stop, receive) = mpsc::sync_channel(128);
        let stopped = Arc::new(AtomicBool::new(false));
        let done = stopped.clone();
        let thread = std::thread::Builder::new()
            .name("worker-heartbeat".into())
            .spawn(move || {
                let started = Instant::now();
                let mut pending = VecDeque::new();
                let mut retry = Instant::now();
                let mut heartbeat = Instant::now() + interval;
                loop {
                    let remaining = lifetime.saturating_sub(started.elapsed());
                    if remaining.is_zero() || done.load(Ordering::Acquire) {
                        break;
                    }
                    let wake = if pending.is_empty() { heartbeat } else { retry };
                    match receive.recv_timeout(
                        wake.saturating_duration_since(Instant::now())
                            .min(remaining),
                    ) {
                        Ok(Signal::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Ok(Signal::Progress(v)) => {
                            if pending.len() < 128 {
                                pending.push_back(v);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if done.load(Ordering::Acquire) || started.elapsed() >= lifetime {
                        break;
                    }
                    if !pending.is_empty() && Instant::now() >= retry {
                        if send(pending.front()) {
                            pending.pop_front();
                            retry = Instant::now();
                        } else {
                            retry = Instant::now() + Duration::from_secs(3);
                        }
                        heartbeat = Instant::now() + interval;
                    } else if Instant::now() >= heartbeat {
                        send(None);
                        heartbeat = Instant::now() + interval;
                    }
                }
            })
            .map_err(|_| Error("heartbeat_thread_unavailable"))?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
            stopped: Some(stopped),
            progress,
        })
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(done) = &self.stopped {
            done.store(true, Ordering::Release);
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.try_send(Signal::Stop);
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
    send_progress(runtime, session, signing, coordinator_key, None)
}
pub fn send_progress(
    runtime: &dyn Runtime,
    session: &Value,
    signing: &ed25519_dalek::SigningKey,
    coordinator_key: &str,
    progress: Option<&Value>,
) -> Result<()> {
    let path = match session.get("heartbeatPath") {
        None => "/v1/action", // Explicit compatibility with older coordinators.
        Some(p) if p == "/v1/heartbeat" => "/v1/heartbeat",
        _ => return Err(Error("invalid_heartbeat_path")),
    };
    let nonce = protocol::id();
    let mut payload = json!({});
    if session["progressPolicy"] == 1
        && let Some(value) = progress
    {
        payload["progress"] = value.clone();
    }
    let envelope = protocol::signed(
        &json!({"sessionId":session["sessionId"],"nonce":nonce,
        "time":runtime.now(),"action":"heartbeat","payload":payload}),
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
