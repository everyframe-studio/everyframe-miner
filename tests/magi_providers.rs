use everyframe_miner::{
    Result,
    network::{Http, Request},
    protocol::sha,
    providers::{Provider, Router},
    studio_video,
};
use serde_json::{Value, json};
use std::cell::RefCell;

const IMAGE: &[u8] = b"fixture image";
const VIDEO: &[u8] = b"\0\0\0\x18ftypisomfixture-video";
const URL: &str =
    "https://api.everyframe.studio/v1/attachment/att_frame?t=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROUTES: &[(&str, &str, &str)] = &[
    (
        "wavespeed",
        "WAVESPEED_API_KEY",
        "sand-ai/magi-2/text-to-video",
    ),
    ("kie", "KIE_API_KEY", "magi-2/text-to-video"),
    ("nanoapi", "NANOAPI_API_KEY", "magi-2.0"),
    ("revana", "REVANA_API_KEY", "magi-2"),
];

fn frame(role: &str) -> Value {
    json!({"url":URL,"sha256":sha(IMAGE),"bytes":IMAGE.len(),"mime":"image/png",
        "role":role,"durationMs":0,"width":1920,"height":1080})
}

fn spec(provider: &str, model: &str, refs: Vec<Value>) -> Value {
    json!({"model":format!("studio/{provider}/{model}"),"input":{
        "prompt":"Ripples on a lake","seed":42,"aspect":"16:9","resolution":"1080p",
        "duration":5,"references":refs,"promptExpansion":"balanced"}})
}

struct Api {
    provider: &'static str,
    calls: RefCell<Vec<Request>>,
}

impl Http for Api {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        self.calls.borrow_mut().push(request.clone());
        if request.url == URL {
            assert!(request.headers.is_empty());
            return Ok(IMAGE.into());
        }
        if request.url.ends_with("/output.mp4") || request.url.ends_with("/content") {
            if self.provider == "revana" {
                assert_eq!(
                    request.url,
                    "https://api.revana.si/v1/generations/test_1/content"
                );
            } else {
                assert!(request.headers.is_empty());
            }
            return Ok(VIDEO.into());
        }
        assert!(
            request
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "Bearer fixture-key")
        );
        let result = if request.method == "POST" {
            let body: Value = serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
            match self.provider {
                "wavespeed" => {
                    assert_eq!(
                        request.url,
                        "https://api.wavespeed.ai/api/v3/sand-ai/magi-2/image-to-video"
                    );
                    assert_eq!(body["image"], URL);
                    assert_eq!(body["last_image"], URL);
                    assert!(body.get("resolution").is_none());
                    json!({"code":200,"data":{"id":"test_1","model":"sand-ai/magi-2/image-to-video","status":"created"}})
                }
                "kie" => {
                    assert!(request.url.ends_with("/api/v1/jobs/createTask"));
                    assert_eq!(body["model"], "magi-2/image-to-video");
                    assert_eq!(body["input"]["first_frame_url"], URL);
                    assert_eq!(body["input"]["last_frame_url"], URL);
                    json!({"code":200,"data":{"taskId":"test_1"}})
                }
                "nanoapi" => {
                    assert!(request.url.ends_with("/v1/tasks"));
                    assert_eq!(body["model"], "magi-2.0");
                    assert_eq!(body["mode"], "image_to_video");
                    assert_eq!(body["image_urls"], json!([URL, URL]));
                    json!({"code":200,"data":{"id":"test_1","model":"magi-2.0","status":"queued"}})
                }
                "revana" => {
                    assert!(request.url.ends_with("/v1/generations"));
                    assert_eq!(body["model"], "magi-2");
                    assert_eq!(body["input"]["image_url"], URL);
                    assert_eq!(body["input"]["end_image_url"], URL);
                    assert_eq!(body["input"]["audio"], true);
                    assert_eq!(body["max_cost_usd"], 0.55);
                    assert!(request.headers.iter().any(|(k, _)| k == "Idempotency-Key"));
                    json!({"id":"test_1"})
                }
                _ => unreachable!(),
            }
        } else {
            match self.provider {
                "wavespeed" => {
                    assert!(request.url.ends_with("/api/v3/predictions/test_1/result"));
                    json!({"code":200,"data":{"id":"test_1","model":"sand-ai/magi-2/image-to-video","status":"completed","outputs":["https://cdn.wavespeed.ai/output.mp4"]}})
                }
                "kie" => {
                    assert!(
                        request
                            .url
                            .ends_with("/api/v1/jobs/recordInfo?taskId=test_1")
                    );
                    json!({"code":200,"data":{"taskId":"test_1","model":"magi-2/image-to-video","state":"success","resultJson":"{\"resultUrls\":[\"https://tempfile.aiquickdraw.com/output.mp4\"]}"}})
                }
                "nanoapi" => {
                    assert_eq!(request.url, "https://api.nanoapi.ai/v1/tasks/test_1");
                    json!({"code":200,"data":{"id":"test_1","model":"magi-2.0","status":"succeeded","outputs":[{"url":"https://cdn.nanoapi.ai/output.mp4"}]}})
                }
                "revana" => json!({"id":"test_1","model":"magi-2","status":"succeeded"}),
                _ => unreachable!(),
            }
        };
        Ok(result.to_string().into_bytes())
    }
}

#[test]
fn magi_keyframes_submit_once_and_deliver_with_provider_specific_contracts() {
    for &(provider, key, model) in ROUTES {
        let input = spec(provider, model, vec![frame("first"), frame("last")]);
        let api = Api {
            provider,
            calls: RefCell::default(),
        };
        let mut router = Router::new(&api, json!({(key):"fixture-key"}));
        let reference = router.submit(&input).unwrap();
        assert_eq!(reference, format!("{provider}_test_1"));
        assert_eq!(router.poll(&reference, &input).unwrap(), "COMPLETED");
        assert_eq!(router.result(&reference, &input).unwrap().media, VIDEO);
        let calls = api.calls.borrow();
        assert_eq!(calls.iter().filter(|call| call.method == "POST").count(), 1);
        if provider == "nanoapi" {
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.url.ends_with("/v1/tasks/test_1"))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn magi_reference_modes_keep_provider_specific_fields() {
    let mut video = frame("video");
    video["mime"] = json!("video/mp4");
    video["durationMs"] = json!(2000);
    for &(provider, _, model) in ROUTES {
        let text = studio_video::body(&spec(provider, model, vec![]), &[]).unwrap();
        let refs = spec(provider, model, vec![frame("image"), video.clone()]);
        let result = studio_video::body(&refs, &[URL.into(), URL.into()]);
        match provider {
            "wavespeed" => {
                assert!(text.0.ends_with("/text-to-video"));
                let (path, body) = result.unwrap();
                assert!(path.ends_with("/reference-to-video"));
                assert_eq!(body["images"], json!([URL]));
                assert_eq!(body["videos"], json!([URL]));
            }
            "kie" => {
                assert_eq!(text.1["model"], "magi-2/text-to-video");
                let (_, body) = result.unwrap();
                assert_eq!(body["model"], "magi-2/reference-to-video");
                assert_eq!(body["input"]["reference_video_urls"], json!([URL]));
            }
            "nanoapi" => {
                assert_eq!(text.1["mode"], "text_to_video");
                let (_, body) = result.unwrap();
                assert_eq!(body["mode"], "reference_to_video");
                assert_eq!(body["video_urls"], json!([URL]));
            }
            "revana" => {
                assert_eq!(result.unwrap_err().0, "reference_kind_not_supported");
                let (_, body) =
                    studio_video::body(&spec(provider, model, vec![frame("image")]), &[URL.into()])
                        .unwrap();
                assert_eq!(body["input"]["image_urls"], json!([URL]));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn nanoapi_key_and_balance_checks_use_the_read_only_balance_endpoint() {
    struct Balance;
    impl Http for Balance {
        fn bytes(&self, request: Request) -> Result<Vec<u8>> {
            assert_eq!(request.url, "https://api.nanoapi.ai/v1/balance");
            assert_eq!(request.method, "GET");
            assert!(request.body.is_none());
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                        && v == "Bearer fixture-key")
            );
            Ok(br#"{"code":200,"data":{"object":"balance","currency":"USD","available":"12.50","frozen":"0"}}"#.to_vec())
        }
    }
    assert_eq!(
        everyframe_miner::key_check::check(&Balance, "nanoapi", "fixture-key").1,
        "key_accepted"
    );
    let rows =
        everyframe_miner::balances::collect(&Balance, &json!({"NANOAPI_API_KEY":"fixture-key"}));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["remainingUsd"], 12.50);
}
