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
                "kling" => {
                    assert_eq!(u.host_str(), Some("api-singapore.klingai.com"));
                    assert_eq!(path, "/text-to-video/kling-3.0");
                    let mut settings = self.spec["input"].clone();
                    settings.as_object_mut().unwrap().remove("prompt");
                    assert_eq!(
                        body,
                        json!({"prompt":self.spec["input"]["prompt"],"settings":settings,"options":{"watermark_info":{"enabled":false}}})
                    );
                    json!({"code":0,"data":{"id":"job123","status":"submitted"}})
                }
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
                "vivix" => {
                    assert_eq!(u.host_str(), Some("api.vivix.ai"));
                    assert_eq!(path, "/v1/videos/jobs");
                    let mut input = self.spec["input"].clone();
                    input.as_object_mut().unwrap().remove("seed");
                    assert_eq!(
                        body,
                        json!({"model":model,"seed":self.spec["input"]["seed"],"jobs":[input]})
                    );
                    json!({"code":0,"data":{"results":[{"job_id":"job123","status":"queued"}]}})
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
                "kling" => {
                    assert_eq!(u.host_str(), Some("api-singapore.klingai.com"));
                    assert_eq!(path, "/tasks");
                    assert_eq!(u.query(), Some("task_ids=job123"));
                    json!({"code":0,"data":[{"id":native,"status":"succeeded","outputs":[{"type":"video","url":"https://p1-kling.klingai.com/output.mp4"}]}]})
                }
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
                "vivix" => {
                    assert_eq!(u.host_str(), Some("api.vivix.ai"));
                    assert_eq!(path, "/v1/videos/jobs/job123");
                    json!({"code":0,"data":{"job_id":native,"status":"succeeded","duration":self.spec["input"]["duration"],"aspect_ratio":"16:9","video_url":"https://static.vivi-x.ai/clips/job123/job123.mp4"}})
                }
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
                    json!({"id":native,"state":"completed","assets":{"video":"https://storage.cdn-luma.com/video.mp4"}})
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
    let mut v = json!({"model":id,"prompt":"EveryFrame test"});
    if models::info(id).unwrap()["seed"] == true {
        v["seed"] = json!(42)
    }
    models::spec(&v).unwrap()
}
#[test]
fn all_models_complete_through_their_provider_adapters() {
    for (id, m) in models::MODELS.as_object().unwrap() {
        let spec = spec(id);
        let api = Api {
            provider: m["provider"].as_str().unwrap().into(),
            spec: spec.clone(),
            calls: RefCell::default(),
            bad: false,
        };
        let mut router = Router::new(&api, credentials());
        if everyframe_miner::model_availability::retired(id).is_some() {
            let error = router.submit(&spec).unwrap_err();
            assert_eq!(error.0, "provider_model_unavailable");
            assert_eq!(
                everyframe_miner::submission::rejected(error.0),
                Some("provider_model_unavailable")
            );
            assert!(api.calls.borrow().is_empty());
            continue;
        }
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

#[test]
fn expansion_preserves_original_registry_and_is_fixed_text_only() {
    let original: Value = serde_json::from_str(include_str!("../config/models.json")).unwrap();
    for (id, contract) in original.as_object().unwrap() {
        assert_eq!(models::info(id).unwrap(), contract, "{id}");
    }
    let added: Value =
        serde_json::from_str(include_str!("../config/provider-expansion.json")).unwrap();
    assert_eq!(added.as_object().unwrap().len(), 15);
    for (id, m) in added.as_object().unwrap() {
        assert!(original.get(id).is_none());
        assert!(
            invitation::PROVIDERS
                .iter()
                .any(|(p, _)| m["provider"] == *p)
        );
        assert_eq!(m["validationStatus"], "documentation-reviewed");
        let value = spec(id);
        models::check(&value).unwrap();
        for (field, extra) in [
            ("callback_url", json!("https://evil.invalid")),
            ("input_images", json!(["https://evil.invalid"])),
            ("resolution", json!("4k")),
            ("model", json!("arbitrary")),
        ] {
            let mut altered = value.clone();
            altered["input"][field] = extra;
            assert!(models::check(&altered).is_err(), "{id}: {field}");
        }
    }
}

#[test]
fn openrouter_failure_with_unassigned_model_is_terminal() {
    struct Failed;
    impl Http for Failed {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.method, "GET");
            Ok(json!({"id":"job123","model":null,"status":"failed","error":"Content policy violation"}).to_string().into_bytes())
        }
    }
    let mut router = Router::new(&Failed, credentials());
    assert_eq!(
        router
            .poll("openrouter_job123", &spec("direct/openrouter/veo-3.1-fast"))
            .unwrap_err()
            .0,
        "provider_content_rejected"
    );
}

#[test]
fn every_added_model_checks_deadline_and_does_not_retry_ambiguous_submission() {
    struct Timeout(std::cell::Cell<usize>);
    impl Http for Timeout {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.method, "POST");
            assert!(r.submit_deadline.is_some());
            self.0.set(self.0.get() + 1);
            Err(everyframe_miner::Error("request_timeout"))
        }
    }
    let added: Value =
        serde_json::from_str(include_str!("../config/provider-expansion.json")).unwrap();
    for id in added.as_object().unwrap().keys() {
        let api = Timeout(std::cell::Cell::new(0));
        let mut router = Router::new(&api, credentials());
        assert_eq!(
            router.submit_before(&spec(id), 0).unwrap_err().0,
            "submission_deadline_elapsed"
        );
        assert_eq!(api.0.get(), 0, "{id}");
        let error = router
            .submit_before(&spec(id), everyframe_miner::now() + 60_000)
            .unwrap_err();
        assert_eq!(error.0, "request_timeout");
        assert_eq!(everyframe_miner::submission::rejected(error.0), None);
        assert_eq!(api.0.get(), 1, "{id}");
    }
}

#[test]
fn expanded_speech_is_bound_to_input_and_cannot_be_recovered_by_resubmitting() {
    for (id, m) in models::MODELS.as_object().unwrap() {
        if m["provider"] != "elevenlabs" {
            continue;
        }
        let s = spec(id);
        let api = Api {
            provider: "elevenlabs".into(),
            spec: s.clone(),
            calls: RefCell::default(),
            bad: false,
        };
        let mut router = Router::new(&api, credentials());
        let r = router.submit(&s).unwrap();
        let changed = models::spec(&json!({"model":id,"prompt":"Different words"})).unwrap();
        assert!(router.result(&r, &changed).is_err());
        let mut restarted = Router::new(&api, credentials());
        assert_eq!(
            restarted.poll(&r, &s).unwrap_err().0,
            "synchronous_result_unavailable"
        );
        assert!(restarted.result(&r, &s).is_err());
        assert_eq!(api.calls.borrow().len(), 1);
    }
}
