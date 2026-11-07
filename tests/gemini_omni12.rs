use everyframe_miner::{google, models, protocol, studio_video};
use serde_json::{Value, json};
fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/gemini-omni12.json")).unwrap()
}
#[test]
fn omni12_specs_and_google_bodies_match_coordinator_vectors() {
    for row in fixtures() {
        let spec = models::spec(&row["request"]).unwrap();
        assert_eq!(spec, row["spec"]);
        assert_eq!(protocol::digest(&spec).unwrap(), row["hash"]);
        models::check(&spec).unwrap();
        if row.get("body").is_some() {
            assert_eq!(
                google::body_with_references(
                    &spec,
                    models::info(spec["model"].as_str().unwrap()).unwrap(),
                    &serde_json::from_value::<Vec<Value>>(row["uploaded"].clone()).unwrap()
                )
                .unwrap(),
                row["body"]
            );
        }
    }
}
#[test]
fn fal12_selects_text_image_and_reference_endpoints_without_seed_passthrough() {
    for row in fixtures().into_iter().filter(|r| {
        r["spec"]["model"]
            .as_str()
            .unwrap()
            .starts_with("studio/fal/")
    }) {
        let spec = row["spec"].clone();
        let refs = spec["input"]["references"].as_array().unwrap();
        let urls: Vec<String> = refs
            .iter()
            .map(|r| r["url"].as_str().unwrap().to_string())
            .collect();
        let (path, body) = studio_video::body(&spec, &urls).unwrap();
        let mode = if refs.is_empty() {
            "text-to-video"
        } else if refs[0]["role"] == "first" {
            "image-to-video"
        } else {
            "reference-to-video"
        };
        assert_eq!(path, format!("/google/gemini-omni-flash/v1.2/{mode}"));
        assert!(body.get("seed").is_none());
        assert_eq!(body["duration"], spec["input"]["duration"]);
        if mode == "reference-to-video" {
            assert_eq!(body["image_urls"], json!([urls[0]]));
            assert_eq!(body["reference_video_urls"], json!([urls[1]]));
        }
    }
}
#[test]
fn omni12_rejects_provider_invalid_requests_before_submission() {
    let mut s = fixtures()[0]["spec"].clone();
    s["input"]["response_format"]["duration"] = json!("3.5s");
    assert!(models::check(&s).is_err());
    s["input"]["response_format"]["duration"] = json!("15s");
    s["input"]["generation_config"]["temperature"] = json!("0.5");
    assert!(models::check(&s).is_err());
    for row in fixtures().into_iter().filter(|r| {
        r["request"]["references"]
            .as_array()
            .is_some_and(|a| a.len() == 2 && a[0]["role"] == "first")
    }) {
        let mut s = row["spec"].clone();
        s["input"]["duration"] = json!(15);
        assert_eq!(
            models::check(&s).unwrap_err().0,
            "invalid_omni_frame_duration"
        );
    }
}
#[test]
fn openrouter12_preserves_seed_audio_and_keyframe_order() {
    let mut s = fixtures()
        .into_iter()
        .find(|r| {
            r["spec"]["model"]
                .as_str()
                .unwrap()
                .starts_with("studio/openrouter/")
        })
        .unwrap()["spec"]
        .clone();
    let source = fixtures()
        .into_iter()
        .find(|r| {
            r["request"]["references"]
                .as_array()
                .is_some_and(|a| a.len() == 2 && a[0]["role"] == "first")
        })
        .unwrap();
    s["input"]["references"] = source["spec"]["input"]["references"].clone();
    let (path, body) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert_eq!(path, "/api/v1/videos");
    assert_eq!(body["model"], "google/gemini-omni-1.2-flash");
    assert_eq!(body["seed"], 42);
    assert_eq!(body["generate_audio"], true);
    assert_eq!(body["resolution"], "4K");
    assert_eq!(body["frame_images"][0]["frame_type"], "first_frame");
    assert_eq!(body["frame_images"][1]["frame_type"], "last_frame");
}

#[test]
fn google12_resumes_uri_result_after_restart_without_a_second_submit() {
    use everyframe_miner::{
        Result,
        network::{Http, Request},
        providers::{Provider, Router},
    };
    use std::cell::RefCell;
    struct Api {
        posts: RefCell<u32>,
        spec: Value,
    }
    impl Http for Api {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "x-goog-api-key" && v == "fixture-google-key")
            );
            if r.method == "POST" {
                assert_eq!(
                    r.url,
                    "https://generativelanguage.googleapis.com/v1beta/interactions"
                );
                *self.posts.borrow_mut() += 1;
                assert_eq!(
                    serde_json::from_slice::<Value>(r.body.as_ref().unwrap()).unwrap()["model"],
                    "gemini-omni-1.2-flash"
                );
                return Ok(json!({"id":"v1_omni12","model":"gemini-omni-1.2-flash","status":"in_progress"}).to_string().into_bytes());
            }
            if r.url.ends_with("/files/result12:download?alt=media") {
                return Ok(b"fixture-video".to_vec());
            }
            assert!(r.url.ends_with("/interactions/v1_omni12"));
            Ok(json!({"id":"v1_omni12","model":"gemini-omni-1.2-flash","status":"completed","steps":[{"type":"model_output","content":[{"type":"video","mime_type":"video/mp4","uri":"https://generativelanguage.googleapis.com/v1beta/files/result12"}]}]}).to_string().into_bytes())
        }
    }
    let spec = fixtures()[2]["spec"].clone();
    let api = Api {
        posts: RefCell::new(0),
        spec: spec.clone(),
    };
    let mut router = Router::new(&api, json!({"GEMINI_API_KEY":"fixture-google-key"}));
    let id = router
        .submit_before(&spec, everyframe_miner::now() + 60000)
        .unwrap();
    drop(router);
    let mut resumed = Router::new(&api, json!({"GEMINI_API_KEY":"fixture-google-key"}));
    assert_eq!(resumed.poll(&id, &api.spec).unwrap(), "COMPLETED");
    assert_eq!(
        resumed.result(&id, &api.spec).unwrap().media,
        b"fixture-video"
    );
    let mut changed = api.spec.clone();
    changed["input"]["prompt"] = json!("another job");
    assert!(resumed.poll(&id, &changed).is_err());
    assert_eq!(*api.posts.borrow(), 1);
}
