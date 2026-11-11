use everyframe_miner::{destination_diagnostics as d, network, worker_diagnostic};
use serde_json::{Value, json};
fn fail(url: &str) -> Value {
    d::clear();
    assert!(network::safe_url(url, &["static.wavespeed.ai".into()]).is_err());
    d::current().unwrap()
}
#[test]
fn rejected_url_retains_hostname_and_rule_but_never_sensitive_components() {
    let raw =
        "https://assets.wavespeed.ai/private/SECRET_PATH.png?token=SECRET_QUERY#SECRET_FRAGMENT";
    let v = fail(raw);
    assert_eq!(v["host"], "assets.wavespeed.ai");
    assert_eq!(v["rule"], "fragment_disallowed");
    assert_eq!(
        v["urlSha256"],
        everyframe_miner::protocol::sha(raw.as_bytes())
    );
    assert_eq!(v["hasQuery"], true);
    assert_eq!(v["hasFragment"], true);
    assert!(!v.to_string().contains("SECRET"));
    assert!(v.get("url").is_none());
    let v = fail("https://assets.wavespeed.ai/path?signature=SECRET");
    assert_eq!(v["rule"], "host_not_allowed");
    assert_eq!(v["allowedHosts"], json!(["static.wavespeed.ai"]));
    assert_eq!(v["port"], 443);
}
#[test]
fn protocol_port_userinfo_and_invalid_urls_have_distinct_reasons() {
    for (url, rule) in [
        ("http://static.wavespeed.ai/SECRET", "https_required"),
        (
            "https://SECRET:SECRET@static.wavespeed.ai/a",
            "userinfo_disallowed",
        ),
        ("https://static.wavespeed.ai:8443/a", "port_disallowed"),
        ("not a URL SECRET", "invalid_url"),
    ] {
        let v = fail(url);
        assert_eq!(v["rule"], rule);
        assert!(!v.to_string().contains("SECRET"));
        assert!(d::sanitize(&v).is_some());
    }
}
#[test]
fn extension_is_capability_gated_and_cache_is_reset() {
    let v = fail("https://assets.wavespeed.ai/x?key=SECRET");
    let spec = json!({"model":"studio/wavespeed/bytedance/seedream-v5.5","input":{}});
    let old = worker_diagnostic::report_with_destination(
        "output_retrieval",
        "unapproved_destination",
        &spec,
        None,
        false,
    );
    assert!(old.get("destination").is_none());
    let new = worker_diagnostic::report_with_destination(
        "output_retrieval",
        "unapproved_destination",
        &spec,
        None,
        true,
    );
    assert_eq!(new["destination"], v);
    assert!(worker_diagnostic::sanitize(&new).is_some());
    d::clear();
    assert!(d::current().is_none());
    let next = worker_diagnostic::report_with_destination(
        "output_retrieval",
        "unapproved_destination",
        &spec,
        None,
        true,
    );
    assert!(next.get("destination").is_none());
}
#[test]
fn forbidden_fields_and_malformed_hostnames_are_rejected() {
    let v = fail("https://assets.wavespeed.ai/x");
    for (key, bad) in [
        ("url", json!("https://secret/")),
        ("path", json!("SECRET")),
        ("host", json!("good.invalid/?token=SECRET")),
        ("allowedHosts", json!(["https://secret/path"])),
    ] {
        let mut x = v.clone();
        x[key] = bad;
        assert!(d::sanitize(&x).is_none());
    }
}
#[test]
fn captures_are_thread_local_and_redirects_keep_their_stage() {
    fail("https://assets.wavespeed.ai/x");
    std::thread::spawn(|| assert!(d::current().is_none()))
        .join()
        .unwrap();
    d::capture(
        "https://cdn.other.example/private?token=SECRET",
        &["cdn.wavespeed.ai".into()],
        "redirect",
        "host_not_allowed",
    );
    assert_eq!(d::current().unwrap()["stage"], "redirect");
}
