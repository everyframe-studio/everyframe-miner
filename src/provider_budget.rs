//! OpenRouter account limits, checked with the deployed generation key.
//! No management key, generation request, or arbitrary upstream text is used.
use crate::{
    Error, Result, need,
    network::{Http, Request},
    protocol,
};
use serde_json::{Value, json};

pub const CHECK_INTERVAL_MS: i64 = 60_000;
pub const MAX_AGE_MS: i64 = 300_000;
pub const EXHAUSTED: &str = "provider_budget_exhausted";

fn amount(value: &Value) -> Result<i64> {
    let n = value
        .as_f64()
        .filter(|n| n.is_finite() && n.abs() <= 1e9)
        .ok_or(Error("invalid_provider_budget"))?;
    Ok((n * 1e6).floor() as i64)
}
fn reset(value: &Value) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    let text = value
        .as_str()
        .filter(|s| s.len() <= 40)
        .ok_or(Error("invalid_provider_budget"))?;
    chrono::DateTime::parse_from_rfc3339(text).map_err(|_| Error("invalid_provider_budget"))?;
    Ok(Some(text.to_owned()))
}

pub fn parse(data: &Value, key: &str, at: i64) -> Result<Value> {
    let data = &data["data"];
    need(
        data.is_object() && data.get("limit").is_some(),
        "invalid_provider_budget",
    )?;
    let mut limits = Vec::new();
    if !data["limit"].is_null() {
        need(amount(&data["limit"])? >= 0, "invalid_provider_budget")?;
        limits.push((amount(&data["limit_remaining"])?, None));
    } else {
        need(data["limit_remaining"].is_null(), "invalid_provider_budget")?;
    }
    let budgets = data["budgets"]
        .as_array()
        .ok_or(Error("invalid_provider_budget"))?;
    need(budgets.len() <= 64, "invalid_provider_budget")?;
    for budget in budgets
        .iter()
        .chain(data.get("effective_budget").filter(|v| !v.is_null()))
    {
        need(
            budget.is_object()
                && amount(&budget["limit_usd"])? >= 0
                && amount(&budget["spend_usd"])? >= 0,
            "invalid_provider_budget",
        )?;
        limits.push((
            amount(&budget["remaining_usd"])?,
            reset(&budget["resets_at"])?,
        ));
    }
    let limiting = limits.into_iter().min_by_key(|(remaining, _)| *remaining);
    let blocked = limiting
        .as_ref()
        .is_some_and(|(remaining, _)| *remaining <= 0);
    Ok(json!({"provider":"openrouter","source":"openrouter-key-v1",
        "state":if blocked {"blocked"} else {"ready"},"checkedAt":at,
        "credentialRef":fingerprint(key),"remainingMicrousd":limiting.as_ref().map(|x| x.0),
        "resetsAt":limiting.and_then(|x| x.1)}))
}
pub fn fingerprint(key: &str) -> String {
    protocol::sha(format!("everyframe-provider-budget-v1\0openrouter\0{key}"))
}
pub fn unknown(key: &str, at: i64) -> Value {
    json!({"provider":"openrouter","source":"openrouter-key-v1","state":"unknown",
        "checkedAt":at,"credentialRef":fingerprint(key),"remainingMicrousd":null,"resetsAt":null})
}
pub fn check(http: &dyn Http, key: &str, at: i64) -> Result<Value> {
    let mut request = Request::get("https://openrouter.ai/api/v1/key", &["openrouter.ai"]);
    request
        .headers
        .push(("authorization".into(), format!("Bearer {key}")));
    request.timeout = 10;
    request.limit = 64000;
    parse(&http.json(request)?, key, at)
}

/// Only OpenRouter's documented, provider-owned machine metadata can establish
/// an account budget rejection. Prompt echoes, raw messages and 5xx cannot.
pub fn http_error(status: u16, bytes: &[u8]) -> Error {
    if matches!(status, 402 | 403)
        && bytes.len() <= 64000
        && let Ok(value) = serde_json::from_slice::<Value>(bytes)
        && value["error"]["code"].as_u64() == Some(status as u64)
        && [
            "openrouter_key_limit",
            "openrouter_guardrail_budget",
            "openrouter_workspace_budget",
        ]
        .contains(
            &value["error"]["metadata"]["limit_source"]
                .as_str()
                .unwrap_or(""),
        )
    {
        return Error(EXHAUSTED);
    }
    crate::diagnostics::http_error(status)
}

/// A failed probe never clears an observed spending limit. Every attempt is
/// bounded and cached, including failures, to avoid hammering a shared gateway.
#[derive(Default)]
pub struct Cache {
    proof: Option<Value>,
    next_check: i64,
}
impl Cache {
    pub fn refresh(&mut self, http: &dyn Http, key: &str, now: i64) {
        let same = self
            .proof
            .as_ref()
            .is_some_and(|v| v["credentialRef"] == fingerprint(key));
        if same && now < self.next_check {
            return;
        }
        self.next_check = now.saturating_add(CHECK_INTERVAL_MS);
        match check(http, key, now) {
            Ok(proof) => self.proof = Some(proof),
            Err(_) if same && self.proof.as_ref().is_some_and(|v| v["state"] == "blocked") => {}
            Err(_) => self.proof = Some(unknown(key, now)),
        }
    }
    pub fn recheck(&mut self, http: &dyn Http, key: &str, now: i64) {
        self.next_check = 0;
        self.refresh(http, key, now);
    }
    pub fn proof(&self) -> Option<Value> {
        self.proof.clone()
    }
    pub fn allows(&self, now: i64) -> bool {
        self.proof.as_ref().is_some_and(|v| {
            v["state"] == "ready"
                && v["checkedAt"]
                    .as_i64()
                    .is_some_and(|t| t <= now && now - t <= MAX_AGE_MS)
        })
    }
    pub fn guard(&mut self, http: &dyn Http, key: &str, now: i64) -> Result<()> {
        self.refresh(http, key, now);
        if self.proof.as_ref().is_some_and(|v| v["state"] == "blocked") {
            return Err(Error(EXHAUSTED));
        }
        need(self.allows(now), "provider_budget_unverified")
    }
    pub fn reject(&mut self, key: &str, now: i64) {
        let mut proof = unknown(key, now);
        proof["state"] = json!("blocked");
        self.proof = Some(proof);
        self.next_check = now.saturating_add(CHECK_INTERVAL_MS);
    }
}
