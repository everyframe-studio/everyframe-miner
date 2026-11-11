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
        let body = if r.url == "https://openrouter.ai/api/v1/key" {
            assert_eq!(r.headers[0].1, "Bearer synthetic-secret");
            json!({"data":{"limit":null,"limit_remaining":null,"budgets":[],"effective_budget":null}})
        } else if r.url.contains("rest.alpha.fal.ai") {
            assert_eq!(r.timeout, 10);
            assert_eq!(r.url, "https://rest.alpha.fal.ai/billing/user_balance");
            assert!(
                r.headers
                    .iter()
                    .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                        && value.starts_with("Key "))
            );
            json!(1.25)
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
    assert_eq!(api.calls.borrow().len(), 3);
    *api.fail.borrow_mut() = Some("request_timeout");
    let report = router.refresh_availability(1_900_001).unwrap();
    assert!(router.models().is_empty());
    assert!(
        report
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "access_denied" || r["status"] == "temporarily_unavailable")
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
    let rows = CatalogCache::default().refresh(
        &api,
        &json!({"OPENROUTER_API_KEY":"synthetic-secret"}),
        1_000_000,
    );
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| retired(r["model"].as_str().unwrap()).is_some()
                || r["status"] == "temporarily_unavailable")
    );
}
#[test]
fn fal_checks_one_authenticated_key_and_vivix_checks_actual_workspace() {
    let api = api();
    let mut r = Router::new(
        &api,
        json!({"FAL_KEY":"synthetic-secret","VIVIX_API_KEY":"synthetic-secret"}),
    );
    let rows = r.refresh_availability(1_000_000).unwrap();
    let calls = api.calls.borrow();
    assert_eq!(
        calls
            .iter()
            .filter(|r| r.url.contains("api.vivix.ai"))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|r| r.url.contains("rest.alpha.fal.ai"))
            .count(),
        1
    );
    assert!(
        calls
            .iter()
            .all(|r| !r.url.contains("api.fal.ai/v1/models"))
    );
    assert!(
        r.models()
            .contains(&"minimax/h3-max-turbo/text-to-video".into())
    );
    assert!(!r.models().contains(&"fal-ai/sora-2/text-to-video".into()));
    assert!(!r.models().contains(&"fal-ai/veo2".into()));
    assert!(r.models().contains(&"studio/vivix/vivix-w1".into()));
    assert!(!r.models().contains(&"studio/vivix/vivix-w1-fast".into()));
    assert!(rows.as_array().unwrap().iter().filter(|row|everyframe_miner::models::info(row["model"].as_str().unwrap()).unwrap()["provider"]=="fal").all(|row|retired(row["model"].as_str().unwrap()).is_some()||row["status"]=="unchecked"));
    assert!(!rows.to_string().contains("synthetic-secret"));
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
fn fal_key_check_outages_never_invent_model_unavailability() {
    for code in [
        "http_429",
        "http_500",
        "http_503",
        "http_403",
        "http_404",
        "request_timeout",
        "invalid_remote_json",
    ] {
        let api = api();
        *api.fail.borrow_mut() = Some(code);
        let mut router = Router::new(&api, json!({"FAL_KEY":"synthetic-secret"}));
        let rows = router.refresh_availability(1_000_000).unwrap();
        assert!(
            router
                .models()
                .contains(&"minimax/h3-max-turbo/text-to-video".into()),
            "{code}"
        );
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|r| retired(r["model"].as_str().unwrap()).is_some()
                    || r["status"] == "unchecked"),
            "{code}"
        );
        assert_eq!(api.calls.borrow().len(), 1);
    }
}
#[test]
fn fal_key_rejection_blocks_work_until_authentication_succeeds() {
    let api = api();
    *api.fail.borrow_mut() = Some("http_401");
    let mut router = Router::new(&api, json!({"FAL_KEY":"synthetic-secret"}));
    let rows = router.refresh_availability(1_000_000).unwrap();
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "access_denied")
    );
    assert!(router.models().is_empty());
    *api.fail.borrow_mut() = Some("http_503");
    router.refresh_availability(1_900_001);
    assert!(router.models().is_empty());
    *api.fail.borrow_mut() = None;
    router.refresh_availability(2_800_002);
    assert!(
        router
            .models()
            .contains(&"minimax/h3-max-turbo/text-to-video".into())
    );
    assert!(!router.models().contains(&"fal-ai/veo2".into()));
}
#[test]
fn fal_key_checks_are_cached_independently_and_changed_keys_are_rechecked() {
    let api = api();
    *api.fail.borrow_mut() = Some("http_429");
    let mut router = Router::new(
        &api,
        json!({"FAL_KEY":"synthetic-secret","OPENROUTER_API_KEY":"synthetic-secret"}),
    );
    for now in [1_000_000, 1_060_001, 1_120_002] {
        router.refresh_availability(now);
    }
    let count = || {
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.url.contains("rest.alpha.fal.ai"))
            .count()
    };
    assert_eq!(count(), 1);
    router.credentials["FAL_KEY"] = json!("replacement-secret");
    router.refresh_availability(1_121_000);
    assert_eq!(count(), 2);
    router.refresh_availability(2_021_001);
    assert_eq!(count(), 3);
    assert!(
        api.calls
            .borrow()
            .iter()
            .all(|r| !r.url.contains("api.fal.ai/v1/models"))
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

#[test]
fn native_elevenlabs_uses_account_model_list_without_claiming_music_entitlements() {
    struct Eleven;
    impl Http for Eleven {
        fn bytes(&self, request: Request) -> Result<Vec<u8>> {
            assert_eq!(request.method, "GET");
            assert_eq!(request.url, "https://api.elevenlabs.io/v1/models");
            assert!(request.body.is_none());
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(k, v)| k == "xi-api-key" && v == "synthetic-secret")
            );
            Ok(serde_json::to_vec(&json!([{"model_id":"eleven_v4"}])).unwrap())
        }
    }
    let mut router = Router::new(&Eleven, json!({"ELEVENLABS_API_KEY":"synthetic-secret"}));
    let report = router.refresh_availability(1_000_000).unwrap();
    let state = |model: &str| {
        report
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["model"] == model)
            .unwrap()["status"]
            .as_str()
            .unwrap()
    };
    assert_eq!(state("studio/native/elevenlabs/eleven_v4"), "available");
    assert_eq!(state("studio/native/elevenlabs/eleven_v3"), "access_denied");
    assert_eq!(state("studio/native/elevenlabs/music_v2_5"), "unchecked");
}
