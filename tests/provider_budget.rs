use everyframe_miner::{
    Error, Result,
    network::{Http, Request},
    provider_budget::{self as b, Cache},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
fn budget(remaining: f64) -> Value {
    json!({"data":{"limit":null,"limit_remaining":null,"budgets":[{"scope":"workspace","limit_usd":100.0,"spend_usd":100.0-remaining,"remaining_usd":remaining,"resets_at":"2026-11-12T00:00:00Z"}],"effective_budget":null}})
}
struct Api {
    data: RefCell<Result<Value>>,
    calls: Cell<u32>,
    posts: Cell<u32>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        self.calls.set(self.calls.get() + 1);
        if r.method == "POST" {
            self.posts.set(self.posts.get() + 1);
            return Err(Error("unexpected_paid_request"));
        }
        assert_eq!(r.url, "https://openrouter.ai/api/v1/key");
        assert_eq!(r.timeout, 10);
        assert_eq!(r.limit, 64000);
        assert_eq!(r.headers[0].0, "authorization");
        assert!(r.headers[0].1.starts_with("Bearer synthetic-"));
        self.data
            .borrow()
            .clone()
            .map(|v| v.to_string().into_bytes())
    }
}
fn api(data: Value) -> Api {
    Api {
        data: RefCell::new(Ok(data)),
        calls: Cell::new(0),
        posts: Cell::new(0),
    }
}
#[test]
fn documented_budget_metadata_only() {
    for source in [
        "openrouter_key_limit",
        "openrouter_guardrail_budget",
        "openrouter_workspace_budget",
    ] {
        let body = json!({"error":{"code":403,"message":"arbitrary text","metadata":{"limit_source":source}}});
        assert_eq!(
            b::http_error(403, body.to_string().as_bytes()).0,
            b::EXHAUSTED
        );
        assert_eq!(
            b::http_error(502, body.to_string().as_bytes()).0,
            "http_502"
        );
    }
    for body in [
        json!({"error":{"code":403,"message":"Key limit exceeded (weekly limit)","metadata":{}}}),
        json!({"error":{"code":403,"metadata":{"limit_source":"content_policy"}}}),
        json!({"error":{"code":400,"metadata":{"limit_source":"openrouter_key_limit"}}}),
    ] {
        assert_eq!(
            b::http_error(403, body.to_string().as_bytes()).0,
            "http_403"
        );
    }
    assert_eq!(
        everyframe_miner::submission::rejected(b::EXHAUSTED),
        Some(b::EXHAUSTED)
    );
    assert_eq!(
        everyframe_miner::submission::rejected("http_403"),
        Some("provider_invalid_request")
    );
}
#[test]
fn strict_budget_parse_uses_tightest_limit() {
    let mut v = budget(0.25);
    v["data"]["limit"] = json!(10);
    v["data"]["limit_remaining"] = json!(2.0);
    assert_eq!(
        b::parse(&v, "synthetic-key", 1).unwrap()["remainingMicrousd"],
        250000
    );
    v["data"]["effective_budget"] = budget(-2.48)["data"]["budgets"][0].clone();
    let proof = b::parse(&v, "synthetic-key", 1).unwrap();
    assert_eq!(proof["state"], "blocked");
    assert_eq!(proof["remainingMicrousd"], -2480000);
    assert!(!proof.to_string().contains("synthetic-key"));
    assert!(b::parse(&json!({"data":{"limit":null}}), "key", 1).is_err());
    v["data"]["effective_budget"]["remaining_usd"] = json!("bad");
    assert!(b::parse(&v, "key", 1).is_err());
}
#[test]
fn cached_block_survives_outage_and_needs_fresh_success() {
    let a = api(budget(-0.72));
    let mut cache = Cache::default();
    assert_eq!(
        cache.guard(&a, "synthetic-key", 1000).unwrap_err().0,
        b::EXHAUSTED
    );
    *a.data.borrow_mut() = Err(Error("request_timeout"));
    cache.refresh(&a, "synthetic-key", 2000);
    assert_eq!(a.calls.get(), 1);
    cache.refresh(&a, "synthetic-key", 62000);
    assert_eq!(a.calls.get(), 2);
    assert!(!cache.allows(62000));
    *a.data.borrow_mut() = Ok(budget(10.0));
    cache.refresh(&a, "synthetic-key", 123000);
    assert!(cache.allows(123000));
    assert!(!cache.allows(500000));
    cache.reject("synthetic-key", 124000);
    assert!(!cache.allows(124000));
    cache.refresh(&a, "synthetic-key", 125000);
    assert!(!cache.allows(125000));
    cache.refresh(&a, "synthetic-key", 185000);
    assert!(cache.allows(185000));
}
#[test]
fn changed_key_failure_does_not_inherit_ready_proof() {
    let a = api(budget(2.0));
    let mut cache = Cache::default();
    cache.refresh(&a, "synthetic-key", 1);
    *a.data.borrow_mut() = Err(Error("http_401"));
    cache.refresh(&a, "synthetic-replacement", 2);
    assert_eq!(
        a.calls.get(),
        2,
        "A changed credential bypasses the old cache"
    );
    assert!(!cache.allows(2));
    assert_ne!(
        cache.proof().unwrap()["credentialRef"],
        b::fingerprint("synthetic-key")
    );
}
#[test]
fn router_blocks_spending_before_generation() {
    let a = api(budget(0.0));
    let mut router = Router::new(&a, json!({"OPENROUTER_API_KEY":"synthetic-key"}));
    let spec = json!({"model":"studio/openrouter/bytedance/seedance-2.0-mini","input":{"prompt":"test","seed":42,"duration":4,"resolution":"720p","aspect":"16:9","references":[],"promptExpansion":"balanced"}});
    // Use the signed contract's input shape.
    let spec = everyframe_miner::models::check(&spec).map(|_| spec);
    if let Ok(spec) = spec {
        assert_eq!(
            router
                .submit_before(&spec, everyframe_miner::now() + 60000)
                .unwrap_err()
                .0,
            b::EXHAUSTED
        );
    } else {
        panic!("invalid fixture");
    }
    assert_eq!(a.posts.get(), 0);
}

#[test]
fn message_only_403_stays_final_but_a_confirmed_limit_blocks_subsequent_work() {
    struct Recheck {
        blocked: bool,
        refused: bool,
        calls: Cell<usize>,
        posts: Cell<usize>,
    }
    impl Http for Recheck {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            if r.url.ends_with("/key") {
                self.calls.set(self.calls.get() + 1);
                return Ok(budget(if self.blocked && self.posts.get() > 0 {
                    -0.72
                } else {
                    1.0
                })
                .to_string()
                .into_bytes());
            }
            assert_eq!(r.method, "POST");
            self.posts.set(self.posts.get() + 1);
            Err(Error("http_403").with_refusal(self.refused.then_some("content_policy")))
        }
    }
    for (blocked, refused) in [(false, false), (true, false), (true, true)] {
        let api = Recheck {
            blocked,
            refused,
            calls: Cell::new(0),
            posts: Cell::new(0),
        };
        let mut router = Router::new(&api, json!({"OPENROUTER_API_KEY":"synthetic-key"}));
        let spec = everyframe_miner::models::spec(
            &json!({"model":"direct/openrouter/veo-3.1","prompt":"test"}),
        )
        .unwrap();
        let error = router
            .submit_before(&spec, everyframe_miner::now() + 60000)
            .unwrap_err();
        assert_eq!(error.0, "http_403");
        assert_eq!(error.refusal(), refused.then_some("content_policy"));
        if blocked {
            assert_eq!(
                router
                    .submit_before(&spec, everyframe_miner::now() + 60000)
                    .unwrap_err()
                    .0,
                b::EXHAUSTED
            );
        }
        assert_eq!(api.posts.get(), 1, "No retry of the generation POST");
        assert_eq!(
            api.calls.get(),
            2,
            "Preflight plus independent failure confirmation"
        );
    }
}
