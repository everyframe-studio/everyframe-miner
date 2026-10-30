use everyframe_miner::{
    Error, Result,
    model_availability::{CatalogCache, retired},
    network::{Http, Request},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
struct Api {
    calls: RefCell<Vec<Request>>,
    fail: RefCell<Option<&'static str>>,
    partial: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert_eq!(r.method, "GET");
        assert!(r.body.is_none());
        assert!(r.redirect_hosts.is_empty());
        self.calls.borrow_mut().push(r.clone());
        if let Some(code) = *self.fail.borrow() {
            return Err(Error(code));
        }
        let body = if r.url.contains("api.fal.ai") {
            assert!(r.headers.is_empty());
            let url = url::Url::parse(&r.url).unwrap();
            let ids: Vec<_> = url
                .query_pairs()
                .filter(|(k, _)| k == "endpoint_id")
                .map(|(_, v)| v.to_string())
                .collect();
            json!({"models":ids.iter().map(|id|json!({"endpoint_id":id,"metadata":{"status":"active"}})).collect::<Vec<_>>(),"has_more":self.partial})
        } else if r.url.contains("api.vivix.ai") {
            assert_eq!(r.headers[0].1, "Bearer synthetic-secret");
            json!({"code":0,"data":[{"name":"vivix-w1"}]})
        } else {
            let user = r.url.contains("/user?");
            if user {
                assert_eq!(r.headers[0].1, "Bearer synthetic-secret");
            } else {
                assert!(r.headers.is_empty());
            }
            let models = if user {
                vec![json!({"id":"bytedance/seedance-2.5"})]
            } else {
                vec![
                    json!({"id":"bytedance/seedance-2.5"}),
                    json!({"id":"google/veo-3.1"}),
                ]
            };
            json!({"data":models,"links":{"next":if self.partial{json!("https://untrusted.example/next")}else{Value::Null}}})
        };
        Ok(body.to_string().into_bytes())
    }
}
fn api() -> Api {
    Api {
        calls: RefCell::new(vec![]),
        fail: RefCell::new(None),
        partial: false,
    }
}
#[test]
fn checked_models_only_narrow_registry_and_do_not_submit() {
    let api = api();
    let mut router = Router::new(&api, json!({"OPENROUTER_API_KEY":"synthetic-secret"}));
    let report = router.refresh_availability(1_000_000).unwrap();
    assert_eq!(
        router.models(),
        vec!["studio/openrouter/bytedance/seedance-2.5"]
    );
    assert_eq!(
        report
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["model"] == "direct/openrouter/veo-3.1")
            .unwrap()["status"],
        "access_denied"
    );
    assert_eq!(
        report
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["model"] == "studio/openrouter/bytedance/seedance-2.0-mini")
            .unwrap()["status"],
        "removed"
    );
    assert!(!report.to_string().contains("synthetic-secret"));
    router.refresh_availability(1_000_001);
    assert_eq!(api.calls.borrow().len(), 2);
    *api.fail.borrow_mut() = Some("request_timeout");
    let report = router.refresh_availability(1_900_001).unwrap();
    assert!(router.models().is_empty());
    assert!(
        report
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "temporarily_unavailable")
    );
}
#[test]
fn rate_limits_auth_and_partial_catalogs_are_not_model_removals() {
    for (code, expected) in [
        ("http_401", "access_denied"),
        ("http_429", "temporarily_unavailable"),
        ("http_500", "temporarily_unavailable"),
    ] {
        let api = api();
        *api.fail.borrow_mut() = Some(code);
        let mut c = CatalogCache::default();
        let rows = c.refresh(
            &api,
            &json!({"OPENROUTER_API_KEY":"synthetic-secret"}),
            1_000_000,
        );
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == expected)
        );
    }
    let mut api = api();
    api.partial = true;
    for key in ["OPENROUTER_API_KEY", "FAL_KEY"] {
        let rows =
            CatalogCache::default().refresh(&api, &json!({key:"synthetic-secret"}), 1_000_000);
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|r| retired(r["model"].as_str().unwrap()).is_some()
                    || r["status"] == "temporarily_unavailable")
        );
    }
}
#[test]
fn fal_batches_public_checks_and_vivix_checks_actual_workspace() {
    let api = api();
    let mut r = Router::new(
        &api,
        json!({"FAL_KEY":"synthetic-secret","VIVIX_API_KEY":"synthetic-secret"}),
    );
    r.refresh_availability(1_000_000);
    let calls = api.calls.borrow();
    assert_eq!(
        calls
            .iter()
            .filter(|r| r.url.contains("api.vivix.ai"))
            .count(),
        1
    );
    let fal_calls: Vec<_> = calls
        .iter()
        .filter(|r| r.url.contains("api.fal.ai"))
        .collect();
    assert!(!fal_calls.is_empty());
    let ids: Vec<String> = fal_calls
        .iter()
        .flat_map(|r| {
            url::Url::parse(&r.url)
                .unwrap()
                .query_pairs()
                .filter(|(k, _)| k == "endpoint_id")
                .map(|(_, v)| v.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(fal_calls.iter().all(|r| {
        url::Url::parse(&r.url)
            .unwrap()
            .query_pairs()
            .filter(|(k, _)| k == "endpoint_id")
            .count()
            <= 50
    }));
    for policy in everyframe_miner::studio_tools::POLICIES
        .as_object()
        .unwrap()
        .values()
    {
        assert!(ids.contains(&policy["apiModel"].as_str().unwrap().to_string()));
    }
    assert!(!r.models().contains(&"fal-ai/sora-2/text-to-video".into()));
    assert!(!r.models().contains(&"fal-ai/veo2".into()));
    assert!(r.models().contains(&"studio/vivix/vivix-w1".into()));
    assert!(!r.models().contains(&"studio/vivix/vivix-w1-fast".into()));
}
#[test]
fn every_provider_api_id_is_separate_from_immutable_contract_id() {
    let model = everyframe_miner::models::info("direct/openrouter/veo-3.1").unwrap();
    assert_eq!(
        everyframe_miner::model_availability::api_id("direct/openrouter/veo-3.1", model),
        "google/veo-3.1"
    );
}

#[test]
fn fal_batch_not_found_isolated_without_removing_other_models() {
    struct Split {
        calls: RefCell<usize>,
    }
    impl Http for Split {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            *self.calls.borrow_mut() += 1;
            let u = url::Url::parse(&r.url).unwrap();
            let ids: Vec<_> = u
                .query_pairs()
                .filter(|(k, _)| k == "endpoint_id")
                .map(|(_, v)| v.to_string())
                .collect();
            if ids.contains(&"minimax/h3-max/text-to-video".to_string()) {
                return Err(Error("http_404"));
            }
            Ok(json!({"models":ids.iter().map(|id|json!({"endpoint_id":id})).collect::<Vec<_>>(),"has_more":false}).to_string().into_bytes())
        }
    }
    let api = Split {
        calls: RefCell::new(0),
    };
    let mut router = Router::new(&api, json!({"FAL_KEY":"synthetic-secret"}));
    let rows = router.refresh_availability(1_000_000).unwrap();
    assert!(*api.calls.borrow() <= 16);
    assert!(
        router
            .models()
            .contains(&"minimax/h3-max-turbo/text-to-video".into())
    );
    assert!(
        !router
            .models()
            .contains(&"minimax/h3-max/text-to-video".into())
    );
    assert_eq!(
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["model"] == "minimax/h3-max/text-to-video")
            .unwrap()["status"],
        "removed"
    );
}

#[test]
fn one_retired_fal_endpoint_and_a_rate_limit_leave_confirmed_models_available() {
    // Fal answers 404 for any batch naming a retired endpoint, then 429 once a
    // burst of requests is spent (2026-10-29: fal-ai/ltx-2/text-to-video).
    struct Fal {
        calls: RefCell<usize>,
        missing: &'static str,
        limit: usize,
    }
    impl Http for Fal {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            *self.calls.borrow_mut() += 1;
            if *self.calls.borrow() > self.limit {
                return Err(Error("http_429"));
            }
            let u = url::Url::parse(&r.url).unwrap();
            let ids: Vec<_> = u
                .query_pairs()
                .filter(|(k, _)| k == "endpoint_id")
                .map(|(_, v)| v.to_string())
                .collect();
            if ids.iter().any(|id| id == self.missing) {
                return Err(Error("http_404"));
            }
            Ok(json!({"models":ids.iter().map(|id|json!({"endpoint_id":id})).collect::<Vec<_>>(),"has_more":false}).to_string().into_bytes())
        }
    }
    let api = Fal {
        calls: RefCell::new(0),
        missing: "minimax/h3-max/text-to-video",
        limit: 6,
    };
    let rows =
        CatalogCache::default().refresh(&api, &json!({"FAL_KEY":"synthetic-secret"}), 1_000_000);
    let rows = rows.as_array().unwrap();
    let status = |s: &str| rows.iter().filter(|r| r["status"] == s).count();
    assert!(
        status("available") > 0,
        "confirmed models survive a later 429"
    );
    assert!(
        status("temporarily_unavailable") > 0,
        "unresolved models wait for the next check"
    );
    assert_eq!(
        *api.calls.borrow(),
        7,
        "no further requests after the rate limit"
    );
}

#[test]
fn coordinator_retirements_only_narrow_and_skip_the_provider_check() {
    let api = api();
    let mut c = CatalogCache::default();
    let fal = json!({"FAL_KEY":"synthetic-secret"});
    let gone = "minimax/h3-max/text-to-video";
    assert!(c.set_server_retirements(&json!({"fal":[gone],"unknown-provider":["x"]})));
    let rows = c.refresh(&api, &fal, 1_000_000);
    assert!(!c.allows(gone));
    assert!(c.allows("minimax/h3-max-turbo/text-to-video"));
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .filter(|r| r["model"] == gone)
            .all(|r| r["status"] == "removed")
    );
    assert!(
        api.calls.borrow().iter().all(|r| !r
            .url
            .contains("endpoint_id=minimax%2Fh3-max%2Ftext-to-video")),
        "a retired endpoint is not requested"
    );
    // Invalid lists are ignored whole; the previous list stays in force.
    for bad in [
        json!(["fal"]),
        json!({"fal":"x"}),
        json!({"fal":[1]}),
        json!({"fal":[""]}),
    ] {
        assert!(!c.set_server_retirements(&bad));
        assert!(!c.allows(gone));
    }
    // An empty list cannot re-enable a model this release itself retired.
    assert!(c.set_server_retirements(&json!({})));
    c.refresh(&api, &fal, 2_000_000);
    assert!(c.allows(gone));
    assert!(!c.allows("fal-ai/ltx-2/text-to-video"));
    assert_eq!(retired("fal-ai/ltx-2/text-to-video"), Some("removed"));
}
