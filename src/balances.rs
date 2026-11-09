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
pub const BALANCE_TIMEOUT_SECONDS: u64 = 30;

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

/// A successful balance read is separate from having credited hosting funds.
/// Do not infer that a postpaid account or its running VM is blocked.
pub fn phala_credit_alert(row: &Value) -> Option<&'static str> {
    if row["provider"] != "phala" || row["status"] != "ok" {
        return None;
    }
    let amount = row["remainingUsd"].as_f64()?;
    if !amount.is_finite() || amount > 0.0 {
        return None;
    }
    Some(if row["stale"] == true {
        "Phala's last reported credited balance was zero or negative. Refresh balances and check billing. Postpaid credit limits are not included."
    } else {
        "Phala has no credited funds. Add funds or check your postpaid billing arrangement. Postpaid credit limits are not included."
    })
}

/// Pika reports integer microdollars. An active postpaid account spends against
/// its current cycle; a scheduled but inactive plan still uses prepaid funds.
pub fn pika_remaining(v: &Value) -> Result<i64> {
    crate::need(
        v["unit"] == "micro_usd" && v["currency"] == "usd",
        "invalid_balance",
    )?;
    let balance = v["balance_micro_usd"]
        .as_i64()
        .ok_or(Error("invalid_balance"))?;
    let remaining = if v["postpaid"].is_null() {
        balance
    } else {
        match v["postpaid"]["active"].as_bool() {
            Some(true) => v["postpaid"]["cycle"]["remaining_micro_usd"]
                .as_i64()
                .ok_or(Error("invalid_balance"))?,
            Some(false) => balance,
            None => return Err(Error("invalid_balance")),
        }
    };
    crate::need(
        remaining.unsigned_abs() <= 1_000_000_000_000_000,
        "invalid_balance",
    )?;
    Ok(remaining)
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
    let response = http.json(balance_request("vivix", key)?)?;
    parse_vivix_balance(&response)
}

fn parse_vivix_balance(response: &Value) -> Result<Value> {
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

fn balance_request(provider: &str, key: &str) -> Result<Request> {
    let (url, host, header, value) = match provider {
        "fal-user" => (
            "https://rest.alpha.fal.ai/billing/user_balance",
            "rest.alpha.fal.ai",
            "Authorization",
            format!("Key {key}"),
        ),
        "vivix" => (
            "https://api.vivix.ai/v1/balance",
            "api.vivix.ai",
            "Authorization",
            format!("Bearer {key}"),
        ),
        "pika" => (
            "https://api.dev.pika.art/billing/balance",
            "api.dev.pika.art",
            "X-API-Key",
            key.into(),
        ),
        "bfl" => (
            "https://api.bfl.ai/v1/credits",
            "api.bfl.ai",
            "x-key",
            key.into(),
        ),
        "runway" => (
            "https://api.dev.runwayml.com/v1/organization",
            "api.dev.runwayml.com",
            "Authorization",
            format!("Bearer {key}"),
        ),
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
    req.timeout = BALANCE_TIMEOUT_SECONDS;
    req.limit = 64000;
    req.headers.push((header.into(), value));
    if provider == "phala" {
        req.headers
            .push(("X-Phala-Version".into(), "2026-06-23".into()));
    }
    if provider == "runway" {
        req.headers
            .push(("X-Runway-Version".into(), "2024-11-06".into()));
    }
    Ok(req)
}

fn parse_usd_balance(provider: &str, v: &Value) -> Result<f64> {
    let n = match provider {
        "fal-user" => {
            if !v.is_number() {
                return Err(Error("invalid_balance"));
            }
            number(v)?
        }
        "pika" => pika_remaining(v)? as f64 / 1_000_000.0,
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
        // BFL and Runway sell API credits at a published one US cent each.
        "bfl" => number(&v["credits"])? / 100.0,
        "runway" => number(&v["creditBalance"])? / 100.0,
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
    let mut planned = Vec::new();
    let mut requests = Vec::new();
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
        let source = if provider == "fal" && key == generation_key {
            "fal-user"
        } else {
            provider
        };
        match balance_request(source, credentials[key].as_str().unwrap()) {
            Ok(request) => {
                planned.push((provider, source));
                requests.push(request);
            }
            Err(error) => {
                let mut r = row(provider, "unsupported", None, crate::now());
                r["diagnostic"] = diagnostic(provider, error.0);
                rows.push(r);
            }
        }
    }
    // Live transport runs these independent reads concurrently; it never retries
    // or batches mutations. Unsupported providers make no HTTP request.
    for ((provider, source), response) in planned.into_iter().zip(http.read_json_batch(requests)) {
        let result = response.and_then(|v| {
            if provider == "vivix" {
                parse_vivix_balance(&v).map(|native| (None, Some(native)))
            } else {
                parse_usd_balance(source, &v).map(|usd| (Some(usd), None))
            }
        });
        let mut r = row(provider, "unavailable", None, crate::now());
        match result {
            Ok((amount, native)) => {
                r["status"] = json!("ok");
                r["remainingUsd"] = json!(amount);
                if let Some(native) = native {
                    r["nativeBalance"] = native;
                }
            }
            Err(error) => {
                r["diagnostic"] = diagnostic(provider, error.0);
            }
        }
        rows.push(r);
    }
    // Preserve the established provider order within each group.
    rows.sort_by_key(|r| {
        (
            r["status"] == "unsupported",
            PROVIDERS
                .iter()
                .position(|(p, _)| r["provider"] == *p)
                .unwrap_or(PROVIDERS.len()),
        )
    });
    rows
}

pub fn diagnostic(provider: &str, code: &str) -> Value {
    let mut d = crate::diagnostics::value(code);
    if code == "request_timeout" {
        d["message"] = json!("The balance query did not finish within 30 seconds.");
        d["next"] = json!(
            "Refresh balances to try this read-only check again. No generation or cloud change was requested."
        );
    }
    if provider == "google" && code == "unsupported_balance" {
        d["message"] = json!(
            "Google does not expose a supported USD balance query through this Gemini API key integration."
        );
        d["next"] = json!(
            "For Prepay, view credits and auto-reload in Google AI Studio Billing. For spending alerts, configure a Google Cloud Billing budget; those alerts are not a live prepaid balance."
        );
    }
    if provider == "kling" && code == "unsupported_balance" {
        d["message"] =
            json!("Kling does not expose a verified live USD balance through this integration.");
        d["next"] = json!(
            "Check API resource packs in the Kling developer console. Package quantities can lag by 12 hours and are not converted to USD."
        );
    }
    if provider == "vivix" && code == "unsupported_balance" {
        d["message"] = json!("This snapshot does not include a Vivix W-credit balance.");
        d["next"] = json!(
            "Update the worker with everycli worker-update, or check W credits in the Vivix billing dashboard. W credits are not converted to USD."
        );
    }
    if matches!(code, "http_401" | "http_403") {
        d["next"] = json!(match provider {
            "fal" =>
                "Verify the Fal API key: everycli set-api-keys --provider fal. An admin key (--provider fal-billing) is optional.",
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
    result.sort_by_key(|r| r["status"] == "unsupported");
    result
}
