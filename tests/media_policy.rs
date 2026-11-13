use everyframe_miner::{
    Result,
    media_policy::{Cache, MAX_TTL_MS, SCHEMA, valid_host},
    network::{Http, Request},
    network::{public_address, safe_url},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
fn policy(revision: i64, now: i64, providers: Value) -> Value {
    json!({"schema":SCHEMA,"chain":{"network":"test"},"revision":revision,"issuedAt":now,"expiresAt":now+MAX_TTL_MS,"providers":providers})
}
#[test]
fn scoped_expiring_and_monotonic() {
    let mut c = Cache::default();
    let chain = json!({"network":"test"});
    let now = everyframe_miner::now();
    let p = policy(2, now, json!({"vivix":["crsr-fnv.vivi-x.ai"]}));
    c.install(&p, &chain, now).unwrap();
    assert_eq!(
        c.hosts("vivix", &["old.example.com"], now),
        vec!["crsr-fnv.vivi-x.ai"]
    );
    assert_eq!(c.hosts("fal", &["*.fal.media"], now), vec!["*.fal.media"]);
    for bad in [
        policy(1, now, json!({})),
        policy(2, now, json!({})),
        policy(3, now - MAX_TTL_MS, json!({})),
        policy(3, now + 30001, json!({})),
        policy(3, now, json!({"unknown":["x.example.com"]})),
    ] {
        assert!(c.install(&bad, &chain, now).is_err());
    }
    assert!(c.install(&p, &json!({"network":"finney"}), now).is_err());
    assert_eq!(
        c.hosts("vivix", &["old.example.com"], now + MAX_TTL_MS),
        vec!["old.example.com"]
    );
    c.install(&policy(3, now, json!({"vivix":[]})), &chain, now)
        .unwrap();
    assert!(c.hosts("vivix", &["old.example.com"], now).is_empty());
}
#[test]
fn invalid_destinations_never_enter_policy_or_bypass_network_guards() {
    for h in [
        "*",
        "*.net",
        "https://example.com",
        "user@example.com",
        "127.0.0.1",
        "[::1]",
        "example.com:8443",
        "foo.*.com",
        "Example.com",
        "foo.local",
        "-x.example.com",
        "*.co.uk",
    ] {
        assert!(!valid_host(h), "{h}");
    }
    assert!(valid_host("*.cloudfront.net"));
    assert!(valid_host("crsr-fnv.vivi-x.ai"));
    let hosts = vec!["*.vivi-x.ai".to_string()];
    for url in [
        "http://crsr-fnv.vivi-x.ai/a",
        "https://crsr-fnv.vivi-x.ai:8443/a",
        "https://user:secret@crsr-fnv.vivi-x.ai/a",
        "https://crsr-fnv.vivi-x.ai.evil.com/a",
    ] {
        assert!(safe_url(url, &hosts).is_err());
    }
    for ip in ["127.0.0.1", "10.0.0.1", "169.254.169.254", "::1"] {
        assert!(!public_address(ip.parse().unwrap()));
    }
}
struct Cdn;
impl Http for Cdn {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts)?;
        if r.url.starts_with("https://api.vivix.ai/") {
            assert!(r.headers.iter().any(|(k, _)| k == "authorization"));
            return Ok(json!({"code":0,"data":{"job_id":"job123","status":"succeeded","video_url":"https://crsr-fnv.vivi-x.ai/result.mp4"}}).to_string().into_bytes());
        }
        assert!(r.url.starts_with("https://crsr-fnv.vivi-x.ai/"));
        assert!(r.headers.is_empty());
        assert_eq!(r.method, "GET");
        assert!(r.body.is_none());
        assert!(r.limit <= 128 * 1024 * 1024);
        Ok(b"synthetic media".to_vec())
    }
}
#[test]
fn completed_vivix_can_download_new_cdn_after_signed_policy_install() {
    let spec = everyframe_miner::models::spec(
        &json!({"model":"direct/vivix/vivix-w1","prompt":"A bird flies","seed":117}),
    )
    .unwrap();
    let mut router = Router::new(&Cdn, json!({"VIVIX_API_KEY":"private-provider-key"}));
    assert!(router.result("vivix_job123", &spec).is_err());
    let chain = json!({"network":"test"});
    let now = everyframe_miner::now();
    router
        .set_media_policy(
            &policy(2, now, json!({"vivix":["crsr-fnv.vivi-x.ai"]})),
            &chain,
            now,
        )
        .unwrap();
    assert_eq!(
        router.result("vivix_job123", &spec).unwrap().media,
        b"synthetic media"
    );
}
