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
                    let urls = if p["requiresVisual"] == true {
                        s["input"]["references"] = json!([asset("first")]);
                        vec!["first".into()]
                    } else {
                        vec![]
                    };
                    models::check(&s).unwrap();
                    studio_video::body(&s, &urls).unwrap();
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

#[test]
fn kling4_maps_text_frames_and_references_without_other_models_fields() {
    let mut s = spec("fal", "kling/v4/text-to-video");
    s["input"]["duration"] = json!(3);
    let (path, b) = studio_video::body(&s, &[]).unwrap();
    assert_eq!(path, "/kling/v4/text-to-video");
    assert_eq!(
        b,
        json!({"prompt":"A cinematic product shot","duration":3,"resolution":"720p","aspect_ratio":"9:16"})
    );
    s["input"]["references"] = json!([asset("first"), asset("last")]);
    let (path, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/kling/v4/image-to-video");
    assert_eq!(b["start_image_url"], "first");
    assert_eq!(b["end_image_url"], "last");
    assert!(b.get("aspect_ratio").is_none());
    let mut video = asset("video");
    video["mime"] = json!("video/mp4");
    video["durationMs"] = json!(30000);
    s["input"]["references"] = json!([asset("first"), asset("image"), video]);
    let (path, b) =
        studio_video::body(&s, &["first".into(), "image".into(), "video".into()]).unwrap();
    assert_eq!(path, "/kling/v4/reference-to-video");
    assert_eq!(b["start_image_url"], "first");
    assert_eq!(b["image_urls"], json!(["image"]));
    assert_eq!(b["video_urls"], json!(["video"]));
    assert_eq!(b["aspect_ratio"], "9:16");
    for field in [
        "seed",
        "image_url",
        "reference_image_urls",
        "generate_audio",
        "prompt_expansion_mode",
    ] {
        assert!(b.get(field).is_none());
    }
    assert_eq!(
        models::info(s["model"].as_str().unwrap()).unwrap()["deadlineMs"],
        1200000
    );
}

#[test]
fn kling4_rejects_bad_references_before_paid_submission() {
    let base = spec("fal", "kling/v4/text-to-video");
    for patch in [
        json!({"width":299}),
        json!({"height":299}),
        json!({"width":4000,"height":720}),
    ] {
        let mut s = base.clone();
        let mut a = asset("image");
        for (k, v) in patch.as_object().unwrap() {
            a[k] = v.clone();
        }
        s["input"]["references"] = json!([a]);
        assert!(studio_video::check(&s).is_err());
    }
    let mut s = base.clone();
    s["input"]["references"] = json!([asset("first"), asset("last"), asset("image")]);
    assert_eq!(
        studio_video::check(&s).unwrap_err().0,
        "kling_end_frame_with_references"
    );
    let mut refs = vec![asset("image"); 10];
    refs.push(asset("first"));
    s["input"]["references"] = json!(refs);
    assert_eq!(
        studio_video::check(&s).unwrap_err().0,
        "kling_image_reference_limit"
    );
    for duration in [2999, 30001] {
        let mut a = asset("video");
        a["mime"] = json!("video/mp4");
        a["durationMs"] = json!(duration);
        s["input"]["references"] = json!([a]);
        assert!(studio_video::check(&s).is_err());
    }
    let mut refs = vec![asset("image"); 10];
    for _ in 0..5 {
        let mut a = asset("video");
        a["mime"] = json!("video/mp4");
        a["durationMs"] = json!(6000);
        refs.push(a);
    }
    s["input"]["references"] = json!(refs);
    studio_video::check(&s).unwrap();
    s["input"]["references"][10]["durationMs"] = json!(6001);
    assert_eq!(
        studio_video::check(&s).unwrap_err().0,
        "reference_duration_exceeded"
    );
}

#[test]
fn kling4_uses_fal_queue_and_downloads_without_credentials() {
    struct Fal {
        calls: RefCell<Vec<Request>>,
    }
    impl Http for Fal {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            everyframe_miner::network::safe_url(&r.url, &r.hosts)?;
            let response = match (r.method.as_str(), r.url.as_str()) {
                ("POST", "https://queue.fal.run/kling/v4/text-to-video") => {
                    let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
                    assert_eq!(b["duration"], 5);
                    assert!(b.get("seed").is_none());
                    json!({"request_id":"kling4-fixture"})
                }
                ("GET", "https://queue.fal.run/kling/v4/requests/kling4-fixture/status") => {
                    json!({"status":"COMPLETED"})
                }
                ("GET", "https://queue.fal.run/kling/v4/requests/kling4-fixture") => {
                    json!({"video":{"url":"https://v3b.fal.media/files/kling4.mp4"}})
                }
                ("GET", "https://v3b.fal.media/files/kling4.mp4") => {
                    assert!(r.headers.is_empty());
                    self.calls.borrow_mut().push(r);
                    return Ok(b"fixture-video".to_vec());
                }
                _ => panic!("Unexpected request: {} {}", r.method, r.url),
            };
            assert!(
                r.headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            );
            self.calls.borrow_mut().push(r);
            Ok(serde_json::to_vec(&response).unwrap())
        }
    }
    let http = Fal {
        calls: RefCell::new(vec![]),
    };
    let mut router = Router::new(&http, json!({"FAL_KEY":"fixture-key"}));
    let s = spec("fal", "kling/v4/text-to-video");
    let id = router.submit(&s).unwrap();
    assert_eq!(router.poll(&id, &s).unwrap(), "COMPLETED");
    assert_eq!(router.result(&id, &s).unwrap().media, b"fixture-video");
    assert_eq!(http.calls.borrow().len(), 4);
}

#[test]
fn heygen_omni_vidu_match_their_own_schemas() {
    let mut h = spec("fal", "heygen/heygen-video/text-to-video");
    let (_, b) = studio_video::body(&h, &[]).unwrap();
    assert_eq!(b["prompt_enhancement"], "turbo");
    assert_eq!(b["enable_safety_checker"], true);
    h["input"]["references"] = json!([asset("first")]);
    h["input"]["promptExpansion"] = json!("disabled");
    let (path, b) = studio_video::body(&h, &["first".into()]).unwrap();
    assert_eq!(path, "/heygen/heygen-video/image-to-video");
    assert!(b.get("aspect_ratio").is_none());
    assert_eq!(b["prompt_enhancement"], "disabled");
    h["input"]["references"] = json!([asset("last"), asset("first")]);
    assert!(studio_video::check(&h).is_err());

    let mut g = spec("fal", "google/gemini-omni-flash/v1.1/text-to-video");
    g["input"]["references"] = json!([asset("first"), asset("last")]);
    let (path, b) = studio_video::body(&g, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/google/gemini-omni-flash/v1.1/image-to-video");
    assert_eq!(b["end_image_url"], "last");
    assert_eq!(b["aspect_ratio"], "9:16");
    assert!(b.get("seed").is_none());
    let mut video = asset("video");
    video["mime"] = json!("video/mp4");
    video["durationMs"] = json!(10000);
    g["input"]["references"] = json!([asset("image"), video.clone(), video.clone(), video.clone()]);
    let (path, b) =
        studio_video::body(&g, &["image".into(), "v1".into(), "v2".into(), "v3".into()]).unwrap();
    assert_eq!(path, "/google/gemini-omni-flash/v1.1/reference-to-video");
    assert_eq!(b["image_urls"], json!(["image"]));
    assert_eq!(b["reference_video_urls"].as_array().unwrap().len(), 3);
    g["input"]["references"][1]["durationMs"] = json!(10001);
    assert!(studio_video::check(&g).is_err());

    let mut v = spec("fal", "fal-ai/vidu/q4/image-to-video");
    assert_eq!(
        studio_video::check(&v).unwrap_err().0,
        "visual_reference_required"
    );
    v["input"]["references"] = json!([asset("first")]);
    let (path, b) = studio_video::body(&v, &["first".into()]).unwrap();
    assert_eq!(path, "/fal-ai/vidu/q4/image-to-video");
    assert!(b.get("aspect_ratio").is_none());
    assert!(b.get("audio").is_none());
    let mut audio = asset("audio");
    audio["mime"] = json!("audio/mpeg");
    audio["durationMs"] = json!(12000);
    v["input"]["references"] = json!([asset("image"), audio.clone(), audio.clone(), audio]);
    let (path, b) =
        studio_video::body(&v, &["image".into(), "a1".into(), "a2".into(), "a3".into()]).unwrap();
    assert_eq!(path, "/fal-ai/vidu/q4/reference-to-video");
    assert_eq!(b["audio"], true);
    assert_eq!(b["reference_image_urls"], json!(["image"]));
    for (key, value) in [("durationMs", json!(2999)), ("mime", json!("audio/wav"))] {
        let mut bad = v.clone();
        bad["input"]["references"][1][key] = value;
        assert!(studio_video::check(&bad).is_err());
    }
}
