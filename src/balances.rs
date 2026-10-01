//! Balance telemetry contains numbers and fixed labels only, never provider responses.
use crate::{
    Error, Result,
    invitation::{PROVIDERS, configured},
    network::{Http, Request},
};
use serde_json::{Value, json};

pub const BILLING_KEYS: [(&str, &str); 2] = [
    ("fal-billing", "FAL_ADMIN_KEY"),
    ("openrouter-billing", "OPENROUTER_MANAGEMENT_KEY"),
];
pub const MAX_AGE_MS: i64 = 15 * 60 * 1000;

fn number(v: &Value) -> Result<f64> {
    let n = v
        .as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n: &f64| n.is_finite() && n.abs() <= 1e9)
        .ok_or(Error("invalid_balance"))?;
    Ok(n)
}

pub fn row(provider: &str, status: &str, remaining: Option<f64>, at: i64) -> Value {
    json!({"provider":provider,"currency":"USD","remainingUsd":remaining,
        "status":status,"checkedAt":at})
}

// The signed protocol permits integers only. Never sign floating-point dollars.
pub fn reports(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({"provider":r["provider"],"currency":"USD",
        "remainingMicrousd":r["remainingUsd"].as_f64().map(|n| (n * 1_000_000.0).round() as i64),
        "status":r["status"],"checkedAt":r["checkedAt"]})
        })
        .collect()
}

fn fetch(http: &dyn Http, provider: &str, key: &str) -> Result<f64> {
    let (url, host, header, value) = match provider {
        "fal" => (
            "https://api.fal.ai/v1/account/billing?expand=credits",
            "api.fal.ai",
            "Authorization",
            format!("Key {key}"),
        ),
        "phala" => (
            "https://cloud-api.phala.com/api/v1/auth/me",
            "cloud-api.phala.com",
            "X-API-Key",
            key.into(),
        ),
        "openrouter" => (
            "https://openrouter.ai/api/v1/credits",
            "openrouter.ai",
            "Authorization",
            format!("Bearer {key}"),
        ),
        _ => return Err(Error("unsupported_balance")),
    };
    let mut req = Request::get(url, &[host]);
    req.timeout = 5;
    req.limit = 64000;
    req.headers.push((header.into(), value));
    if provider == "phala" {
        req.headers
            .push(("X-Phala-Version".into(), "2026-06-23".into()));
    }
    let v = http.json(req)?;
    let n = match provider {
        "fal" => {
            if v["credits"]["currency"] != "USD" {
                return Err(Error("non_usd_balance"));
            }
            number(&v["credits"]["current_balance"])?
        }
        "phala" => {
            // Actual paid/granted credits only, including on post-paid accounts.
            // Never count a credit limit or subtract an unfinalized invoice.
            number(&v["credits"]["balance"])? + number(&v["credits"]["granted_balance"])?
        }
        "openrouter" => number(&v["data"]["total_credits"])? - number(&v["data"]["total_usage"])?,
        _ => unreachable!(),
    };
    if !n.is_finite() || n.abs() > 1e9 {
        return Err(Error("invalid_balance"));
    }
    Ok(n)
}

/// Only configured accounts are queried. Billing-only keys never enter worker envs.
pub fn collect(http: &dyn Http, credentials: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    for (provider, generation_key) in PROVIDERS
        .into_iter()
        .chain([("phala", "PHALA_CLOUD_API_KEY")])
    {
        let billing_key = match provider {
            "fal" => "FAL_ADMIN_KEY",
            "openrouter" => "OPENROUTER_MANAGEMENT_KEY",
            _ => generation_key,
        };
        let key = if configured(&credentials[billing_key]) {
            billing_key
        } else {
            generation_key
        };
        if !configured(&credentials[key]) {
            continue;
        }
        let result = fetch(http, provider, credentials[key].as_str().unwrap());
        let (status, amount) = match result {
            Ok(n) => ("ok", Some(n)),
            Err(Error("unsupported_balance")) => ("unsupported", None),
            Err(_) => ("unavailable", None),
        };
        rows.push(row(provider, status, amount, crate::now()));
    }
    rows
}

/// Merge only after authenticating the coordinator response. Local failures replace
/// old successes so an inaccessible account never appears as a live balance.
pub fn merge(remote: &Value, local: Vec<Value>, now: i64) -> Vec<Value> {
    let mut result = Vec::new();
    for (provider, _) in PROVIDERS
        .into_iter()
        .chain([("phala", "PHALA_CLOUD_API_KEY")])
    {
        let local_row = local.iter().find(|r| r["provider"] == provider);
        let remote_row = remote
            .as_array()
            .and_then(|a| a.iter().find(|r| r["provider"] == provider));
        let Some(r) = local_row.or(remote_row) else {
            continue;
        };
        let status = r["status"]
            .as_str()
            .filter(|s| ["ok", "unavailable", "unsupported"].contains(s))
            .unwrap_or("unavailable");
        let checked = r["checkedAt"]
            .as_i64()
            .filter(|t| *t > 0 && *t <= now + 30000);
        let amount = if status == "ok" && r["currency"] == "USD" {
            if local_row.is_some() {
                number(&r["remainingUsd"]).ok()
            } else {
                r["remainingMicrousd"]
                    .as_i64()
                    .filter(|n| n.unsigned_abs() <= 1_000_000_000_000_000)
                    .map(|n| n as f64 / 1_000_000.0)
            }
        } else {
            None
        };
        let mut clean = row(
            provider,
            if status == "ok" && amount.is_none() {
                "unavailable"
            } else {
                status
            },
            amount,
            checked.unwrap_or(0),
        );
        clean["source"] = json!(if local_row.is_some() {
            "local"
        } else {
            "synced"
        });
        clean["stale"] = json!(checked.is_none_or(|t| now - t > MAX_AGE_MS));
        result.push(clean);
    }
    result
}
