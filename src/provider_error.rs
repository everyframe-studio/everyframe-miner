//! Bounded provider diagnostics. Unknown wording is preserved, credentials and
//! signed URLs are removed, and text never controls submission certainty.
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub code: Option<String>,
    pub message: Option<String>,
    pub truncated: bool,
}
impl ProviderError {
    pub fn value(&self) -> Value {
        let mut value = serde_json::json!({});
        if let Some(code) = &self.code {
            value["code"] = serde_json::json!(code);
        }
        if let Some(message) = &self.message {
            value["message"] = serde_json::json!(message);
        }
        if self.truncated {
            value["truncated"] = Value::Bool(true);
        }
        value
    }
}

static FILTERS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
    r#"(?i)https?://[^\s<>\"']+"#,
    r"(?i)\b(?:bearer|basic)\s+[a-z0-9._~+/=-]+",
    r#"(?i)(?:authorization|api[_ -]?key|x-api-key|access[_ -]?token|refresh_token|secret|password)[\"']?\s*[:=]\s*[\"']?[^\s,;\"'}]+"#,
    r"\b(?:sk-[A-Za-z0-9_-]{8,}|AIza[A-Za-z0-9_-]{20,}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)\b",
].into_iter().map(|p| Regex::new(p).unwrap()).collect()
});
pub fn clean(text: &str, limit: usize, secrets: &[String]) -> (String, bool) {
    let mut text: String = text.chars().take(64000).collect();
    for secret in secrets.iter().filter(|s| s.len() >= 4) {
        text = text.replace(secret, "[redacted]");
    }
    for filter in FILTERS.iter() {
        text = filter.replace_all(&text, "[redacted]").into_owned();
    }
    text.retain(|c| {
        (!c.is_control() || matches!(c, '\n' | '\t'))
            && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    });
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    text.truncate(end);
    (text.trim().to_owned(), truncated)
}
fn fields(
    data: &Value,
    depth: usize,
    budget: &mut usize,
    codes: &mut Vec<String>,
    messages: &mut Vec<String>,
) {
    if depth > 8 || *budget == 0 {
        return;
    }
    *budget -= 1;
    match data {
        Value::String(text) => {
            messages.push(text.chars().take(64000).collect());
        }
        Value::Array(items) => {
            for item in items.iter().take(32) {
                fields(item, depth + 1, budget, codes, messages);
            }
        }
        Value::Object(object) => {
            for key in ["code", "error_code", "failureCode", "status_code", "type"] {
                if let Some(value) = object.get(key) {
                    if let Some(code) = value.as_str() {
                        codes.push(code.chars().take(1024).collect());
                    } else if value.is_number() {
                        codes.push(value.to_string());
                    }
                }
            }
            for key in [
                "message",
                "msg",
                "error_message",
                "status_message",
                "status_msg",
                "error_msg",
                "fail_reason",
                "task_status_msg",
                "failure_reason",
                "failureReason",
                "reason",
            ] {
                if let Some(text) = object.get(key).and_then(Value::as_str) {
                    messages.push(text.chars().take(64000).collect());
                }
            }
            for key in [
                "error",
                "errors",
                "detail",
                "details",
                "failure",
                "base_resp",
            ] {
                if let Some(value) = object.get(key) {
                    fields(value, depth + 1, budget, codes, messages);
                }
            }
            if let Some(raw) = object
                .get("metadata")
                .and_then(|m| m.get("raw"))
                .and_then(Value::as_str)
                && raw.len() <= 64000
            {
                if let Ok(value) = serde_json::from_str::<Value>(raw) {
                    fields(&value, depth + 1, budget, codes, messages);
                } else if !raw.trim_start().starts_with(['{', '[']) {
                    messages.push(raw.to_owned());
                }
            }
        }
        _ => {}
    }
}
pub fn extract(data: &Value, secrets: &[String]) -> Option<ProviderError> {
    let (mut codes, mut messages) = (vec![], vec![]);
    fields(data, 0, &mut 128, &mut codes, &mut messages);
    let (code, code_cut) = clean(codes.first().map_or("", String::as_str), 128, secrets);
    let mut unique = vec![];
    for message in messages {
        if !unique.contains(&message) {
            unique.push(message);
        }
    }
    let (message, message_cut) = clean(&unique.join("\n"), 4096, secrets);
    if code.is_empty() && message.is_empty() {
        return None;
    }
    Some(ProviderError {
        code: (!code.is_empty()).then_some(code),
        message: (!message.is_empty()).then_some(message),
        truncated: code_cut || message_cut,
    })
}
