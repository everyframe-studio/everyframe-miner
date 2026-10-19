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
    assert_eq!(api.calls.borrow().len(), 2);
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
    assert!(*api.calls.borrow() <= 12);
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
