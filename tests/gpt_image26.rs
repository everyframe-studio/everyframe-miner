use everyframe_miner::{
    Error, Result, models, native_media,
    network::{Http, Request},
    protocol::{b64, digest, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
fn request(model: &str, p: Value, refs: Vec<Value>) -> Value {
    json!({"model":model,"prompt":"A field of flowers","parameters":p,"references":refs})
}
fn asset(role: &str) -> Value {
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_fixture?t={}","a".repeat(32)),"sha256":sha(b"\x89PNG\r\n\x1a\nfixture"),"bytes":15,"mime":"image/png","role":role,"width":1024,"height":1024,"durationMs":null,"frameCount":null})
}
fn direct(v: &str) -> String {
    format!("direct/openai/gpt-image-2.6-{v}")
}
#[test]
fn exact_model_ids_generation_edits_and_all_reviewed_controls() {
    for v in ["sunburst", "flare"] {
        let model = direct(v);
        for q in ["auto", "low", "medium", "high", "xhigh", "max"] {
            for complexity in ["low", "medium", "high"] {
                let s = models::spec(&request(
                    &model,
                    json!({"quality":q,"complexity":complexity,"size":"3840x2160","n":10}),
                    vec![],
                ))
                .unwrap();
                let (path, b) = native_media::body(&s, &[]).unwrap();
                assert_eq!(path, "/v1/images/generations");
                assert_eq!(b["model"], format!("gpt-image-2.6-{v}"));
                assert_eq!(b["quality"], q);
                assert_eq!(b["complexity"], complexity);
            }
        }
        let s = models::spec(&request(
            &model,
            json!({"output_format":"webp","background":"transparent","output_compression":0}),
            vec![asset("image"), asset("markup")],
        ))
        .unwrap();
        let (path, b) = native_media::body(&s, &["first".into(), "overlay".into()]).unwrap();
        assert_eq!(path, "/v1/images/edits");
        assert_eq!(b["markup"]["image_url"], "overlay");
        assert_eq!(b["images"][0]["image_url"], "first");
        for p in [
            json!({"output_format":"svg","n":2}),
            json!({"quality":"ultra"}),
            json!({"size":"4096x4096"}),
            json!({"complexity":"max"}),
            json!({"frame_count":3}),
            json!({"output_format":"gif","output_compression":20}),
            json!({"output_format":"gif","frame_count":17}),
            json!({"model":"other"}),
            json!({"endpoint":"https://evil.invalid"}),
        ] {
            assert!(models::spec(&request(&model, p, vec![])).is_err());
        }
        assert!(
            models::spec(&request(
                &model,
                json!({"output_format":"svg"}),
                vec![asset("image")]
            ))
            .is_err()
        );
    }
}
#[test]
fn provider_specific_fields_are_translated_and_unsupported_fallback_options_rejected() {
    for v in ["sunburst", "flare"] {
        let fal = format!("studio/native/fal/openai/gpt-image-2.6/{v}");
        let s = models::spec(&request(
            &fal,
            json!({"quality":"max","complexity":"low","moderation":"low"}),
            vec![asset("image"), asset("mask")],
        ))
        .unwrap();
        let (path, b) = native_media::body(&s, &["image".into(), "mask".into()]).unwrap();
        assert_eq!(path, format!("/openai/gpt-image-2.6/{v}/edit"));
        assert_eq!(b["mask_url"], "mask");
        assert_eq!(b["reasoning_effort"], "low");
        assert_eq!(b["enable_safety_checker"], false);
        assert!(b.get("complexity").is_none());
        let or = format!("studio/native/openrouter/openai/gpt-image-2.6-{v}");
        let s = models::spec(&request(
            &or,
            json!({"aspect_ratio":"9:16","complexity":"medium"}),
            vec![asset("image")],
        ))
        .unwrap();
        let (path, b) = native_media::body(&s, &["image".into()]).unwrap();
        assert_eq!(path, "/api/v1/images");
        assert_eq!(b["provider"]["options"]["openai"]["complexity"], "medium");
        assert_eq!(b["provider"]["allow_fallbacks"], false);
        assert_eq!(b["input_references"][0]["image_url"]["url"], "image");
        for p in [
            json!({"size":"1024x1024"}),
            json!({"output_format":"svg"}),
            json!({"output_format":"gif","frame_count":5}),
        ] {
            assert!(models::spec(&request(&or, p, vec![])).is_err());
        }
    }
}
struct Api {
    calls: RefCell<Vec<Request>>,
    format: &'static str,
    fail: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        everyframe_miner::network::safe_url(&r.url, &r.hosts)?;
        self.calls.borrow_mut().push(r.clone());
        if r.url.contains("/attachment/") {
            assert!(r.headers.is_empty());
            return Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec());
        }
        if r.url.contains("fal.media") {
            assert!(r.headers.is_empty());
            return Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec());
        }
        if self.fail {
            return Err(Error("http_502"));
        }
        let result = if r.url.contains("queue.fal.run") {
            if r.method == "POST" {
                json!({"request_id":"fixture"})
            } else if r.url.ends_with("/status") {
                json!({"request_id":"fixture","status":"COMPLETED"})
            } else {
                json!({"images":[{"url":"https://v3.fal.media/image.png","content_type":"image/png"}]})
            }
        } else {
            let data = match self.format {
                "gif" => b"GIF89afixture".to_vec(),
                "svg" => b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
                _ => b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            };
            json!({"data":[{"b64_json":b64(&data)}]})
        };
        Ok(result.to_string().into_bytes())
    }
}
#[test]
fn sync_formats_are_bundled_bound_to_input_and_never_resubmitted_on_restart() {
    for format in ["png", "gif", "svg"] {
        let s = models::spec(&request(
            &direct("sunburst"),
            json!({"output_format":format}),
            vec![],
        ))
        .unwrap();
        let api = Api {
            calls: RefCell::default(),
            format,
            fail: false,
        };
        let creds = json!({"OPENAI_API_KEY":"private-fixture-key"});
        let mut r = Router::new(&api, creds.clone());
        let id = r.submit(&s).unwrap();
        assert_eq!(r.poll(&id, &s).unwrap(), "COMPLETED");
        let out = r.result(&id, &s).unwrap();
        let bundle: Value = serde_json::from_slice(&out.media).unwrap();
        assert_eq!(
            bundle["parts"][0]["mime"],
            if format == "svg" {
                "image/svg+xml".into()
            } else {
                format!("image/{format}")
            }
        );
        let mut restarted = Router::new(&api, creds);
        assert!(restarted.poll(&id, &s).is_err());
        assert!(restarted.result(&id, &s).is_err());
        assert_eq!(api.calls.borrow().len(), 1);
    }
}
#[test]
fn fal_resumes_by_get_after_restart_and_never_leaks_credentials_to_media() {
    let s = models::spec(&request(
        "studio/native/fal/openai/gpt-image-2.6/flare",
        json!({}),
        vec![],
    ))
    .unwrap();
    let api = Api {
        calls: RefCell::default(),
        format: "png",
        fail: false,
    };
    let creds = json!({"FAL_KEY":"private-fixture-key"});
    let mut r = Router::new(&api, creds.clone());
    let id = r.submit(&s).unwrap();
    drop(r);
    let mut resumed = Router::new(&api, creds);
    assert_eq!(resumed.poll(&id, &s).unwrap(), "COMPLETED");
    resumed.result(&id, &s).unwrap();
    assert_eq!(
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
    assert!(
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.url.contains("/requests/"))
            .all(|r| r
                .url
                .contains("/openai/gpt-image-2.6/flare/text-to-image/requests/fixture"))
    );
}
#[test]
fn uncertain_synchronous_submission_has_no_internal_retry() {
    let s = models::spec(&request(&direct("flare"), json!({}), vec![])).unwrap();
    let api = Api {
        calls: RefCell::default(),
        format: "png",
        fail: true,
    };
    let mut r = Router::new(&api, json!({"OPENAI_API_KEY":"private-fixture-key"}));
    assert!(r.submit(&s).is_err());
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn signed_specs_match_coordinator_vectors() {
    let vectors: Value = serde_json::from_str(include_str!("fixtures/gpt-image-26.json")).unwrap();
    for v in vectors.as_array().unwrap() {
        let spec = models::spec(&v["input"]);
        if v["valid"] == true {
            let spec = spec.unwrap();
            assert_eq!(spec, v["spec"]);
            assert_eq!(digest(&spec).unwrap(), v["hash"]);
        } else {
            assert!(spec.is_err(), "{}", v["input"]);
        }
    }
}
