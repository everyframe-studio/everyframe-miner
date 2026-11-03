use everyframe_miner::{
    Result, models, native_media,
    network::{Http, Request},
    protocol::{digest, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nverified-input";
struct Api {
    video: bool,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        let result = if u.host_str() == Some("api.everyframe.studio") {
            assert!(r.headers.is_empty());
            PNG.to_vec()
        } else if u.host_str() == Some("delivery-us1.bfl.ai") {
            assert!(r.headers.is_empty());
            if self.video {
                b"test-mp4".to_vec()
            } else {
                b"\xff\xd8\xfftest-jpeg".to_vec()
            }
        } else {
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "x-key" && v == "fixture-secret")
            );
            if r.method == "POST" {
                let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
                assert!(b["prompt"].is_string());
                assert!(b.get("model").is_none());
                assert!(!String::from_utf8_lossy(r.body.as_ref().unwrap()).contains("@reference"));
                json!({"id":"task123","polling_url":"https://api.us1.bfl.ai/v1/get_result?id=task123"}).to_string().into_bytes()
            } else {
                assert_eq!(u.host_str(), Some("api.us1.bfl.ai"));
                json!({"id":"task123","status":"Ready","result":{"sample":"https://delivery-us1.bfl.ai/output"}}).to_string().into_bytes()
            }
        };
        self.calls.borrow_mut().push(r);
        Ok(result)
    }
}
#[test]
fn every_bfl_contract_reaches_its_own_endpoint_and_recovers_without_resubmitting() {
    let mut checked = 0;
    for (id, m) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["adapter"] == "schema-bfl")
    {
        let mut refs = m["exampleReferences"].clone();
        for r in refs.as_array_mut().unwrap() {
            r["bytes"] = json!(PNG.len());
            r["sha256"] = json!(sha(PNG));
        }
        let spec =
            models::spec(&json!({"model":id,"prompt":"A quiet river","references":refs})).unwrap();
        digest(&spec).unwrap();
        let api = Api {
            video: m["kind"] == "video",
            calls: RefCell::default(),
        };
        let keys = json!({"BFL_API_KEY":"fixture-secret"});
        let reference = Router::new(&api, keys.clone()).submit(&spec).unwrap();
        let mut resumed = Router::new(&api, keys);
        assert_eq!(resumed.poll(&reference, &spec).unwrap(), "COMPLETED");
        let result = resumed.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], m["kind"]);
        let calls = api.calls.borrow();
        let posts: Vec<_> = calls.iter().filter(|r| r.method == "POST").collect();
        assert_eq!(posts.len(), 1);
        assert_eq!(
            url::Url::parse(&posts[0].url).unwrap().path(),
            m["apiPath"].as_str().unwrap()
        );
        checked += 1;
    }
    assert_eq!(checked, 14);
}
#[test]
fn bfl_settings_use_the_reviewed_schema_and_reject_webhooks_and_unverified_images() {
    let model = "studio/native/bfl/flux-3-image";
    for p in [
        json!({"resolution":"4k","aspect_ratio":"9:16"}),
        json!({"width":3840,"height":2160,"grounding":false}),
        json!({"safety_tolerance":3}),
    ] {
        assert!(models::spec(&json!({"model":model,"prompt":"A poster","parameters":p})).is_ok());
    }
    for p in [
        json!({"resolution":"32k"}),
        json!({"webhook_url":"https://evil.example"}),
        json!({"images":["https://evil.example/a.png"]}),
    ] {
        assert!(models::spec(&json!({"model":model,"prompt":"A poster","parameters":p})).is_err());
    }
}

struct RegionalApi {
    polling: String,
    delivery: String,
    calls: RefCell<Vec<Request>>,
    states: RefCell<Vec<&'static str>>,
}
impl Http for RegionalApi {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let data = if r.method == "POST" {
            assert_eq!(r.url, "https://api.bfl.ai/v1/flux-3-image");
            json!({"id":"task_123","polling_url":self.polling})
                .to_string()
                .into_bytes()
        } else if r.url == self.delivery {
            assert!(r.headers.is_empty(), "API credentials never reach delivery");
            assert!(r.redirect_hosts.is_empty());
            b"\xff\xd8\xfffixture-jpeg".to_vec()
        } else {
            assert_eq!(r.url, self.polling);
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "x-key" && v == "fixture-secret")
            );
            let status = if self.states.borrow().is_empty() {
                "Ready"
            } else {
                self.states.borrow_mut().remove(0)
            };
            json!({"id":"task_123","status":status,"result":{"sample":self.delivery}})
                .to_string()
                .into_bytes()
        };
        self.calls.borrow_mut().push(r);
        Ok(data)
    }
}

#[test]
fn regional_tasks_resume_on_the_returned_cluster_and_download_without_credentials() {
    for region in ["us", "eu", "us1", "eu1", "us2", "eu2", "ap-southeast-1"] {
        let api = RegionalApi {
            polling: format!("https://api.{region}.bfl.ai/v1/get_result?id=task_123"),
            delivery: format!("https://delivery.{region}.bfl.ai/output.jpg?sig=fixture"),
            calls: RefCell::default(),
            states: RefCell::default(),
        };
        let spec =
            models::spec(&json!({"model":"studio/native/bfl/flux-3-image","prompt":"A river"}))
                .unwrap();
        let key = json!({"BFL_API_KEY":"fixture-secret"});
        let reference = Router::new(&api, key.clone()).submit(&spec).unwrap();
        let mut resumed = Router::new(&api, key);
        assert_eq!(resumed.poll(&reference, &spec).unwrap(), "COMPLETED");
        let output = resumed.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&output.media).unwrap();
        assert_eq!(bundle["parts"][0]["mime"], "image/jpeg");
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
    }
}

#[test]
fn unsafe_response_preserves_paid_task_id_without_following_or_reposting() {
    let api = RegionalApi {
        polling: "https://evil.example/v1/get_result?id=task_123&key=secret".into(),
        delivery: "https://delivery.us.bfl.ai/output.jpg".into(),
        calls: RefCell::default(),
        states: RefCell::default(),
    };
    let spec = models::spec(&json!({"model":"studio/native/bfl/flux-3-image","prompt":"A river"}))
        .unwrap();
    let key = json!({"BFL_API_KEY":"fixture-secret"});
    let reference = Router::new(&api, key.clone()).submit(&spec).unwrap();
    assert!(reference.ends_with("unresolved_task_123"));
    assert!(!reference.contains("evil") && !reference.contains("secret"));
    let mut resumed = Router::new(&api, key);
    assert_eq!(
        resumed.poll(&reference, &spec).unwrap_err().0,
        "bfl_polling_url_rejected"
    );
    assert_eq!(api.calls.borrow().len(), 1);
}

#[test]
fn flux3_reasoning_and_generation_resume_without_a_second_submission() {
    let api = RegionalApi {
        polling: "https://api.isr.bfl.ai/v1/get_result?id=task_123".into(),
        delivery: "https://delivery.isr.bfl.ai/output.png".into(),
        calls: RefCell::default(),
        states: RefCell::new(vec!["Pending", "Reasoning", "Generating", "Ready"]),
    };
    let spec = models::spec(&json!({"model":"studio/native/bfl/flux-3-image","prompt":"A river"}))
        .unwrap();
    let key = json!({"BFL_API_KEY":"fixture-secret"});
    let reference = Router::new(&api, key.clone()).submit(&spec).unwrap();
    let mut resumed = Router::new(&api, key);
    for expected in ["IN_PROGRESS", "IN_PROGRESS", "IN_PROGRESS", "COMPLETED"] {
        assert_eq!(resumed.poll(&reference, &spec).unwrap(), expected);
    }
    assert!(!resumed.result(&reference, &spec).unwrap().media.is_empty());
    assert_eq!(
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}
