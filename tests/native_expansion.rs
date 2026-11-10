use everyframe_miner::{
    Result, models, native_media,
    network::{Http, Request},
    protocol::{b64, digest, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture-image";
struct Api {
    m: Value,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn dialogue(&self, mut r: Request, voices: &Value, inputs: &Value) -> Result<Vec<u8>> {
        assert_eq!(r.method, "GET");
        assert!(r.url.contains("/v1/text-to-dialogue/stream-input?"));
        assert!(r.url.contains(self.m["apiModel"].as_str().unwrap()));
        assert!(!voices.as_array().unwrap().is_empty());
        assert_eq!(inputs[0]["text"], "A calm river");
        r.method = "WEBSOCKET".into();
        self.calls.borrow_mut().push(r);
        Ok(b"fixture-audio".to_vec())
    }
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        let provider = self.m["provider"].as_str().unwrap();
        let kind = self.m["kind"].as_str().unwrap();
        let adapter = self.m["adapter"].as_str().unwrap();
        let bytes = if u.host_str() == Some("api.everyframe.studio") {
            assert!(r.headers.is_empty());
            PNG.to_vec()
        } else if [
            "output.klingai.com",
            "storage.cdn-luma.com",
            "output.runwayml.com",
        ]
        .contains(&u.host_str().unwrap())
        {
            assert!(r.headers.is_empty());
            if kind == "image" {
                PNG.to_vec()
            } else {
                b"fixture-video".to_vec()
            }
        } else {
            let key = if provider == "elevenlabs" {
                "xi-api-key"
            } else {
                "authorization"
            };
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == key && v.contains("fixture-secret"))
            );
            if provider == "runway" {
                assert!(
                    r.headers
                        .iter()
                        .any(|(k, v)| k == "X-Runway-Version" && v == "2024-11-06")
                );
            }
            if r.method == "POST" {
                let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
                assert!(!String::from_utf8_lossy(r.body.as_ref().unwrap()).contains("@reference:"));
                if provider == "elevenlabs" {
                    assert_eq!(body["model_id"], self.m["apiModel"]);
                    assert!(!body.get("output_format").is_some());
                    b"fixture-audio".to_vec()
                } else if adapter == "schema-minimax-image" {
                    assert_eq!(body["model"], "image-01");
                    assert_eq!(body["response_format"], "base64");
                    json!({"base_resp":{"status_code":0},"data":{"image_base64":[b64(PNG)]}})
                        .to_string()
                        .into_bytes()
                } else if adapter == "schema-minimax-tts" {
                    assert_eq!(body["model"], self.m["apiModel"]);
                    assert_eq!(body["output_format"], "hex");
                    json!({"base_resp":{"status_code":0},"data":{"audio":hex::encode(b"fixture-audio")}}).to_string().into_bytes()
                } else if provider == "kling" {
                    assert!(body["prompt"].is_string() || body["contents"][0]["text"].is_string());
                    json!({"code":0,"data":{"id":"job123","task_id":"job123"}})
                        .to_string()
                        .into_bytes()
                } else {
                    assert_eq!(body["model"], self.m["apiModel"]);
                    json!({"id":"job123"}).to_string().into_bytes()
                }
            } else {
                let d = match provider {
                    "kling" if kind == "image" => {
                        json!({"code":0,"data":{"task_id":"job123","task_status":"succeed","task_result":{"images":[{"url":"https://output.klingai.com/file"}]}}})
                    }
                    "kling" => {
                        json!({"code":0,"data":[{"id":"job123","status":"succeeded","outputs":[{"type":"video","url":"https://output.klingai.com/file"}]}]})
                    }
                    "runway" => {
                        json!({"id":"job123","status":"SUCCEEDED","output":["https://output.runwayml.com/file"]})
                    }
                    "luma-agents" => {
                        json!({"id":"job123","model":self.m["apiModel"],"state":"completed","output":[{"type":kind,"url":"https://storage.cdn-luma.com/file"}]})
                    }
                    _ => panic!("unexpected fixture request"),
                };
                d.to_string().into_bytes()
            }
        };
        self.calls.borrow_mut().push(r);
        Ok(bytes)
    }
}
#[test]
fn native_expansion_submits_once_and_binds_recovery_to_the_input() {
    let mut n = 0;
    for (id, m) in native_media::MODELS.as_object().unwrap() {
        if !m["nativeSchema"].is_object()
            || ["schema-bfl", "schema-minimax-video", "schema-wavespeed"]
                .contains(&m["adapter"].as_str().unwrap())
        {
            continue;
        }
        let mut refs = m["exampleReferences"].clone();
        for r in refs.as_array_mut().unwrap() {
            r["bytes"] = json!(PNG.len());
            r["sha256"] = json!(sha(PNG));
        }
        let spec = models::spec(&json!({"model":id,"prompt":"A calm river","references":refs}))
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        digest(&spec).unwrap();
        let provider = m["provider"].as_str().unwrap();
        let key = everyframe_miner::invitation::PROVIDERS
            .iter()
            .find(|(p, _)| *p == provider)
            .unwrap()
            .1;
        let mut keys = json!({});
        keys[key] = json!("fixture-secret");
        let api = Api {
            m: m.clone(),
            calls: RefCell::default(),
        };
        let mut router = Router::new(&api, keys.clone());
        let reference = router
            .submit(&spec)
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        let sync = provider == "elevenlabs"
            || ["schema-minimax-tts", "schema-minimax-image"]
                .contains(&m["adapter"].as_str().unwrap());
        if sync {
            assert!(Router::new(&api, keys).poll(&reference, &spec).is_err());
        } else {
            router = Router::new(&api, keys);
        }
        assert_eq!(router.poll(&reference, &spec).unwrap(), "COMPLETED");
        let output = router
            .result(&reference, &spec)
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        let data: Value = serde_json::from_slice(&output.media).unwrap();
        assert_eq!(data["parts"][0]["type"], m["kind"]);
        let mut wrong = spec.clone();
        wrong["input"]["prompt"] = json!("Different prompt");
        assert!(router.poll(&reference, &wrong).is_err());
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST" || r.method == "WEBSOCKET")
                .count(),
            1
        );
        n += 1;
    }
    assert_eq!(n, 43);
}
#[test]
fn invalid_unowned_references_and_conflicting_settings_fail_before_billing() {
    for v in [
        json!({"model":"studio/native/kling/kling-4.0","prompt":"A river","parameters":{"contents":[{"type":"prompt"},{"type":"refer_image","url":"https://example.com/image.png"}]}}),
        json!({"model":"studio/native/minimax/image-01","prompt":"A river","parameters":{"width":1024}}),
        json!({"model":"studio/native/luma-agents/ray-3.2","prompt":"A river","parameters":{"video":{"duration":"10s","hdr":true}}}),
        json!({"model":"studio/native/runway/gen4.5","prompt":"A river","parameters":{"model":"other"}}),
    ] {
        assert!(models::spec(&v).is_err(), "{v}");
    }
}
