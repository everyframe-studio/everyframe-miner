//! Optional price refresh on an independent thread. It never submits a
//! generation, reads management credentials, or changes a worker deployment.
use crate::{
    Error, Result, need,
    network::Http,
    protocol, provider_prices,
    worker::Runtime,
    worker_balances::{BalanceSession, SharedSession},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Default)]
pub struct Reporter {
    next_check: i64,
    next_send: i64,
    session: Option<String>,
    pending: VecDeque<Value>,
}
fn call(
    runtime: &dyn Runtime,
    session: &BalanceSession,
    key: &str,
    action: &str,
    payload: Value,
) -> Result<Value> {
    let nonce = protocol::id();
    let body = protocol::signed(
        &json!({"sessionId":session.id,"nonce":nonce,"time":runtime.now(),"action":action,"payload":payload}),
        &session.signing,
    )?;
    let v = protocol::verified(&runtime.post("/v1/action", &body)?, key)?;
    need(
        v["sessionId"] == session.id && v["requestNonce"] == nonce && v["result"]["ok"] == true,
        "invalid_price_acknowledgment",
    )?;
    Ok(v["result"].clone())
}
impl Reporter {
    pub fn tick(
        &mut self,
        http: &dyn Http,
        runtime: &dyn Runtime,
        credentials: &Value,
        session: Option<&BalanceSession>,
        key: &str,
    ) -> Result<bool> {
        let now = runtime.now();
        let Some(session) = session.filter(|s| s.automatic_offers && s.expires > now + 10000)
        else {
            return Ok(false);
        };
        if self.session.as_deref() != Some(&session.id) {
            self.pending.clear();
            self.next_check = 0;
            self.next_send = 0;
            self.session = Some(session.id.clone());
        }
        if now < self.next_send {
            return Ok(false);
        }
        if self.pending.front().is_some_and(|p| {
            p["prices"].as_array().is_none_or(|a| {
                a.iter()
                    .any(|v| now - v["checkedAt"].as_i64().unwrap_or(0) >= 300000)
            })
        }) {
            self.pending.clear();
            self.next_check = 0;
        }
        if self.pending.is_empty() && now >= self.next_check {
            self.next_check = now + 60000;
            self.next_send = now + 15000;
            let plan = call(runtime, session, key, "price_plan", json!({}))?;
            let providers = plan["providers"]
                .as_array()
                .filter(|p| p.len() <= 2)
                .ok_or(Error("invalid_price_plan"))?;
            let mut seen = vec![];
            // Validate the entire signed plan before any provider call or pending write.
            for p in providers {
                let provider = p["provider"]
                    .as_str()
                    .filter(|p| ["fal", "openrouter"].contains(p))
                    .ok_or(Error("invalid_price_plan"))?;
                need(!seen.contains(&provider), "invalid_price_plan")?;
                seen.push(provider);
                let revision = p["revision"]
                    .as_u64()
                    .filter(|r| *r > 0 && *r <= 9007199254740991)
                    .ok_or(Error("invalid_price_plan"))?;
                let models = p["models"]
                    .as_array()
                    .filter(|m| !m.is_empty() && m.len() <= 64)
                    .ok_or(Error("invalid_price_plan"))?;
                let mut ids = std::collections::HashSet::new();
                for m in models {
                    let model = m["model"].as_str().ok_or(Error("invalid_price_plan"))?;
                    need(ids.insert(model), "invalid_price_plan")?;
                    let (known, api) =
                        provider_prices::model_request(model).ok_or(Error("invalid_price_plan"))?;
                    need(
                        known == provider && m["apiModel"] == api,
                        "invalid_price_plan",
                    )?;
                }
                let _ = revision;
            }
            for p in providers {
                let provider = p["provider"].as_str().unwrap();
                let revision = p["revision"].as_u64().unwrap();
                let models = p["models"].as_array().unwrap();
                let prices =
                    provider_prices::collect(http, credentials, provider, models, runtime.now());
                for chunk in prices.chunks(8) {
                    self.pending.push_back(
                        json!({"provider":provider,"policyRevision":revision,"prices":chunk}),
                    );
                }
            }
            self.next_check = runtime.now() + 60000;
            self.next_send = 0;
        }
        let Some(payload) = self.pending.front() else {
            return Ok(false);
        };
        self.next_send = runtime.now() + 15000;
        match call(runtime, session, key, "prices", payload.clone()) {
            Ok(_) => {
                self.pending.pop_front();
                self.next_send = 0;
                Ok(true)
            }
            Err(e) => {
                if e.0 == "http_409" {
                    self.pending.clear();
                    self.next_check = 0;
                }
                Err(e)
            }
        }
    }
}
pub fn start(
    credentials: Value,
    runtime: crate::worker::LiveRuntime,
    session: SharedSession,
    key: String,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("provider-price-refresh".into())
        .spawn(move || {
            let mut reporter = Reporter::default();
            while !stop.load(Ordering::Relaxed) {
                let current = session.try_lock().ok().and_then(|s| s.clone());
                let _ = reporter.tick(
                    &crate::network::PublicHttp,
                    &runtime,
                    &credentials,
                    current.as_ref(),
                    &key,
                );
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .map_err(|_| Error("price_reporter_start_failed"))?;
    Ok(())
}
