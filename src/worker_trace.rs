//! A bounded, job-scoped trace. There is deliberately no file, shell, provider
//! request, arbitrary URL, or credential reader in the diagnostic protocol.
use crate::{Error, Result, need};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

pub const RETENTION_MS: i64 = 3 * 86_400_000;
pub const MAX_JOBS: usize = 128;
pub const MAX_EVENTS: usize = 64;
pub const MAX_TRACE_BYTES: usize = 32768;

fn hex(value: &Value, len: usize) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn word(value: &Value, max: usize) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    })
}
fn phase(value: &Value) -> bool {
    value.as_str().is_some_and(|p| {
        matches!(
            p,
            "submitting"
                | "provider_queued"
                | "provider_processing"
                | "provider_waiting"
                | "provider_completed"
                | "provider_poll_retry"
                | "retrieving_output"
                | "output_fetch_retry"
                | "uploading_output"
                | "upload_retry"
                | "worker_failed"
                | "submission_rejected"
                | "submission_uncertain"
                | "output_accepted"
        )
    })
}
#[derive(Clone)]
struct Entry {
    job: String,
    attempt: String,
    input_hash: String,
    updated: i64,
    dropped: usize,
    events: VecDeque<Value>,
}
#[derive(Default)]
pub struct Cache {
    entries: VecDeque<Entry>,
}
impl Cache {
    fn prune(&mut self, now: i64) {
        self.entries
            .retain(|e| now.saturating_sub(e.updated) <= RETENTION_MS);
        while self.entries.len() > MAX_JOBS {
            self.entries.pop_front();
        }
    }
    pub fn record(&mut self, work: &Value, event: &Value, now: i64) {
        if !hex(&work["jobId"], 48)
            || !hex(&work["attemptId"], 48)
            || !hex(&work["inputHash"], 64)
            || !phase(&event["phase"])
            || now < 0
        {
            return;
        }
        self.prune(now);
        let job = work["jobId"].as_str().unwrap();
        let attempt = work["attemptId"].as_str().unwrap();
        let index = match self
            .entries
            .iter()
            .position(|e| e.job == job && e.attempt == attempt)
        {
            Some(i) => i,
            None => {
                self.entries.push_back(Entry {
                    job: job.into(),
                    attempt: attempt.into(),
                    input_hash: work["inputHash"].as_str().unwrap().into(),
                    updated: now,
                    dropped: 0,
                    events: VecDeque::new(),
                });
                self.entries.len() - 1
            }
        };
        let e = &mut self.entries[index];
        if e.input_hash != work["inputHash"].as_str().unwrap() {
            return;
        }
        let mut safe = json!({"at":now,"phase":event["phase"]});
        for key in ["status", "code"] {
            if word(&event[key], 80) {
                safe[key] = event[key].clone();
            }
        }
        for key in ["bytes", "retries"] {
            if event[key].as_u64().is_some_and(|n| n <= 2147483647) {
                safe[key] = event[key].clone();
            }
        }
        if let Some(d) = crate::worker_diagnostic::sanitize(&event["diagnostic"]) {
            safe["diagnostic"] = d;
        }
        if e.events.len() >= MAX_EVENTS {
            e.events.remove(1);
            e.dropped += 1;
        }
        e.updated = now;
        e.events.push_back(safe);
        while e.events.len() > 2
            && e.events.iter().map(|v| v.to_string().len()).sum::<usize>() > MAX_TRACE_BYTES - 1024
        {
            e.events.remove(1);
            e.dropped += 1;
        }
        self.prune(now);
    }
    pub fn snapshot(
        &mut self,
        job: &str,
        attempt: &str,
        input_hash: &str,
        now: i64,
    ) -> Option<Value> {
        self.prune(now);
        let e = self
            .entries
            .iter()
            .find(|e| e.job == job && e.attempt == attempt && e.input_hash == input_hash)?;
        let value = json!({"version":1,"jobId":e.job,"attemptId":e.attempt,"inputHash":e.input_hash,"events":e.events,"droppedEvents":e.dropped});
        (value.to_string().len() <= MAX_TRACE_BYTES).then_some(value)
    }
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
pub fn record(work: &Value, event: &Value, now: i64) {
    if let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(Cache::default())).lock() {
        cache.record(work, event, now);
    }
}
pub fn validate_request(value: &Value, now: i64) -> Result<()> {
    let keys = ["requestId", "jobId", "attemptId", "inputHash", "expiresAt"];
    need(
        value
            .as_object()
            .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k))),
        "invalid_diagnostic_request",
    )?;
    need(
        hex(&value["requestId"], 48)
            && hex(&value["jobId"], 48)
            && hex(&value["attemptId"], 48)
            && hex(&value["inputHash"], 64),
        "invalid_diagnostic_request",
    )?;
    need(
        value["expiresAt"]
            .as_i64()
            .is_some_and(|t| t > now && t <= now + 86_400_000),
        "diagnostic_request_expired",
    )
}
pub fn answer(value: &Value, now: i64) -> Result<Value> {
    validate_request(value, now)?;
    let trace = CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .map_err(|_| Error("diagnostic_cache_unavailable"))?
        .snapshot(
            value["jobId"].as_str().unwrap(),
            value["attemptId"].as_str().unwrap(),
            value["inputHash"].as_str().unwrap(),
            now,
        );
    Ok(
        json!({"requestId":value["requestId"],"jobId":value["jobId"],"attemptId":value["attemptId"],
        "state":if trace.is_some(){"available"}else{"not_retained"},"trace":trace}),
    )
}
