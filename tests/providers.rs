use everyframe_miner::{
    Result, invitation, models,
    network::{Http, Request, safe_url},
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;

struct Api {
    provider: String,
    spec: Value,
    calls: RefCell<Vec<Request>>,
    bad: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts).unwrap();
        let u = url::Url::parse(&r.url).unwrap();
        let path = u.path();
        let model = &models::info(self.spec["model"].as_str().unwrap()).unwrap()["apiModel"];
        let native = if self.bad { "wrong-id" } else { "job123" };
        let body: Value = r
            .body
            .as_ref()
            .map(|b| serde_json::from_slice(b).unwrap())
            .unwrap_or(json!(null));
        let media = r.limit >= 10 * 1024 * 1024;
        let v = if media {
            if self.provider == "google" {
                assert!(r.headers.iter().any(|(k, _)| k == "x-goog-api-key"));
                assert!(r.redirect_hosts.contains(&"*.googleusercontent.com".into()));
            } else if !["elevenlabs", "openrouter"].contains(&self.provider.as_str()) {
                assert!(r.headers.is_empty(), "credentials must not reach CDN");
            }
            json!(null)
        } else if r.method == "POST" {
            assert!(!r.headers.is_empty());
            match self.provider.as_str() {
                "fal" => {
                    assert_eq!(body, self.spec["input"]);
                    assert_eq!(path, format!("/{}", self.spec["model"].as_str().unwrap()));
                    json!({"request_id":"job123"})
                }
                "minimax" => {
                    assert_eq!(body["model"], *model);
                    assert_eq!(body["prompt"], self.spec["input"]["prompt"]);
                    json!({"task_id":"job123","base_resp":{"status_code":0}})
                }
                "openrouter" | "luma" => {
                    assert_eq!(body["model"], *model);
                    json!({"id":"job123"})
                }
                "bfl" => {
                    assert_eq!(body, self.spec["input"]);
                    json!({"id":"job123","polling_url":"https://api.eu1.bfl.ai/v1/get_result?id=job123"})
                }
                "replicate" => {
                    assert_eq!(body["input"], self.spec["input"]);
                    json!({"id":"job123"})
                }
                "google" => {
                    assert_eq!(body["instances"][0]["prompt"], self.spec["input"]["prompt"]);
                    assert!(body["parameters"].get("prompt").is_none());
                    json!({"name":format!("models/{}/operations/job123",model.as_str().unwrap())})
                }
                "runway" => {
                    assert_eq!(body["promptText"], self.spec["input"]["prompt"]);
                    assert_eq!(body["model"], *model);
                    assert!(body.get("prompt").is_none());
                    json!({"id":"job123"})
                }
                _ => panic!("unexpected submit"),
            }
        } else {
            match self.provider.as_str() {
                "fal" => {
                    if path.ends_with("/status") {
                        json!({"status":"COMPLETED"})
                    } else {
                        json!({"video":{"url":"https://v3.fal.media/video.mp4"}})
                    }
                }
                "minimax" => {
                    if path.ends_with("/retrieve") {
                        json!({"base_resp":{"status_code":0},"file":{"file_id":"file123","download_url":"https://file.cdn.minimax.io/video.mp4"}})
                    } else {
                        json!({"task_id":native,"status":"Success","file_id":"file123","base_resp":{"status_code":0}})
                    }
                }
                "openrouter" => json!({"id":native,"model":model,"status":"completed"}),
                "bfl" => {
                    assert_eq!(u.host_str(), Some("api.eu1.bfl.ai"));
                    json!({"id":native,"status":"Ready","result":{"sample":"https://delivery-eu1.bfl.ai/image.png"}})
                }
                "replicate" => {
                    json!({"id":native,"model":model,"input":self.spec["input"],"status":"succeeded","output":"https://replicate.delivery/video.mp4"})
                }
                "google" => {
                    json!({"name":format!("models/{}/operations/{native}",model.as_str().unwrap()),"done":true,"response":{"generateVideoResponse":{"generatedSamples":[{"video":{"uri":"https://generativelanguage.googleapis.com/v1beta/files/output123:download?alt=media"}}]}}})
                }
                "runway" => {
                    json!({"id":native,"status":"SUCCEEDED","output":["https://media.runwayml.com/video.mp4"]})
                }
                "luma" => {
                    json!({"id":native,"state":"completed","assets":{"video":"https://media.lumalabs.ai/video.mp4"}})
                }
                _ => panic!("unexpected poll"),
            }
        };
        self.calls.borrow_mut().push(r);
        Ok(if media {
            b"synthetic media".to_vec()
        } else {
            v.to_string().into_bytes()
        })
    }
}
fn credentials() -> Value {
    let mut c = json!({});
    for (_, key) in invitation::PROVIDERS {
        c[key] = json!("synthetic-test-key-not-real")
    }
    c
}
fn spec(id: &str) -> Value {
    let mut v = json!({"model":id,"prompt":"Everyframe test"});
    if models::info(id).unwrap()["seed"] == true {
        v["seed"] = json!(42)
    }
    models::spec(&v).unwrap()
}
#[test]
fn all_45_models_complete_through_their_nine_adapters() {
    for (id, m) in models::MODELS.as_object().unwrap() {
        let spec = spec(id);
        let api = Api {
            provider: m["provider"].as_str().unwrap().into(),
            spec: spec.clone(),
            calls: RefCell::default(),
            bad: false,
        };
        let mut router = Router::new(&api, credentials());
        let reference = router.submit(&spec).unwrap();
        assert_eq!(router.poll(&reference, &spec).unwrap(), "COMPLETED", "{id}");
        let output = router.result(&reference, &spec).unwrap();
        assert_eq!(output.media, b"synthetic media", "{id}");
        assert_eq!(output.response_hash.len(), 64);
        let calls = api.calls.borrow();
        assert_eq!(
            calls.iter().filter(|r| r.method == "POST").count(),
            1,
            "{id}"
        );
        if api.provider == "elevenlabs" {
            assert_eq!(calls.len(), 1);
            let body: Value = serde_json::from_slice(calls[0].body.as_ref().unwrap()).unwrap();
            assert_eq!(body["text"], spec["input"]["prompt"]);
        }
    }
}
#[test]
fn missing_disabled_keys_never_submit() {
    let api = Api {
        provider: "fal".into(),
        spec: spec(models::DEFAULT_MODEL),
        calls: RefCell::default(),
        bad: false,
    };
    for c in [json!({}), json!({"FAL_KEY":invitation::DISABLED})] {
        let mut router = Router::new(&api, c);
        assert!(router.models().is_empty());
        assert!(router.submit(&api.spec).is_err());
    }
    assert!(api.calls.borrow().is_empty());
}
#[test]
fn foreign_refs_and_path_injection_are_rejected_before_network() {
    let api = Api {
        provider: "minimax".into(),
        spec: spec("direct/minimax/hailuo-2.3"),
        calls: RefCell::default(),
        bad: false,
    };
    let mut router = Router::new(&api, credentials());
    for reference in [
        "fal_job123",
        "minimax_../secret",
        "minimax_x?token=foo",
        "minimax_",
    ] {
        assert!(router.poll(reference, &api.spec).is_err());
    }
    assert!(api.calls.borrow().is_empty());
}
#[test]
fn provider_response_id_binding() {
    for (id, m) in models::MODELS.as_object().unwrap() {
        let p = m["provider"].as_str().unwrap();
        if ["fal", "elevenlabs"].contains(&p) {
            continue;
        }
        let api = Api {
            provider: p.into(),
            spec: spec(id),
            calls: RefCell::default(),
            bad: true,
        };
        let mut router = Router::new(&api, credentials());
        let reference = router.submit(&api.spec).unwrap();
        assert!(router.poll(&reference, &api.spec).is_err(), "{id}");
    }
}
#[test]
fn synchronous_speech_cannot_resubmit_after_restart() {
    let api = Api {
        provider: "elevenlabs".into(),
        spec: spec("direct/elevenlabs/multilingual-v2-rachel"),
        calls: RefCell::default(),
        bad: false,
    };
    let mut router = Router::new(&api, credentials());
    let reference = router.submit(&api.spec).unwrap();
    let mut restarted = Router::new(&api, credentials());
    assert!(restarted.poll(&reference, &api.spec).is_err());
    assert_eq!(api.calls.borrow().len(), 1);
    assert_ne!(protocol::digest(&api.spec).unwrap(), "");
}
