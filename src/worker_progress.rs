//! Bounded metadata about a job. Never contains prompts, media, provider URLs,
//! credentials or a permission to retry a provider submission.
use crate::{protocol, worker_heartbeat::Guard};
use serde_json::{Value, json};
use std::io::Write;

pub struct Reporter<'a> {
    guard: &'a Guard,
    job: Value,
    attempt: Value,
    run: String,
    sequence: u64,
    previous: Value,
    previous_at: i64,
    work: Value,
}
impl<'a> Reporter<'a> {
    pub fn new(work: &Value, guard: &'a Guard) -> Self {
        Self {
            guard,
            job: work["jobId"].clone(),
            attempt: work["attemptId"].clone(),
            run: protocol::id(),
            sequence: 0,
            previous: Value::Null,
            previous_at: 0,
            work: work.clone(),
        }
    }
    pub fn emit(&mut self, phase: &str, detail: Value, at: i64) {
        let mut fields = json!({"phase":phase});
        for key in ["status", "code", "retries", "bytes"] {
            if let Some(v) = detail.get(key) {
                let valid = if ["status", "code"].contains(&key) {
                    v.as_str().is_some_and(|s| {
                        !s.is_empty()
                            && s.len() <= 80
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    })
                } else {
                    v.as_u64().is_some_and(|n| n <= 2147483647)
                };
                if valid {
                    fields[key] = v.clone();
                }
            }
        }
        // Repeat failures retain a cumulative count, but send at most one
        // observation every 30 seconds while their phase/code stays unchanged.
        let mut identity = fields.clone();
        identity.as_object_mut().unwrap().remove("retries");
        if identity == self.previous && at - self.previous_at < 30000 {
            return;
        }
        self.previous = identity;
        self.previous_at = at;
        self.sequence += 1;
        fields["jobId"] = self.job.clone();
        fields["attemptId"] = self.attempt.clone();
        fields["runId"] = json!(self.run);
        fields["sequence"] = json!(self.sequence);
        fields["at"] = json!(at);
        crate::worker_trace::record(&self.work, &fields, at);
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            json!({"event":"worker_job_progress","progress":fields})
        );
        self.guard.report(fields);
    }
}
