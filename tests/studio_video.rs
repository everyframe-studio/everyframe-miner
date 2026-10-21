use everyframe_miner::{
    Error, Result, models,
    network::{Http, Request},
    protocol::sha,
    providers::{Provider, Router},
    studio_video,
};
use serde_json::{Value, json};
use std::cell::RefCell;
fn spec(provider: &str, model: &str) -> Value {
    let id = format!("studio/{provider}/{model}");
    let policy = &studio_video::POLICIES[&id];
    json!({"model":id,"input":{"prompt":"A cinematic product shot","seed":42,"duration":5,"resolution":policy["resolutions"][0],"aspect":"9:16","references":[],"promptExpansion":"balanced"}})
}
fn asset(role: &str) -> Value {
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_{role}?t={}","a".repeat(32)),"sha256":sha(b"fixture image bytes"),"bytes":19,"mime":"image/jpeg","role":role,"width":720,"height":1280,"durationMs":0})
}
#[test]
fn flux3_max_body_and_reference_contract() {
    let mut s = spec("fal", "blackforestlabs/flux-3-max/text-to-video");
    let (path, b) = studio_video::body(&s, &[]).unwrap();
    assert_eq!(path, "/blackforestlabs/flux-3-max/text-to-video");
    assert_eq!(
        b,
        json!({"prompt":"A cinematic product shot","seed":42,"duration":5,"aspect_ratio":"9:16","resolution":"720p","generate_audio":true,"reasoning_level":"low"})
    );
    s["input"]["duration"] = json!(20);
    s["input"]["resolution"] = json!("1080p");
    s["input"]["references"] = json!([asset("first"), asset("last")]);
    let (path, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/blackforestlabs/flux-3-max/image-to-video");
    assert_eq!(b["image_url"], "first");
    assert_eq!(b["end_image_url"], "last");
    assert_eq!(b["aspect_ratio"], "auto");
    s["input"]["references"] = json!([asset("image")]);
    assert!(studio_video::check(&s).is_err());
    s["input"]["references"] = json!([asset("last")]);
    assert!(studio_video::check(&s).is_err());
    s["input"]["references"] = json!([]);
    for (key, value) in [("duration", json!(30)), ("resolution", json!("480p"))] {
        let mut bad = s.clone();
        bad["input"][key] = value;
        assert!(studio_video::check(&bad).is_err());
    }
}
#[test]
fn slow_reference_download_cannot_submit_after_deadline() {
    struct Slow;
    impl Http for Slow {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.method, "GET", "paid submission must never occur");
            std::thread::sleep(std::time::Duration::from_millis(40));
            Ok(b"fixture image bytes".to_vec())
        }
    }
    let mut s = spec("fal", "minimax/h3-max/text-to-video");
    s["input"]["references"] = json!([asset("image")]);
    let mut router = Router::new(&Slow, json!({"FAL_KEY":"synthetic-fixture-key"}));
    assert_eq!(
        router
            .submit_before(&s, everyframe_miner::now() + 10)
            .unwrap_err()
            .0,
        "submission_deadline_elapsed"
    );
}
#[test]
fn studio_contracts_validate_every_resolution_aspect_and_duration() {
    for (id, p) in studio_video::POLICIES.as_object().unwrap() {
        assert!(models::info(id).is_ok());
        for aspect in p["aspects"].as_array().unwrap() {
            for resolution in p["resolutions"].as_array().unwrap() {
                for duration in [p["min"].as_i64().unwrap(), p["max"].as_i64().unwrap()] {
                    let mut s = spec(
                        p["provider"].as_str().unwrap(),
                        p["apiModel"].as_str().unwrap(),
                    );
                    s["input"]["aspect"] = aspect.clone();
                    s["input"]["resolution"] = resolution.clone();
                    s["input"]["duration"] = json!(duration);
                    models::check(&s).unwrap();
                    studio_video::body(&s, &[]).unwrap();
                }
            }
        }
    }
}
#[test]
fn provider_fields_and_reference_modes_are_preserved() {
    let mut s = spec("fal", "minimax/h3-max/text-to-video");
    s["input"]["references"] = json!([asset("image")]);
    let (path, b) = studio_video::body(&s, &["https://fixture.test/image".into()]).unwrap();
    assert_eq!(path, "/minimax/h3-max/reference-to-video");
    assert_eq!(b["resolution"], "480P");
    assert_eq!(b["aspect_ratio"], "9:16");
    assert_eq!(b["reference_image_urls"][0], "https://fixture.test/image");
    s = spec("fal", "alibaba/wan-3.0/text-to-video");
    s["input"]["references"] = json!([asset("first"), asset("last")]);
    let (path, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/alibaba/wan-3.0/image-to-video");
    assert_eq!(b["start_image_url"], "first");
    assert_eq!(b["end_image_url"], "last");
    assert_eq!(b["audio"], true);
    s = spec("openrouter", "bytedance/seedance-2.5");
    s["input"]["references"] = json!([asset("image")]);
    let (path, b) = studio_video::body(&s, &["image".into()]).unwrap();
    assert_eq!(path, "/api/v1/videos");
    assert_eq!(b["model"], "bytedance/seedance-2.5");
    assert_eq!(b["input_references"][0]["image_url"]["url"], "image");
    assert_eq!(b["resolution"], "480p");
    s = spec("vivix", "vivix-w1");
    s["input"]["references"] = json!([asset("first"), asset("last")]);
    let (_, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(b["jobs"][0]["first_frame_image"], "first");
    assert_eq!(b["jobs"][0]["last_frame_image"], "last");
    assert_eq!(b["seed"], 42);
    assert!(b["jobs"][0].get("resolution").is_none());
}
#[test]
fn rejects_unsupported_fields_modes_and_source_urls() {
    let base = spec("vivix", "vivix-w1");
    for (key, value) in [
        ("resolution", json!("1080p")),
        ("aspect", json!("21:9")),
        ("duration", json!(16)),
        ("seed", json!(-1)),
        ("injected", json!(true)),
    ] {
        let mut s = base.clone();
        s["input"][key] = value;
        assert!(models::check(&s).is_err());
    }
    for url in [
        "http://api.everyframe.studio/v1/attachment/att_a?t=aaaaaaaaaaaaaaaaaaaa",
        "https://127.0.0.1/secret",
        "https://api.everyframe.studio.evil.test/v1/attachment/att_a?t=aaaaaaaaaaaaaaaaaaaa",
        "https://api.everyframe.studio/v1/admin?t=aaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut s = base.clone();
        let mut a = asset("first");
        a["url"] = json!(url);
        s["input"]["references"] = json!([a]);
        assert!(models::check(&s).is_err());
    }
    let mut s = base.clone();
    s["input"]["references"] = json!([asset("first"), asset("image")]);
    assert!(models::check(&s).is_err());
    let mut s = spec("fal", "minimax/h3-max-turbo/text-to-video");
    s["input"]["references"] = json!([asset("image")]);
    assert!(models::check(&s).is_err());
}
struct Api {
    calls: RefCell<Vec<Request>>,
    bad: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let url = r.url.clone();
        if url.contains("/attachment/") {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            assert_eq!(r.limit, 19);
            self.calls.borrow_mut().push(r);
            return Ok(if self.bad {
                b"different bad bytes".to_vec()
            } else {
                b"fixture image bytes".to_vec()
            });
        }
        let reply = if url.contains("/v1/files/") {
            let body = r.body.as_ref().unwrap();
            assert!(body.windows(19).any(|w| w == b"fixture image bytes"));
            json!({"code":0,"data":{"url":"https://static.vivi-x.ai/reference.jpg"}})
        } else {
            assert_eq!(url, "https://api.vivix.ai/v1/videos/jobs");
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert_eq!(
                b["jobs"][0]["ref_images"][0]["url"],
                "https://static.vivi-x.ai/reference.jpg"
            );
            json!({"code":0,"data":{"results":[{"job_id":"job_fixture"}]}})
        };
        self.calls.borrow_mut().push(r);
        Ok(reply.to_string().into_bytes())
    }
}
#[test]
fn reference_hash_is_verified_before_upload_or_paid_submit() {
    for bad in [false, true] {
        let api = Api {
            calls: RefCell::default(),
            bad,
        };
        let mut router = Router::new(&api, json!({"VIVIX_API_KEY":"fixture-key-not-production"}));
        let mut s = spec("vivix", "vivix-w1");
        s["input"]["references"] = json!([asset("image")]);
        let result = router.submit(&s);
        if bad {
            assert_eq!(result, Err(Error("reference_integrity_mismatch")));
            assert_eq!(api.calls.borrow().len(), 1);
        } else {
            assert_eq!(result.unwrap(), "vivix_job_fixture");
            assert_eq!(api.calls.borrow().len(), 3);
        }
    }
}
