use everyframe_miner::{
    Error, Result, balances, key_check, models, native_media,
    network::{Http, Request},
    protocol::{digest, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";
struct Api {
    model: Value,
    calls: RefCell<Vec<Request>>,
    mode: Cell<u8>,
    output_url: RefCell<Option<String>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        self.calls.borrow_mut().push(r.clone());
        if u.host_str() == Some("api.everyframe.studio") {
            assert!(r.headers.is_empty());
            return Ok(PNG.to_vec());
        }
        if u.host_str() == Some("static.wavespeed.ai")
            || u.host_str().is_some_and(|h| h.ends_with(".cloudfront.net"))
        {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            return Ok(match self.model["kind"].as_str().unwrap() {
                "image" => PNG.to_vec(),
                "video" => b"\x00\x00\x00\x18ftypisom".to_vec(),
                _ => b"ID3fixture".to_vec(),
            });
        }
        assert_eq!(u.host_str(), Some("api.wavespeed.ai"));
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                    && v == "Bearer fixture-secret")
        );
        if r.url.ends_with("/balance") {
            assert_eq!(r.method, "GET");
            return Ok(json!({"code":200,"data":{"balance":0}})
                .to_string()
                .into_bytes());
        }
        if r.url.ends_with("/models") {
            assert_eq!(r.method, "GET");
            return Ok(
                json!({"code":200,"data":[{"model_id":self.model["apiModel"]}]})
                    .to_string()
                    .into_bytes(),
            );
        }
        if r.method == "POST" {
            assert_eq!(u.path(), self.model["apiPath"].as_str().unwrap());
            let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert!(
                body[self.model["apiPrompt"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches('/')]
                .is_string()
            );
            assert!(!body.to_string().contains("@reference:"));
            assert!(!body.to_string().contains("fixture-secret"));
            if self.mode.get() == 1 {
                return Err(Error("request_timeout"));
            }
            return Ok(json!({"code":200,"data":{"id":"task_123","model":self.model["apiModel"],"status":"created"}}).to_string().into_bytes());
        }
        assert_eq!(u.path(), "/api/v3/predictions/task_123/result");
        Ok(match self.mode.get(){
   2=>json!({"code":200,"data":{"id":"other","model":self.model["apiModel"],"status":"completed"}}),
   3=>json!({"code":200,"data":{"id":"task_123","model":self.model["apiModel"],"status":"failed","code":1200,"error":"Content policy. fixture-secret"}}),
   6=>json!({"code":200,"data":{"id":"task_123","model":self.model["apiModel"],"status":"failed","code":5003}}),
   4=>json!({"code":5003,"message":"temporarily unavailable"}),
   _=>json!({"code":200,"data":{"id":"task_123","model":self.model["apiModel"],"status":"completed","outputs":[self.output_url.borrow().as_deref().unwrap_or(if self.mode.get()==5{"https://evil.example/x"}else{"https://static.wavespeed.ai/x"})]}})
  }.to_string().into_bytes())
    }
}
fn api(model: &Value) -> Api {
    Api {
        model: model.clone(),
        calls: RefCell::default(),
        mode: Cell::new(0),
        output_url: RefCell::default(),
    }
}
fn input(id: &str, m: &Value) -> Value {
    let mut refs = m["exampleReferences"].clone();
    for r in refs.as_array_mut().unwrap() {
        r["bytes"] = json!(PNG.len());
        r["sha256"] = json!(sha(PNG));
    }
    models::spec(&json!({"model":id,"prompt":"A calm river","references":refs}))
        .unwrap_or_else(|e| panic!("{id}: {e:?}"))
}
#[test]
fn seedream_downloads_wavespeed_cloudfront_distributions() {
    let id = "studio/wavespeed/bytedance/seedream-v5.5";
    let spec = input(id, &native_media::MODELS[id]);
    for host in [
        "d2h7xmz5gqybh9.cloudfront.net",
        "d111111abcdef8.cloudfront.net",
    ] {
        let api = api(&native_media::MODELS[id]);
        api.output_url
            .replace(Some(format!("https://{host}/generated/fixture.png")));
        let reference = native_media::reference(&spec, "task_123").unwrap();
        // Recover an existing prediction without submitting another paid job.
        let result = Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-secret"}))
            .result(&reference, &spec)
            .unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], "image");
        let calls = api.calls.borrow();
        assert!(calls.iter().all(|r| r.method == "GET"));
        let download = calls.last().unwrap();
        assert_eq!(
            url::Url::parse(&download.url).unwrap().host_str(),
            Some(host)
        );
        assert!(download.hosts.iter().any(|h| h == "*.cloudfront.net"));
        assert!(
            download.headers.is_empty(),
            "provider key must stay off the CDN"
        );
        assert!(download.redirect_hosts.is_empty());
    }
}

#[test]
fn seedream_cdn_allowance_keeps_other_destinations_blocked() {
    let id = "studio/wavespeed/bytedance/seedream-v5.5";
    let spec = input(id, &native_media::MODELS[id]);
    let reference = native_media::reference(&spec, "task_123").unwrap();
    for url in [
        "https://cloudfront.net/generated/fixture.png",
        "https://d2h7xmz5gqybh9.cloudfront.net.evil.example/fixture.png",
        "http://d2h7xmz5gqybh9.cloudfront.net/fixture.png",
        "https://d2h7xmz5gqybh9.cloudfront.net:444/fixture.png",
        "https://user:pass@d2h7xmz5gqybh9.cloudfront.net/fixture.png",
        "https://d2h7xmz5gqybh9.cloudfront.net/fixture.png#fragment",
        "https://127.0.0.1/fixture.png",
    ] {
        let api = api(&native_media::MODELS[id]);
        api.output_url.replace(Some(url.into()));
        let err = Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-secret"}))
            .result(&reference, &spec)
            .err()
            .expect("unapproved output must be rejected");
        assert_eq!(err.0, "unapproved_destination", "{url}");
        assert_eq!(
            api.calls.borrow().len(),
            1,
            "only the status GET is permitted"
        );
    }
}
#[test]
fn all_reviewed_wavespeed_endpoints_submit_once_and_recover_by_id() {
    let mut count = 0;
    for (id, m) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["provider"] == "wavespeed")
    {
        let spec = input(id, m);
        digest(&spec).unwrap();
        let api = api(m);
        let key = json!({"WAVESPEED_API_KEY":"fixture-secret"});
        let reference = Router::new(&api, key.clone()).submit(&spec).unwrap();
        let mut recovered = Router::new(&api, key);
        assert_eq!(recovered.poll(&reference, &spec).unwrap(), "COMPLETED");
        let result = recovered.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], m["kind"]);
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.url.contains("/predictions/"))
                .count(),
            1,
            "poll result is cached for 2 seconds"
        );
        for p in [
            json!({"webhook_url":"https://evil.example"}),
            json!({"image":"https://evil.example"}),
            json!({"api_key":"secret"}),
        ] {
            assert!(models::spec(&json!({"model":id,"prompt":"test","parameters":p,"references":m["exampleReferences"]})).is_err(),"{id}");
        }
        count += 1;
    }
    assert_eq!(count, 87); // Base catalog plus the two LTX 2.5 routes.
}
#[test]
fn uncertainty_and_refusals_never_trigger_resubmission() {
    let id = "studio/wavespeed/wavespeed-ai/z-image/turbo";
    let m = &native_media::MODELS[id];
    let spec = input(id, m);
    let api = api(m);
    let key = json!({"WAVESPEED_API_KEY":"fixture-secret"});
    api.mode.set(1);
    let e = Router::new(&api, key.clone()).submit(&spec).unwrap_err();
    assert_eq!(e.0, "request_timeout");
    assert_eq!(everyframe_miner::submission::rejected(e.0), None);
    let reference = native_media::reference(&spec, "task_123").unwrap();
    api.mode.set(2);
    assert_eq!(
        Router::new(&api, key.clone())
            .poll(&reference, &spec)
            .unwrap_err()
            .0,
        "provider_id_mismatch"
    );
    api.mode.set(3);
    let e = Router::new(&api, key.clone())
        .poll(&reference, &spec)
        .unwrap_err();
    assert_eq!(e.0, "wavespeed_content_rejected");
    assert!(!format!("{e:?}").contains("fixture-secret"));
    api.mode.set(4);
    let mut router = Router::new(&api, key.clone());
    let before = api.calls.borrow().len();
    for _ in 0..3 {
        assert!(everyframe_miner::submission::transient_poll(
            router.poll(&reference, &spec).unwrap_err().0
        ))
    }
    assert_eq!(
        api.calls.borrow().len(),
        before + 1,
        "outages are cached too"
    );
    api.mode.set(6);
    let error = Router::new(&api, key.clone())
        .poll(&reference, &spec)
        .unwrap_err();
    assert!(!everyframe_miner::submission::transient_poll(error.0));
    api.mode.set(5);
    assert!(Router::new(&api, key).result(&reference, &spec).is_err());
    assert!(
        !api.calls
            .borrow()
            .iter()
            .any(|r| r.url.contains("evil.example"))
    );
    assert_eq!(
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}
#[test]
fn read_only_key_balance_and_catalog_checks() {
    let id = "studio/wavespeed/wavespeed-ai/z-image/turbo";
    let api = api(&native_media::MODELS[id]);
    assert_eq!(
        key_check::check(&api, "wavespeed", "fixture-secret").0,
        key_check::Verdict::Valid
    );
    let row = balances::collect(&api, &json!({"WAVESPEED_API_KEY":"fixture-secret"})).remove(0);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["remainingUsd"].as_f64(), Some(0.0));
    let mut router = Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-secret"}));
    router.refresh_availability(1_000_000);
    assert_eq!(router.models(), vec![id]);
    assert!(api.calls.borrow().iter().all(|r| r.method == "GET"));
}
#[test]
fn http_5xx_and_ambiguous_payloads_do_not_prove_rejection() {
    use everyframe_miner::{submission, wavespeed};
    let b = br#"{"code":1407,"message":"balance empty"}"#;
    assert_eq!(
        wavespeed::http_error(402, b).0,
        "wavespeed_billing_required"
    );
    assert_eq!(wavespeed::http_error(503, b).0, "http_503");
    assert_eq!(submission::rejected(wavespeed::http_error(503, b).0), None);
    assert_eq!(
        wavespeed::http_error(402, br#"{"code":1407,"data":{"id":"accepted"}}"#).0,
        "invalid_provider_response"
    );
}

#[test]
fn worker_matches_all_coordinator_contract_hashes_and_provider_payloads() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/wavespeed.json")).unwrap();
    assert_eq!(vectors.len(), 85);
    for v in vectors {
        let spec = models::spec(&v["request"]).unwrap();
        assert_eq!(spec, v["spec"]);
        assert_eq!(digest(&spec).unwrap(), v["inputHash"]);
        let urls = spec["input"]["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["url"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let (_, body) = native_media::body(&spec, &urls).unwrap();
        assert_eq!(body, v["body"]);
    }
}
