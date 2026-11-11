//! Destination metadata only. Raw paths, query strings, fragments and userinfo
//! never leave the URL parser. The cache is thread-local and reset for each job.
use serde_json::{Value, json};
use std::cell::RefCell;
use url::Url;

thread_local! {
    static LAST: RefCell<Option<Value>> = const { RefCell::new(None) };
}
pub fn clear() {
    LAST.with(|v| *v.borrow_mut() = None);
}
pub fn current() -> Option<Value> {
    LAST.with(|v| v.borrow().clone())
}
fn host(value: &str, wildcard: bool) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b".-:[]".contains(&b)
                || wildcard && b == b'*'
        })
}
pub fn capture(raw: &str, allowed: &[String], stage: &str, rule: &str) {
    let parsed = Url::parse(raw).ok();
    let hostname = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .filter(|s| host(s, false));
    let scheme = match parsed.as_ref().map(Url::scheme) {
        Some(s @ ("https" | "http" | "data" | "blob" | "file")) => s,
        Some(_) => "other",
        None => "invalid",
    };
    let value = json!({"version":1,"stage":stage,"rule":rule,"scheme":scheme,
        "host":hostname,"port":parsed.as_ref().and_then(Url::port_or_known_default),
        "hasUserInfo":parsed.as_ref().is_some_and(|u| !u.username().is_empty() || u.password().is_some()),
        "hasQuery":parsed.as_ref().is_some_and(|u| u.query().is_some()),
        "hasFragment":parsed.as_ref().is_some_and(|u| u.fragment().is_some()),
        "allowedHosts":allowed.iter().filter(|s| host(s,true)).take(16).collect::<Vec<_>>(),
        "urlSha256":crate::protocol::sha(raw.as_bytes())});
    LAST.with(|v| *v.borrow_mut() = sanitize(&value));
}
pub fn sanitize(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let keys = [
        "version",
        "stage",
        "rule",
        "scheme",
        "host",
        "port",
        "hasUserInfo",
        "hasQuery",
        "hasFragment",
        "allowedHosts",
        "urlSha256",
    ];
    if o.len() != keys.len()
        || !keys.iter().all(|k| o.contains_key(*k))
        || v["version"] != 1
        || !matches!(v["stage"].as_str()?, "initial" | "redirect")
        || !matches!(
            v["rule"].as_str()?,
            "invalid_url"
                | "https_required"
                | "userinfo_disallowed"
                | "port_disallowed"
                | "fragment_disallowed"
                | "host_not_allowed"
                | "dns_failed"
                | "private_address"
        )
        || !matches!(
            v["scheme"].as_str()?,
            "https" | "http" | "data" | "blob" | "file" | "other" | "invalid"
        )
        || !(v["host"].is_null() || host(v["host"].as_str()?, false))
        || !(v["port"].is_null() || v["port"].as_u64().is_some_and(|p| p <= 65535))
    {
        return None;
    }
    for k in ["hasUserInfo", "hasQuery", "hasFragment"] {
        v[k].as_bool()?;
    }
    let allowed = v["allowedHosts"].as_array()?;
    if allowed.len() > 16
        || !allowed
            .iter()
            .all(|s| s.as_str().is_some_and(|s| host(s, true)))
    {
        return None;
    }
    let hash = v["urlSha256"].as_str()?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    Some(v.clone())
}
