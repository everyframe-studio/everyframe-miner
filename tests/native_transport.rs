use everyframe_miner::{
    Error, Result, models, native_media,
    network::{Http, Request},
    protocol::{b64, digest},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nsynthetic-png";
struct Api {
    model: Value,
    calls: RefCell<Vec<Request>>,
    mode: &'static str,
}
impl Http for Api {
    fn bytes(&self, req: Request) -> Result<Vec<u8>> {
        let url = url::Url::parse(&req.url).unwrap();
        let post = req.method == "POST";
        let adapter = self.model["adapter"].as_str().unwrap();
        if req.url.contains("everyframe.studio") {
            self.calls.borrow_mut().push(req);
            return Ok(b"tampered".to_vec());
        }
        if url.host_str() == Some("imgen.x.ai")
            || url.host_str() == Some("vidgen.x.ai")
            || url.host_str() == Some("v3.fal.media")
        {
            assert!(req.headers.is_empty());
            assert!(req.redirect_hosts.is_empty());
            self.calls.borrow_mut().push(req);
            return Ok(if adapter == "xai-images" || adapter == "gpt-image-26" {
                PNG.to_vec()
            } else {
                b"synthetic-mp4".to_vec()
            });
        }
        assert_eq!(
            url.host_str(),
            Some(match self.model["provider"].as_str().unwrap() {
                "openai" => "api.openai.com",
                "xai" => "api.x.ai",
                "openrouter" => "openrouter.ai",
                "fal" => "queue.fal.run",
                other => panic!("Add a transport fixture for {other}"),
            })
        );
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                    && v == if self.model["provider"] == "fal" {
                        "Key fixture-secret"
                    } else {
                        "Bearer fixture-secret"
                    })
        );
        let response = if post {
            let body: Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
            if self.mode == "timeout" {
                self.calls.borrow_mut().push(req);
                return Err(Error("network_timeout"));
            }
            match adapter {
                "gpt-image-26" if self.model["provider"] == "fal" => {
                    assert_eq!(body["num_images"], 1);
                    assert_eq!(body["reasoning_effort"], "high");
                    json!({"request_id":"job_123"})
                }
                "openai-images" | "gpt-image-26" => {
                    assert_eq!(body["model"], self.model["apiModel"]);
                    json!({"data":[{"b64_json":b64(PNG)}]})
                }
                "openai-audio" => {
                    json!({"model":self.model["apiModel"],"choices":[{"message":{"audio":{"data":b64(b"synthetic-wav"),"transcript":"hello"}}}]})
                }
                "xai-tts" => {
                    assert!(body.get("model").is_none());
                    self.calls.borrow_mut().push(req);
                    return Ok(b"synthetic-mp3".to_vec());
                }
                _ => {
                    if adapter == "xai-images" {
                        assert_eq!(body["deferred"], true);
                    }
                    json!({"request_id":"job_123"})
                }
            }
        } else if self.model["provider"] == "fal" {
            if url.path().ends_with("/status") {
                assert!(url.path().ends_with("/requests/job_123/status"));
                json!({"request_id":"job_123","status":"COMPLETED"})
            } else {
                assert!(url.path().ends_with("/requests/job_123"));
                json!({"images":[{"url":"https://v3.fal.media/image.png","content_type":"image/png"}]})
            }
        } else {
            assert!(url.path().ends_with("/job_123"));
            if self.mode == "pending" {
                json!({"request_id":"job_123","status":"pending"})
            } else if self.mode == "wrong-id" {
                json!({"request_id":"other_job","status":"done"})
            } else if self.mode == "failed" {
                json!({"request_id":"job_123","status":"failed","error":{"message":"secret provider text"}})
            } else if adapter == "xai-images" {
                json!({"request_id":"job_123","status":"done","data":[{"url":if self.mode=="evil-cdn" {"https://evil.example/a.png"}else{"https://imgen.x.ai/a.png"},"mime_type":"image/png"}]})
            } else {
                json!({"status":"done","model":self.model["apiModel"],"video":{"url":"https://vidgen.x.ai/a.mp4"}})
            }
        };
        self.calls.borrow_mut().push(req);
        Ok(response.to_string().into_bytes())
    }
}
fn api(id: &str, mode: &'static str) -> Api {
    Api {
        model: native_media::MODELS[id].clone(),
        calls: RefCell::default(),
        mode,
    }
}
fn credentials() -> Value {
    json!({"OPENAI_API_KEY":"fixture-secret","XAI_API_KEY":"fixture-secret","OPENROUTER_API_KEY":"fixture-secret","FAL_KEY":"fixture-secret"})
}
fn spec(id: &str) -> Value {
    models::spec(&json!({"model":id,"prompt":"A quiet lake"})).unwrap()
}
#[test]
fn every_native_model_submits_once_and_returns_a_bound_bundle() {
    for (id, _) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| !m["nativeSchema"].is_object())
    {
        let api = api(id, "done");
        let spec = spec(id);
        let mut worker = Router::new(&api, credentials());
        assert!(worker.models().iter().any(|x| x == id));
        let r = worker.submit(&spec).unwrap();
        assert_eq!(worker.poll(&r, &spec).unwrap(), "COMPLETED");
        let output = worker.result(&r, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&output.media).unwrap();
        assert_eq!(bundle["kind"], "native-media-bundle");
        assert_eq!(bundle["parts"].as_array().unwrap().len(), 1);
        assert_eq!(output.response_hash.len(), 64);
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1,
            "{id}"
        );
    }
}
#[test]
fn deferred_jobs_recover_with_get_only_but_sync_results_are_never_replayed() {
    for (id, model) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| !m["nativeSchema"].is_object())
    {
        let api = api(id, "done");
        let spec = spec(id);
        let r = Router::new(&api, credentials()).submit(&spec).unwrap();
        let mut restarted = Router::new(&api, credentials());
        if ["xai-images", "xai-videos"].contains(&model["adapter"].as_str().unwrap())
            || model["provider"] == "fal"
        {
            assert_eq!(restarted.poll(&r, &spec).unwrap(), "COMPLETED");
            restarted.result(&r, &spec).unwrap();
        } else {
            assert_eq!(
                restarted.poll(&r, &spec).unwrap_err().0,
                "synchronous_result_unavailable"
            );
        }
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
fn invalid_or_missing_credentials_never_submit_and_timeouts_never_retry() {
    for id in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| !m["nativeSchema"].is_object())
        .map(|(id, _)| id)
    {
        let api = api(id, "timeout");
        let spec = spec(id);
        assert_eq!(
            Router::new(&api, json!({})).submit(&spec).unwrap_err().0,
            "provider_key_required"
        );
        assert!(api.calls.borrow().is_empty());
        assert_eq!(
            Router::new(&api, credentials())
                .submit(&spec)
                .unwrap_err()
                .0,
            "network_timeout"
        );
        assert_eq!(api.calls.borrow().len(), 1);
    }
}
#[test]
fn references_are_verified_before_post_and_cdn_credentials_never_escape() {
    let id = "direct/xai/grok-imagine-image-2.0";
    let api = api(id, "done");
    let spec=models::spec(&json!({"model":id,"prompt":"edit","references":[{"url":format!("https://api.everyframe.studio/v1/attachment/att_test?t={}","a".repeat(32)),"bytes":8,"sha256":"a".repeat(64),"mime":"image/png","role":"image"}]})).unwrap();
    assert_eq!(
        Router::new(&api, credentials())
            .submit(&spec)
            .unwrap_err()
            .0,
        "reference_integrity_mismatch"
    );
    assert!(api.calls.borrow().iter().all(|r| r.method == "GET"));
    for mode in ["wrong-id", "failed", "evil-cdn"] {
        let api = crate::api(id, mode);
        let spec = crate::spec(id);
        let mut w = Router::new(&api, credentials());
        let r = w.submit(&spec).unwrap();
        assert!(w.result(&r, &spec).is_err());
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
fn fractional_provider_settings_remain_signable_and_reach_the_provider_as_numbers() {
    let spec =
        models::spec(&json!({"model":"direct/xai/tts","prompt":"hi","parameters":{"speed":0.85}}))
            .unwrap();
    assert_eq!(spec["input"]["parameters"]["speed"], "0.85");
    digest(&spec).unwrap();
    assert_eq!(native_media::body(&spec, &[]).unwrap().1["speed"], 0.85);
}
