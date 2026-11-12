use everyframe_miner::{
    Error, Result, models, native_media,
    network::{Http, Request},
    protocol::{b64, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";
#[test]
fn content_refusals_and_uncertain_submissions_cannot_become_safe_fallbacks() {
    assert_eq!(
        everyframe_miner::h3_image::http_error(422).0,
        "provider_content_rejected"
    );
    assert_eq!(
        everyframe_miner::submission::rejected(everyframe_miner::h3_image::http_error(422).0),
        Some("provider_content_rejected")
    );
    for status in [500, 502, 503, 504] {
        assert_eq!(
            everyframe_miner::submission::rejected(
                everyframe_miner::h3_image::http_error(status).0
            ),
            None
        );
    }
}
#[test]
fn coordinator_and_worker_sign_identical_unicode_requests() {
    let rows: Value =
        serde_json::from_str(include_str!("fixtures/h3-image-20261112.json")).unwrap();
    for row in rows.as_array().unwrap() {
        let s = models::spec(&row["input"]).unwrap();
        assert_eq!(s, row["spec"]);
        assert_eq!(
            everyframe_miner::protocol::digest(&s).unwrap(),
            row["digest"]
        );
    }
}
fn input(model: &str, p: Value, refs: Value) -> Value {
    json!({"model":model,"prompt":"A blue paper crane","parameters":p,"references":refs})
}
fn asset() -> Value {
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_fixture?t={}","a".repeat(32)),"sha256":sha(PNG),"bytes":PNG.len(),"mime":"image/png","role":"image","width":1024,"height":1024,"durationMs":0,"frameCount":0})
}
#[test]
fn native_contracts_cover_exact_4k_custom_sizes_and_all_six_routes() {
    for (id, m) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["h3Image"] == true)
    {
        for size in ["1024x1024", "4096x4096", "5472x3072", "7104x2368"] {
            let spec = models::spec(&input(
                id,
                json!({"size":size,"n":10,"output_format":"png"}),
                json!([]),
            ))
            .unwrap();
            let (path, b) = native_media::body(&spec, &[]).unwrap();
            assert!(path.starts_with('/'));
            assert!(b.get("seed").is_none());
            assert_eq!(
                b[if m["provider"] == "fal" {
                    "num_images"
                } else {
                    "n"
                }],
                10
            );
        }
        let refs = vec![asset(); 16];
        let spec = models::spec(&input(id, json!({"size":"4096x4096"}), json!(refs))).unwrap();
        let (_, b) = native_media::body(&spec, &vec!["owned-image".to_owned(); 16]).unwrap();
        assert_eq!(
            b[if m["provider"] == "fal" {
                "image_urls"
            } else if m["provider"] == "minimax" {
                "images"
            } else {
                "input_references"
            }]
            .as_array()
            .unwrap()
            .len(),
            16
        );
    }
}
#[test]
fn unsupported_controls_and_ambiguous_sizing_are_rejected() {
    for p in [
        json!({"size":"1x1"}),
        json!({"size":"999999999999999999x1"}),
        json!({"size":"4096x4096","area":"4K"}),
        json!({"seed":1}),
        json!({"negative_prompt":"x"}),
        json!({"mask":"@reference:0"}),
        json!({"n":11}),
    ] {
        assert!(models::spec(&input("direct/minimax/h3-image", p, json!([]))).is_err());
    }
    assert!(
        models::spec(&input(
            "direct/minimax/h3-image",
            json!({"disable_prompt_enhanced":true}),
            json!([asset()])
        ))
        .is_err()
    );
    assert!(
        models::spec(&input(
            "direct/minimax/h3-image",
            json!({}),
            json!(vec![asset(); 17])
        ))
        .is_err()
    );
    let spec = models::spec(&input(
        "direct/minimax/h3-image",
        json!({"area":"4K","aspect_ratio":"3:1"}),
        json!([]),
    ))
    .unwrap();
    assert_eq!(spec["input"]["parameters"]["size"], "auto");
    assert!(
        models::spec(&input(
            "studio/native/fal/minimax/h3-image",
            json!({"area":"4K"}),
            json!([])
        ))
        .is_err()
    );
}

#[test]
fn openrouter_preserves_minimax_automatic_4k_default_instead_of_its_own_default() {
    let spec = models::spec(&input(
        "studio/native/openrouter/minimax/h3-image",
        json!({"size":"auto"}),
        json!([]),
    ))
    .unwrap();
    let (_, body) = native_media::body(&spec, &[]).unwrap();
    assert_eq!(body["resolution"], "4K");
    assert_eq!(body["aspect_ratio"], "auto");
    assert!(body.get("size").is_none());
}
struct Api {
    provider: String,
    calls: RefCell<Vec<Request>>,
    partial: bool,
    fail: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let host = url::Url::parse(&r.url)
            .unwrap()
            .host_str()
            .unwrap()
            .to_owned();
        let answer = if host == "api.everyframe.studio"
            || host == "file.cdn.minimax.io"
            || host == "v3b.fal.media"
        {
            assert!(r.headers.is_empty());
            PNG.to_vec()
        } else if r.method == "POST" {
            assert!(r.timeout >= 660);
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "authorization" && v.contains("test-secret"))
            );
            let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert!(body.get("seed").is_none());
            if self.fail {
                self.calls.borrow_mut().push(r);
                return Err(Error("request_timeout"));
            }
            if self.provider == "fal" {
                json!({"request_id":"h3job"}).to_string().into_bytes()
            } else if self.partial {
                json!({"data":[{"error":{"code":"1027","message":"image refused"}}]})
                    .to_string()
                    .into_bytes()
            } else if self.provider == "minimax" {
                assert_eq!(body["response_format"], "url");
                json!({"data":[{"url":"https://file.cdn.minimax.io/h3/result.png"}]})
                    .to_string()
                    .into_bytes()
            } else {
                assert_eq!(body["provider"]["only"], json!(["minimax"]));
                assert_eq!(body["provider"]["allow_fallbacks"], false);
                json!({"data":[{"b64_json":b64(PNG),"media_type":"image/png"}]})
                    .to_string()
                    .into_bytes()
            }
        } else if r.url.ends_with("/status") {
            json!({"request_id":"h3job","status":"COMPLETED"})
                .to_string()
                .into_bytes()
        } else {
            json!({"images":[{"url":"https://v3b.fal.media/h3/result.png","content_type":"image/png"}]}).to_string().into_bytes()
        };
        self.calls.borrow_mut().push(r);
        Ok(answer)
    }
}
#[test]
fn each_provider_submits_once_authenticates_only_api_and_delivers_bound_output() {
    for provider in ["minimax", "openrouter", "fal"] {
        for fast in [false, true] {
            let model = format!(
                "{}{}",
                if provider == "minimax" {
                    "direct/minimax/h3-image"
                } else if provider == "openrouter" {
                    "studio/native/openrouter/minimax/h3-image"
                } else {
                    "studio/native/fal/minimax/h3-image"
                },
                if fast { "-highspeed" } else { "" }
            );
            let spec = models::spec(&input(
                &model,
                json!({"output_format":"png","size":"4096x4096"}),
                json!([asset()]),
            ))
            .unwrap();
            let api = Api {
                provider: provider.into(),
                calls: RefCell::default(),
                partial: false,
                fail: false,
            };
            let keys = json!({"MINIMAX_API_KEY":"test-secret","OPENROUTER_API_KEY":"test-secret","FAL_KEY":"test-secret"});
            let mut router = Router::new(&api, keys.clone());
            let reference = router.submit(&spec).unwrap();
            if provider == "fal" {
                router = Router::new(&api, keys.clone());
            } else {
                assert!(
                    Router::new(&api, keys.clone())
                        .poll(&reference, &spec)
                        .is_err()
                );
            }
            assert_eq!(router.poll(&reference, &spec).unwrap(), "COMPLETED");
            let out = router.result(&reference, &spec).unwrap();
            let bundle: Value = serde_json::from_slice(&out.media).unwrap();
            assert_eq!(bundle["parts"][0]["data"], b64(PNG));
            let changed = models::spec(&input(
                &model,
                json!({"output_format":"png","size":"2048x2048"}),
                json!([asset()]),
            ))
            .unwrap();
            assert!(router.poll(&reference, &changed).is_err());
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
}
#[test]
fn timeout_and_per_image_failure_never_replay_the_submission() {
    let spec = models::spec(&input("direct/minimax/h3-image", json!({}), json!([]))).unwrap();
    for (partial, fail) in [(true, false), (false, true)] {
        let api = Api {
            provider: "minimax".into(),
            calls: RefCell::default(),
            partial,
            fail,
        };
        let mut router = Router::new(&api, json!({"MINIMAX_API_KEY":"test-secret"}));
        assert!(router.submit(&spec).is_err());
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
