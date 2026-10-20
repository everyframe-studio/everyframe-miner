//! Bounded, read-only history filters shared by terminal and browser commands.
use crate::{Error, Result, cli::Args, need};
use serde_json::{Value, json};

pub fn query(args: &Args) -> Result<Value> {
    let mut out = json!({});
    for (flag, key) in [
        ("--limit", "limit"),
        ("--cursor", "cursor"),
        ("--status", "status"),
        ("--model", "model"),
    ] {
        if let Some(value) = args.get(flag) {
            out[key] = json!(value);
        }
    }
    validate_query(&out)?;
    need(
        !args.flag("--summary") || out.as_object().is_some_and(|o| o.is_empty()),
        "history_summary_conflict",
    )?;
    Ok(out)
}

pub fn validate_query(value: &Value) -> Result<()> {
    let entries = value.as_object().ok_or(Error("invalid_history_query"))?;
    for (key, v) in entries {
        let s = v.as_str().ok_or(Error("invalid_history_query"))?;
        match key.as_str() {
            "limit" => need(
                !s.is_empty()
                    && s.bytes().all(|c| c.is_ascii_digit())
                    && !s.starts_with('0')
                    && s.parse::<u16>().is_ok_and(|n| (1..=100).contains(&n)),
                "invalid_history_limit",
            )?,
            "status" => need(
                ["all", "active", "completed", "failed", "cancelled"].contains(&s),
                "invalid_history_status",
            )?,
            "model" => need(
                !s.is_empty()
                    && s.len() <= 150
                    && s.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._/-".contains(&c)),
                "invalid_history_model",
            )?,
            "cursor" => need(
                !s.is_empty()
                    && s.len() <= 300
                    && s.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                "invalid_history_cursor",
            )?,
            _ => return Err(Error("invalid_history_query")),
        }
    }
    Ok(())
}

pub fn validate_page(history: &Value) -> Result<()> {
    let mut filters = json!({"status":history["filters"]["status"]});
    if !history["filters"]["model"].is_null() {
        filters["model"] = history["filters"]["model"].clone();
    }
    validate_query(&filters).map_err(|_| Error("invalid_job_history"))?;
    need(
        history["version"] == 1
            && history["currency"] == "USD"
            && history["unit"] == "microusd"
            && history["rows"]
                .as_array()
                .is_some_and(|rows| rows.len() <= history["limit"].as_u64().unwrap_or(0) as usize)
            && history["limit"]
                .as_u64()
                .is_some_and(|n| (1..=100).contains(&n))
            && (history["nextCursor"].is_null()
                || history["nextCursor"].as_str().is_some_and(|s| {
                    !s.is_empty()
                        && s.len() <= 300
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                })),
        "invalid_job_history",
    )
}
