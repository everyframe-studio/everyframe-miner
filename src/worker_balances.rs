//! Automatic, read-only balance telemetry. This loop runs on its own thread;
//! provider timeouts never hold the serial generation state machine.
use crate::{Error, Result, balances, network::Http, protocol, worker::Runtime};
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const CHECK_INTERVAL_MS: i64 = 60_000;
pub const RETRY_INTERVAL_MS: i64 = 30_000;
#[derive(Clone)]
pub struct BalanceSession {
    pub id: String,
    pub expires: i64,
    pub signing: SigningKey,
}
pub type SharedSession = Arc<Mutex<Option<BalanceSession>>>;
#[derive(Default)]
pub struct Reporter {
    checked_at: Option<i64>,
    sent_at: Option<i64>,
    acknowledged: Option<(String, i64)>,
    reports: Vec<Value>,
}
impl Reporter {
    // Only the fixed worker entrypoint calls this with its deployed key set.
    // No HTTP server, arbitrary signer, endpoint override, or paid request.
    pub fn tick(
        &mut self,
        http: &dyn Http,
        runtime: &dyn Runtime,
        credentials: &Value,
        session: Option<&BalanceSession>,
        coordinator_key: &str,
    ) -> Result<bool> {
        let now = runtime.now();
        let Some(session) = session.filter(|s| s.expires > now + 10_000) else {
            return Ok(false);
        };
        if self
            .checked_at
            .is_none_or(|at| now - at >= CHECK_INTERVAL_MS)
        {
            self.reports = balances::worker_reports(http, credentials);
            self.checked_at = Some(now);
        }
        let checked = self.checked_at.unwrap_or(now);
        if self.reports.is_empty()
            || self.acknowledged.as_ref() == Some(&(session.id.clone(), checked))
            || self.sent_at.is_some_and(|at| now - at < RETRY_INTERVAL_MS)
        {
            return Ok(false);
        }
        self.sent_at = Some(now);
        let nonce = protocol::id();
        let body = protocol::signed(
            &json!({"sessionId":session.id,"nonce":nonce,
            "time":now,"action":"balances","payload":{"balances":self.reports}}),
            &session.signing,
        )?;
        let reply = protocol::verified(&runtime.post("/v1/action", &body)?, coordinator_key)?;
        if reply["sessionId"] != session.id
            || reply["requestNonce"] != nonce
            || reply["result"]["ok"] != true
        {
            return Err(Error("invalid_balance_acknowledgment"));
        }
        self.acknowledged = Some((session.id.clone(), checked));
        Ok(true)
    }
}

pub fn start(
    credentials: Value,
    runtime: crate::worker::LiveRuntime,
    session: SharedSession,
    coordinator_key: String,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("balance-telemetry".into())
        .spawn(move || {
            let mut reporter = Reporter::default();
            while !stop.load(Ordering::Relaxed) {
                let current = session.try_lock().ok().and_then(|s| s.clone());
                // Errors deliberately contain no upstream body or key. Nothing is
                // logged, and the reporter backs off without stopping generation.
                let _ = reporter.tick(
                    &crate::network::PublicHttp,
                    &runtime,
                    &credentials,
                    current.as_ref(),
                    &coordinator_key,
                );
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .map_err(|_| Error("balance_reporter_start_failed"))?;
    Ok(())
}
