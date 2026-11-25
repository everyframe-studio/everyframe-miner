use everyframe_miner::{
    Result, invitation, models,
    network::{Http, Request, safe_url},
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;

const SEEDANCE_GATEWAYS: [&str; 5] = ["vidlab", "revana", "higgsfield", "kie", "piapi"];
const MP4_FIXTURE: &[u8] = b"\0\0\0\x18ftypisom";

struct Api {
    provider: String,
    spec: Value,
    calls: RefCell<Vec<Request>>,
    bad: bool,
}
impl Http for Api {
    fn dialogue(&self, mut r: Request, voices: &Value, inputs: &Value) -> Result<Vec<u8>> {
        assert_eq!(r.method, "GET");
        assert!(
            r.url
                .starts_with("https://api.elevenlabs.io/v1/text-to-dialogue/stream-input?")
        );
        assert_eq!(voices[0], self.spec["input"]["voice_id"]);
        assert_eq!(inputs[0]["text"], self.spec["input"]["prompt"]);
        r.method = "WEBSOCKET".into();
        r.body = Some(json!({"text":inputs[0]["text"]}).to_string().into_bytes());
        self.calls.borrow_mut().push(r);
        Ok(b"synthetic media".to_vec())
    }
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
        let modern_google = self.provider == "google"
            && models::info(self.spec["model"].as_str().unwrap()).unwrap()["interaction"] == true;
        let media = r.limit >= 10 * 1024 * 1024 && !modern_google;
        let v = if modern_google {
            assert_eq!(u.host_str(), Some("generativelanguage.googleapis.com"));
            assert!(r.headers.iter().any(|(k, _)| k == "x-goog-api-key"));
            let m = models::info(self.spec["model"].as_str().unwrap()).unwrap();
            if r.method == "POST" {
                assert_eq!(path, "/v1beta/interactions");
                assert_eq!(body, everyframe_miner::google::body(&self.spec, m).unwrap());
                json!({"id":"job123","model":model,"status":"in_progress"})
            } else {
                assert_eq!(path, "/v1beta/interactions/job123");
                json!({"id":native,"model":model,"status":"completed","steps":[{"type":"model_output","content":[{"type":m["kind"],"mime_type":m["mime"],"data":"c3ludGhldGljIG1lZGlh"}]}]})
            }
        } else if media {
            if self.provider == "google" {
                assert!(r.headers.iter().any(|(k, _)| k == "x-goog-api-key"));
                assert!(r.redirect_hosts.contains(&"*.googleusercontent.com".into()));
            } else if ["vidlab", "revana"].contains(&self.provider.as_str()) {
                assert_eq!(path, "/v1/generations/job123/content");
                assert!(
                    r.headers
                        .iter()
                        .any(|(k, v)| k == "authorization" && v.starts_with("Bearer "))
                );
                assert!(!r.redirect_hosts.is_empty());
            } else if !["elevenlabs", "openrouter", "revana", "vidlab"]
                .contains(&self.provider.as_str())
            {
                assert!(r.headers.is_empty(), "credentials must not reach CDN");
            }
            json!(null)
        } else if r.method == "POST" {
            assert!(!r.headers.is_empty());
            match self.provider.as_str() {
                "vidlab" | "revana" => {
                    assert_eq!(path, "/v1/generations");
                    assert_eq!(body["max_cost_usd"], 1.05);
                    assert_eq!(body["model"], "seedance-2.5");
                    assert_eq!(body["input"], self.spec["input"]);
                    assert_eq!(
                        body["input"][if self.provider == "vidlab" {
                            "generate_audio"
                        } else {
                            "audio"
                        }],
                        false
                    );
                    assert!(r.headers.iter().any(|(k, _)| k == "Idempotency-Key"));
                    if self.provider == "vidlab" {
                        json!({"data":{"id":"job123"}})
                    } else {
                        json!({"id":"job123"})
                    }
                }
                "higgsfield" => {
                    if model == "sand/magi-2/text-to-video" {
                        assert_eq!(path, "/sand/magi-2/text-to-video");
                        assert_eq!(body["generate_audio"], true);
                        assert_eq!(body["duration"], self.spec["input"]["duration"]);
                    } else {
                        assert_eq!(path, "/bytedance/seedance-2.5/text-to-video");
                        assert_eq!(body, self.spec["input"]);
                    }
                    assert!(
                        r.headers
                            .iter()
                            .any(|(k, v)| k == "authorization" && v.starts_with("Key "))
                    );
                    json!({"request_id":"job123"})
                }
                "kie" => {
                    assert_eq!(path, "/api/v1/jobs/createTask");
                    assert_eq!(
                        body,
                        json!({"model":"bytedance/seedance-2-5","input":self.spec["input"]})
                    );
                    json!({"code":200,"data":{"taskId":"job123"}})
                }
                "piapi" => {
                    assert_eq!(path, "/api/v1/task");
                    assert!(r.headers.iter().any(|(k, _)| k == "X-API-Key"));
                    assert_eq!(
                        body,
                        json!({"model":"seedance","task_type":"seedance-2.5","input":self.spec["input"]})
                    );
                    json!({"code":200,"data":{"task_id":"job123"}})
                }
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
                "vidlab" | "revana" => {
                    assert_eq!(path, "/v1/generations/job123");
                    let job = json!({"id":native,"status":"succeeded","model":"seedance-2.5"});
                    if self.provider == "vidlab" {
                        json!({"data":job})
                    } else {
                        job
                    }
                }
                "higgsfield" => {
                    assert_eq!(path, "/requests/job123/status");
                    json!({"request_id":native,"status":"completed","video":{"url":"https://media.higgsfield.ai/example.mp4"}})
                }
                "kie" => {
                    assert_eq!(path, "/api/v1/jobs/recordInfo");
                    assert_eq!(u.query(), Some("taskId=job123"));
                    json!({"code":200,"data":{"taskId":native,"state":"success","resultJson":json!({"resultUrls":["https://tempfile.aiquickdraw.com/example.mp4"]}).to_string()}})
                }
                "piapi" => {
                    assert_eq!(path, "/api/v1/task/job123");
                    json!({"code":200,"data":{"task_id":native,"status":"completed","output":{"video":"https://img.theapi.app/example.mp4"}}})
                }
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
        Ok(
            if media && SEEDANCE_GATEWAYS.contains(&self.provider.as_str()) {
                MP4_FIXTURE.to_vec()
            } else if media {
                b"synthetic media".to_vec()
            } else {
                v.to_string().into_bytes()
            },
        )
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
    for (id, m) in models::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["nativeMedia"] != true)
    {
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
        let expected: &[u8] = if SEEDANCE_GATEWAYS.contains(&api.provider.as_str()) {
            MP4_FIXTURE
        } else {
            b"synthetic media"
        };
        assert_eq!(output.media, expected, "{id}");
        assert_eq!(output.response_hash.len(), 64);
        let calls = api.calls.borrow();
        assert_eq!(
            calls
                .iter()
                .filter(|r| r.method == "POST" || r.method == "WEBSOCKET")
                .count(),
            1,
            "{id}"
        );
        if api.provider == "elevenlabs" {
            assert_eq!(calls.len(), 1);
            let body: Value = serde_json::from_slice(calls[0].body.as_ref().unwrap()).unwrap();
            assert_eq!(
                if body["text"].is_null() {
                    &body["inputs"][0]["text"]
                } else {
                    &body["text"]
                },
                &spec["input"]["prompt"]
            );
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
    for (id, m) in models::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["nativeMedia"] != true)
    {
        let p = m["provider"].as_str().unwrap();
        if ["fal", "elevenlabs"].contains(&p)
            || everyframe_miner::model_availability::retired(id).is_some()
        {
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
    assert_eq!(added.as_object().unwrap().len(), 20);
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
        fn dialogue(&self, r: Request, _voices: &Value, _inputs: &Value) -> Result<Vec<u8>> {
            assert_eq!(r.method, "GET");
            assert!(r.submit_deadline.is_some());
            self.0.set(self.0.get() + 1);
            Err(everyframe_miner::Error("request_timeout"))
        }
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            if r.url == "https://openrouter.ai/api/v1/key" {
                assert_eq!(r.method, "GET");
                return Ok(json!({"data":{"limit":null,"limit_remaining":null,"budgets":[],"effective_budget":null}}).to_string().into_bytes());
            }
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
        if everyframe_miner::model_availability::retired(id).is_some() {
            assert_eq!(
                router.submit_before(&spec(id), 0).unwrap_err().0,
                "provider_model_unavailable"
            );
            assert_eq!(api.0.get(), 0, "{id}");
            continue;
        }
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
    for (id, m) in models::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["nativeMedia"] != true)
    {
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

#[test]
fn seedance_key_checks_use_free_account_or_estimate_endpoints() {
    use everyframe_miner::key_check::{Verdict, check};
    struct Probe(&'static str, &'static str, Value);
    impl Http for Probe {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.url, self.0);
            assert_eq!(r.method, self.1);
            assert!(r.redirect_hosts.is_empty());
            Ok(self.2.to_string().into_bytes())
        }
    }
    for (provider, url, method, body) in [
        (
            "vidlab",
            "https://vidlab.ai/v1/balance",
            "GET",
            json!({"data":{"available_usd":0}}),
        ),
        (
            "revana",
            "https://api.revana.si/v1/balance",
            "GET",
            json!({"available_usd":0}),
        ),
        (
            "kie",
            "https://api.kie.ai/api/v1/chat/credit",
            "GET",
            json!({"code":200,"data":0}),
        ),
        (
            "piapi",
            "https://api.piapi.ai/account/info",
            "GET",
            json!({"data":{"equivalent_in_usd":0}}),
        ),
        (
            "higgsfield",
            "https://api.higgsfield.ai/estimate/bytedance/seedance-2.5/text-to-video",
            "POST",
            json!({"usd":"0.5885"}),
        ),
    ] {
        assert_eq!(
            check(&Probe(url, method, body), provider, "fixture-key").0,
            Verdict::Valid
        );
        assert_eq!(
            check(&Probe(url, method, json!({})), provider, "fixture-key").0,
            Verdict::Unverified
        );
    }
}

#[test]
fn seedance_gateway_failures_preserve_unknown_submissions_and_moderation() {
    struct Reply(Value);
    impl Http for Reply {
        fn bytes(&self, _: Request) -> Result<Vec<u8>> {
            Ok(self.0.to_string().into_bytes())
        }
    }
    for provider in ["kie", "piapi"] {
        let spec = spec(&format!("direct/{provider}/seedance-2.5"));
        for (code, category) in [
            (401, Some("provider_auth_rejected")),
            (402, Some("provider_billing_required")),
            (429, Some("provider_capacity_limited")),
            (500, None),
        ] {
            let http = Reply(json!({"code":code}));
            let error = Router::new(&http, credentials()).submit(&spec).unwrap_err();
            assert_eq!(everyframe_miner::submission::rejected(error.0), category);
        }
    }
    let http = Reply(json!({"request_id":"task","status":"nsfw"}));
    let err = Router::new(&http, credentials())
        .poll("higgsfield_task", &spec("direct/higgsfield/seedance-2.5"))
        .unwrap_err();
    assert_eq!(err.0, "provider_content_rejected");
}

#[test]
fn higgsfield_magi_task_is_submitted_polled_and_downloaded() {
    let spec = json!({"model":"studio/higgsfield/sand/magi-2/text-to-video","input":{
        "seed":42,"prompt":"Blue waves","aspect":"16:9","resolution":"1080p","duration":5,
        "references":[],"promptExpansion":"balanced"}});
    let api = Api {
        provider: "higgsfield".into(),
        spec: spec.clone(),
        calls: RefCell::default(),
        bad: false,
    };
    let mut router = Router::new(&api, credentials());
    let id = router.submit(&spec).unwrap();
    assert_eq!(id, "higgsfield_job123");
    assert_eq!(router.poll(&id, &spec).unwrap(), "COMPLETED");
    assert!(!router.result(&id, &spec).unwrap().media.is_empty());
}
