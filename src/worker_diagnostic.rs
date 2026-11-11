//! Signed failure metadata only. No prompt, request body, response body, key,
//! URL, or output bytes can enter this report.
use serde_json::{Value, json};

pub fn sanitize(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if object.keys().any(|k| {
        ![
            "version",
            "phase",
            "code",
            "provider",
            "providerCompleted",
            "providerRequestId",
            "httpStatus",
            "destination",
            "execution",
        ]
        .contains(&k.as_str())
    }) {
        return None;
    }
    let phase = value["phase"].as_str()?;
    if value["version"] != 1
        || ![
            "submission",
            "provider_poll",
            "output_retrieval",
            "output_upload",
        ]
        .contains(&phase)
    {
        return None;
    }
    for (key, max) in [("code", 80), ("provider", 40)] {
        let v = value[key].as_str()?;
        if v.is_empty()
            || v.len() > max
            || !v
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
        {
            return None;
        }
    }
    if value["providerCompleted"].as_bool()
        != Some(["output_retrieval", "output_upload"].contains(&phase))
    {
        return None;
    }
    if let Some(id) = value.get("providerRequestId") {
        let s = id.as_str()?;
        if s.is_empty()
            || s.len() > 100
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return None;
        }
    }
    if let Some(status) = value.get("httpStatus") {
        let status = status.as_u64()?;
        if !(100..=599).contains(&status) || value["code"] != format!("http_{status}") {
            return None;
        }
    }
    if let Some(destination) = value.get("destination") {
        if ![
            "unapproved_destination",
            "private_destination",
            "dns_failed",
        ]
        .contains(&value["code"].as_str()?)
        {
            return None;
        }
        crate::destination_diagnostics::sanitize(destination)?;
    }
    if let Some(execution) = value.get("execution") {
        crate::execution_diagnostics::sanitize(execution)?;
    }
    Some(value.clone())
}

pub fn report(phase: &str, code: &str, spec: &Value, reference: Option<&str>) -> Value {
    let phase = match phase {
        "submission" | "provider_poll" | "output_retrieval" | "output_upload" => phase,
        _ => "provider_poll",
    };
    let safe_code = !code.is_empty()
        && code.len() <= 80
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    let provider = spec["model"]
        .as_str()
        .and_then(|id| crate::models::info(id).ok())
        .and_then(|m| m["provider"].as_str())
        .unwrap_or("unknown");
    let completed = matches!(phase, "output_retrieval" | "output_upload");
    let mut value = json!({"version":1,"phase":phase,
        "code":if safe_code {code} else {"unknown_worker_error"},
        "provider":provider,"providerCompleted":completed});
    if let Some(status) = code
        .strip_prefix("http_")
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| (100..=599).contains(s))
    {
        value["httpStatus"] = json!(status);
    }
    if let Some(reference) = reference {
        // Native references bind a provider task ID to this exact input hash.
        // Other opaque references may contain capabilities; do not forward them.
        let id = reference
            .starts_with("native_")
            .then(|| crate::native_media::reference_id(spec, reference).ok())
            .flatten();
        if let Some(id) = id {
            value["providerRequestId"] = json!(id);
        }
    }
    value
}

/// The coordinator explicitly negotiates this optional diagnostic extension.
pub fn report_with_destination(
    phase: &str,
    code: &str,
    spec: &Value,
    reference: Option<&str>,
    enabled: bool,
) -> Value {
    let mut value = report(phase, code, spec, reference);
    if enabled
        && matches!(
            code,
            "unapproved_destination" | "private_destination" | "dns_failed"
        )
        && let Some(destination) = crate::destination_diagnostics::current()
    {
        value["destination"] = destination;
    }
    if let Some(execution) = crate::execution_diagnostics::snapshot() {
        value["execution"] = execution;
    }
    value
}
