//! Bounded Phala diagnostics, separate from generation/refusal classification.
//! Only validation descriptions are retained; echoed inputs and context are not.
use crate::{Error, ErrorContext, provider_error};
use serde_json::{Value, json};

pub const MAX_BODY: usize = 16384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhalaError {
    pub code: Option<String>,
    pub message: Option<String>,
    pub fields: Vec<String>,
    pub request_id: Option<String>,
    pub truncated: bool,
}

impl PhalaError {
    pub fn value(&self) -> Value {
        let mut out = json!({});
        if let Some(v) = &self.code {
            out["code"] = json!(v);
        }
        if let Some(v) = &self.message {
            out["message"] = json!(v);
        }
        if !self.fields.is_empty() {
            out["fields"] = json!(self.fields);
        }
        if let Some(v) = &self.request_id {
            out["requestId"] = json!(v);
        }
        if self.truncated {
            out["truncated"] = json!(true);
        }
        out
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = &self.code {
            parts.push(format!("Phala code: {v}"));
        }
        if let Some(v) = &self.message {
            parts.push(format!("Phala reason: {v}"));
        }
        if let Some(v) = &self.request_id {
            parts.push(format!("Request ID: {v}"));
        }
        parts.join("\n")
    }
}

fn field(value: &Value) -> Option<String> {
    let names: Vec<String> = if let Some(parts) = value.as_array() {
        parts
            .iter()
            .map(|p| {
                p.as_str()
                    .map(str::to_owned)
                    .or_else(|| p.as_u64().map(|n| n.to_string()))
            })
            .collect::<Option<_>>()?
    } else {
        value.as_str()?.split('.').map(str::to_owned).collect()
    };
    if names.is_empty() || names.len() > 8 {
        return None;
    }
    for s in &names {
        if s.len() > 64
            || s.is_empty()
            || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return None;
        }
    }
    Some(names.join("."))
}

fn descriptions(
    value: &Value,
    depth: usize,
    messages: &mut Vec<String>,
    fields: &mut Vec<String>,
    codes: &mut Vec<String>,
) {
    if depth > 6 || messages.len() >= 16 {
        return;
    }
    if let Some(v) = value.as_str() {
        messages.push(v.to_owned());
    } else if let Some(values) = value.as_array() {
        for v in values.iter().take(16) {
            descriptions(v, depth + 1, messages, fields, codes);
        }
    } else if let Some(object) = value.as_object() {
        for key in ["code", "error_code", "type"] {
            if let Some(v) = object.get(key).and_then(Value::as_str)
                && codes.len() < 16
            {
                codes.push(v.to_owned());
            }
        }
        let path = object
            .get("loc")
            .or_else(|| object.get("field"))
            .and_then(field);
        if let Some(path) = &path
            && !fields.contains(path)
        {
            fields.push(path.clone());
        }
        for key in ["message", "msg", "error_message"] {
            if let Some(v) = object.get(key).and_then(Value::as_str) {
                let v = v.to_owned();
                messages.push(path.as_ref().map_or(v.clone(), |p| format!("{p}: {v}")));
            }
        }
        for key in ["error", "errors", "detail", "details"] {
            if let Some(v) = object.get(key) {
                descriptions(v, depth + 1, messages, fields, codes);
            }
        }
    }
}

fn request_secrets(value: &Value, depth: usize, secrets: &mut Vec<String>) {
    if depth > 10 || secrets.len() >= 256 {
        return;
    }
    match value {
        Value::String(s) if s.len() >= 4 => {
            // Exact request strings must not be reflected back by the server.
            secrets.push(s.clone());
            secrets.push(
                serde_json::to_string(s)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_owned(),
            );
        }
        Value::Array(a) => {
            for v in a.iter().take(64) {
                request_secrets(v, depth + 1, secrets);
            }
        }
        Value::Object(o) => {
            for v in o.values().take(64) {
                request_secrets(v, depth + 1, secrets);
            }
        }
        _ => {}
    }
}

pub fn failure(
    status: u16,
    bytes: &[u8],
    request_id: Option<&str>,
    request: &crate::network::Request,
) -> Error {
    let error = crate::diagnostics::http_error(status);
    let mut secrets: Vec<String> = request
        .headers
        .iter()
        .filter(|(k, _)| ["authorization", "x-api-key"].contains(&k.to_ascii_lowercase().as_str()))
        .flat_map(|(_, v)| {
            [
                v.clone(),
                v.split_once(' ').map_or(v.as_str(), |(_, s)| s).to_owned(),
            ]
        })
        .collect();
    if let Some(body) = &request.body
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        request_secrets(&value, 0, &mut secrets);
    }
    // Redact a whole value before a shorter overlapping input can obscure it.
    // Response bytes are bounded above; truncate only after redaction below.
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    let (mut messages, mut fields, mut codes) = (Vec::new(), Vec::new(), Vec::new());
    let mut body_request_id = None;
    if bytes.len() <= MAX_BODY
        && let Ok(v) = serde_json::from_slice::<Value>(bytes)
    {
        descriptions(&v, 0, &mut messages, &mut fields, &mut codes);
        body_request_id = v
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    messages.dedup();
    let (message, message_cut) = provider_error::clean(&messages.join("\n"), 4096, &secrets);
    let (code, code_cut) =
        provider_error::clean(codes.first().map_or("", String::as_str), 128, &secrets);
    let request_id = request_id
        .or(body_request_id.as_deref())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
        })
        // Request IDs are remote text too: do not let this field bypass the
        // credential-pattern filtering used for codes and messages.
        .filter(|s| provider_error::clean(s, 128, &secrets).0 == *s)
        .map(str::to_owned)
        .filter(|s| !secrets.iter().any(|v| s.contains(v)));
    if message.is_empty() && code.is_empty() && request_id.is_none() {
        return error;
    }
    fields.retain(|v| provider_error::clean(v, 512, &secrets).0 == *v);
    let mut error = error;
    error.1 = Some(ErrorContext::Phala(Box::new(PhalaError {
        code: (!code.is_empty()).then_some(code),
        message: (!message.is_empty()).then_some(message),
        fields,
        request_id,
        truncated: bytes.len() > MAX_BODY || message_cut || code_cut,
    })));
    error
}
