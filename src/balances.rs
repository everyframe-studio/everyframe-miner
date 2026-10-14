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
            let mut report = json!({"provider":r["provider"],"currency":"USD",
        "remainingMicrousd":r["remainingUsd"].as_f64().map(|n| (n * 1_000_000.0).round() as i64),
        "status":r["status"],"checkedAt":r["checkedAt"]});
            if let Some(native) = r.get("nativeBalance") {
                report["nativeBalance"] = native.clone();
            }
            report
        })
        .collect()
}

/// Undocumented read-only endpoint: fail closed on any schema change. Never
/// substitute A credits, a missing W row, or an upstream message for W funds.
pub fn vivix_w_balance(http: &dyn Http, key: &str) -> Result<Value> {
    let mut req = Request::get("https://api.vivix.ai/v1/balance", &["api.vivix.ai"]);
    req.headers
        .push(("Authorization".into(), format!("Bearer {key}")));
    req.timeout = 5;
    req.limit = 64000;
    let response = http.json(req)?;
    if response["code"].as_i64() != Some(0) {
        return Err(Error("invalid_balance"));
    }
    let entries = response["data"]["balances"]
        .as_array()
        .ok_or(Error("invalid_balance"))?;
    let w = entries
        .iter()
        .filter(|r| r["scope"] == "W")
        .collect::<Vec<_>>();
    if w.len() != 1 {
        return Err(Error("invalid_balance"));
    }
    let amount = number(&w[0]["available"])?;
    // Round down, never invent spendable fractions.
    let micro = (amount * 1_000_000.0).floor() as i64;
    if amount > 0.0 && micro == 0 {
        return Err(Error("invalid_balance"));
    }
    Ok(json!({"unit":"W_CREDITS","remainingMicrounits":micro}))
}

/// Worker telemetry uses deployed generation credentials only. Billing/admin
/// and Phala hosting keys remain local to the operator's console device.
pub fn worker_reports(http: &dyn Http, credentials: &Value) -> Vec<Value> {
    let mut deployed = json!({});
    for (_, key) in PROVIDERS {
        if configured(&credentials[key]) {
            deployed[key] = credentials[key].clone();
        }
    }
    let local = collect(http, &deployed);
    reports(&local)
        .into_iter()
        .filter_map(|mut row| {
            let provider = row["provider"].as_str()?;
            let (_, key) = PROVIDERS.iter().find(|(name, _)| *name == provider)?;
            let value = deployed[key].as_str()?;
            row["credentialRef"] = json!(crate::protocol::sha(format!(
                "everyframe-worker-balance-credential-v1\0{provider}\0{value}"
            )));
            Some(row)
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
            if !v["credits"]["currency"].is_string() {
                return Err(Error("invalid_balance"));
            }
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
        if provider == "vivix" {
            let mut r = row(provider, "unavailable", None, crate::now());
            match vivix_w_balance(http, credentials[key].as_str().unwrap()) {
                Ok(native) => {
                    r["status"] = json!("ok");
                    r["nativeBalance"] = native;
                }
                Err(e) => {
                    r["diagnostic"] = diagnostic(provider, e.0);
                }
            }
            rows.push(r);
            continue;
        }
        let result = fetch(http, provider, credentials[key].as_str().unwrap());
        let (status, amount, error) = match result {
            Ok(n) => ("ok", Some(n), None),
            Err(e) if e.0 == "unsupported_balance" => {
                ("unsupported", None, Some("unsupported_balance"))
            }
            Err(e) => ("unavailable", None, Some(e.0)),
        };
        let mut r = row(provider, status, amount, crate::now());
        if let Some(code) = error {
            r["diagnostic"] = diagnostic(provider, code);
        }
        rows.push(r);
    }
    rows
}

pub fn diagnostic(provider: &str, code: &str) -> Value {
    let mut d = crate::diagnostics::value(code);
    if provider == "vivix" && code == "unsupported_balance" {
        d["message"] = json!("This snapshot does not include a Vivix W-credit balance.");
        d["next"] = json!(
            "Update the worker with everycli worker-update, or check W credits in the Vivix billing dashboard. W credits are not converted to USD."
        );
    }
    if matches!(code, "http_401" | "http_403") {
        d["next"] = json!(match provider {
            "fal" =>
                "Verify a Fal key with billing access: everycli set-api-keys --provider fal-billing. Generation access alone may not permit balance reads.",
            "phala" =>
                "Verify the Phala Cloud API key and its account permissions: everycli set-api-keys --provider phala.",
            "openrouter" =>
                "Verify an OpenRouter management key: everycli set-api-keys --provider openrouter-billing.",
            _ => "Check the provider API key and account permissions.",
        });
    }
    d
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
        let native = if provider == "vivix" && status == "ok" {
            r.get("nativeBalance")
                .filter(|b| {
                    b["unit"] == "W_CREDITS"
                        && b["remainingMicrounits"]
                            .as_i64()
                            .is_some_and(|n| n.unsigned_abs() <= 1_000_000_000_000_000)
                })
                .map(|b| json!({"unit":"W_CREDITS","remainingMicrounits":b["remainingMicrounits"]}))
        } else {
            None
        };
        let mut clean = row(
            provider,
            if status == "ok" && amount.is_none() && native.is_none() {
                "unavailable"
            } else {
                status
            },
            amount,
            checked.unwrap_or(0),
        );
        if let Some(native) = native {
            clean["nativeBalance"] = native;
        }
        clean["source"] = json!(if local_row.is_some() {
            "local"
        } else if r["source"] == "attested-worker" {
            "worker"
        } else {
            "synced"
        });
        clean["stale"] = json!(
            checked.is_none_or(|t| now - t > MAX_AGE_MS)
                || local_row.is_none() && (r["stale"] == true || r["attestationCurrent"] == false)
        );
        if clean["status"] != "ok" {
            let code = if status == "unsupported" {
                "unsupported_balance"
            } else if local_row.is_some() {
                r["diagnostic"]["code"].as_str().unwrap_or("request_failed")
            } else if status == "ok" {
                "invalid_balance"
            } else {
                // The coordinator protocol does not carry provider diagnostics.
                // Never display arbitrary text from a remote snapshot.
                "balance_snapshot_reason_unavailable"
            };
            clean["diagnostic"] = diagnostic(provider, code);
        }
        result.push(clean);
    }
    result
}
