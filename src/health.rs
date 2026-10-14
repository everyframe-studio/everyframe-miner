//! Read-only evidence, never an activation, balance check or paid smoke test.
use serde_json::{Value, json};

fn check(name: &str, state: &str, detail: &str) -> Value {
    json!({"name":name,"ok":state=="PASS","state":state,"detail":detail})
}

pub fn runtime_checks(status: &Value, at: i64) -> Vec<Value> {
    let mut checks = Vec::new();
    let fresh = status["lastSeen"]
        .as_i64()
        .is_some_and(|seen| seen > at.saturating_sub(30_000) && seen <= at);
    checks.push(check(
        "Worker heartbeat",
        if !status["lastSeen"].is_i64() { "WARN" } else if fresh && status["online"] == true { "PASS" } else { "FAIL" },
        if fresh && status["online"] == true {
            "Worker heartbeat received within 30 seconds."
        } else {
            "No current worker heartbeat verified. Check everycli status; do not redeploy an existing VM."
        },
    ));
    let session = status["sessionExpires"].as_i64();
    let active = session.is_some_and(|expiry| expiry > at) && status["attested"] == true;
    checks.push(check(
        "Attestation session",
        if session.is_none() { "WARN" } else if active { "PASS" } else { "FAIL" },
        if active { "Verified workload session has not expired." } else {
            "Current session could not be verified. Wait for fresh workload attestation; inspect everycli doctor."
        },
    ));
    let offers = status["offers"].as_array();
    let active_offers: Vec<_> = offers
        .into_iter()
        .flatten()
        .filter(|o| o["offered"] == true)
        .collect();
    if active_offers.is_empty() {
        checks.push(check("Active model support", "WARN", "No active model offer. Review everycli offers and choose a model and discount before accepting provider-billed work."));
    }
    for offer in active_offers {
        let name = format!("Model: {}", offer["model"].as_str().unwrap_or("unknown"));
        let catalog = status["modelCatalog"].as_array().and_then(|rows| {
            rows.iter()
                .find(|row| row["model"] == offer["model"] && row["model"].is_string())
        });
        let (state, detail) = if offer["pricingCurrent"] != true {
            (
                "FAIL",
                "Offer pricing is stale or unverified. Review everycli offers before renewing your bid.",
            )
        } else if let Some(row) = catalog {
            if row["offerable"] == true
                && row["workerAvailable"] == true
                && row["blockers"].as_array().is_some_and(|v| v.is_empty())
            {
                (
                    "PASS",
                    "Current offer; coordinator permits this model and the worker advertises support. API balance and generation are not tested.",
                )
            } else {
                (
                    "FAIL",
                    "This active model is not confirmed ready. Run everycli offers for its integration, pricing, worker-release or provider blocker.",
                )
            }
        } else {
            (
                "WARN",
                "Coordinator did not report model capability evidence. A bid alone does not prove worker support; inspect everycli offers.",
            )
        };
        checks.push(check(&name, state, detail));
    }
    checks
}

/// Historical acceptance is evidence, not a claim about the latest job or all models.
pub fn activity(status: &Value) -> Value {
    let accepted = status["jobs"].as_array().map(|rows| {
        rows.iter()
            .filter(|r| r["state"] == "accepted")
            .filter_map(|r| r["count"].as_u64())
            .fold(0u64, u64::saturating_add)
    });
    json!({"activeJobs":status["activeJobs"].as_u64(),"acceptedJobs":accepted,
        "jobs":status["jobs"],"jobsByModel":status["jobsByModel"],
        "note":"Historical accepted jobs are not a live test of every model. Zero active jobs is normal when idle. No paid generation or API balance check was performed."})
}
