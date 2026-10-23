use everyframe_miner::{
    Error, Result, models,
    network::{Http, Request},
    protocol::digest,
    providers::{Provider, Router},
    studio_media,
};
use serde_json::{Value, json};
use std::cell::RefCell;
fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/studio-media.json")).unwrap()
}
#[test]
fn image_contracts_match_app_body_and_coordinator_hash_across_all_models_and_aspects() {
    let rows = fixtures();
    assert_eq!(rows.len(), 70);
    for r in rows {
        let spec = &r["spec"];
        models::check(spec).unwrap();
        assert_eq!(digest(spec).unwrap(), r["hash"]);
        let urls = r["urls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let (path, body) = studio_media::body(spec, &urls).unwrap();
        assert_eq!(path, r["path"]);
        assert_eq!(body, r["body"]);
    }
}
fn media_spec(model: &str, parameters: Value) -> Value {
    let p = &studio_media::POLICIES[model];
    let prompt = parameters
        .get("text")
        .or_else(|| parameters.get("prompt"))
        .cloned()
        .unwrap_or(json!("A cinematic clip"));
    let audio = p["kind"] == "audio";
    json!({"model":model,"input":{"prompt":prompt,"seed":42,"duration":parameters.get("duration").or_else(||parameters.get("seconds_total")).or_else(||parameters.get("duration_seconds")).cloned().unwrap_or(json!(5)),"resolution":if audio{"audio"}else{"480p"},"aspect":if audio{"none"}else{"16:9"},"references":[],"promptExpansion":"balanced","parameters":parameters}})
}
#[test]
fn audio_native_options_are_preserved() {
    for (model, params) in [
        (
            "fal-ai/elevenlabs/tts/eleven-v4",
            json!({"text":"Hello studio.","voice":"Rachel","language_code":"en","output_format":"mp3_44100_128"}),
        ),
        (
            "minimax/music-3",
            json!({"prompt":"A gentle orchestral film score","lyrics":"[instrumental]","duration":120}),
        ),
        (
            "fal-ai/stable-audio-25/text-to-audio",
            json!({"prompt":"An orchestral score","seconds_total":180,"num_inference_steps":8}),
        ),
        (
            "fal-ai/elevenlabs/sound-effects/v2",
            json!({"text":"A whoosh","duration_seconds":5,"loop":true,"output_format":"mp3_44100_128"}),
        ),
    ] {
        let spec = media_spec(&format!("studio/media/fal/{model}"), params.clone());
        let (path, body) = studio_media::body(&spec, &[]).unwrap();
        assert_eq!(path, format!("/{model}"));
        assert_eq!(body, params);
        let mut bad = spec.clone();
        bad["input"]["parameters"]["url"] = json!("https://private.invalid");
        assert!(models::check(&bad).is_err());
    }
}
#[test]
fn h3_references_bind_frames_and_guided_images_use_the_native_start_field() {
    let mut spec = media_spec("studio/media/fal/minimax/h3-max/text-to-video", json!({}));
    let mut a = fixtures()[1]["spec"]["input"]["references"][0].clone();
    a["role"] = json!("video");
    a["mime"] = json!("video/mp4");
    a["durationMs"] = json!(5000);
    a["frameCount"] = json!(120);
    let mut first = fixtures()[1]["spec"]["input"]["references"][1].clone();
    first["role"] = json!("first");
    spec["input"]["references"] = json!([first, a]);
    let (path, body) = studio_media::body(&spec, &["first".into(), "clip".into()]).unwrap();
    assert_eq!(path, "/minimax/h3-max/reference-to-video");
    assert_eq!(body["image_url"], "first");
    assert!(body.get("start_image_url").is_none());
    assert_eq!(body["reference_video_urls"], json!(["clip"]));
    assert!(body.get("frameCount").is_none());
    let old = digest(&spec).unwrap();
    spec["input"]["references"][1]["frameCount"] = json!(240);
    assert_ne!(digest(&spec).unwrap(), old);
}
#[test]
fn layout_edit_keeps_mask_separate_and_preserves_reference_order() {
    let mut spec = fixtures()[1]["spec"].clone();
    spec["input"]["parameters"]["editPrecision"] = json!("high");
    spec["input"]["parameters"]["maskIndex"] = json!(1);
    let (path, body) = studio_media::body(&spec, &["cast".into(), "mask".into()]).unwrap();
    assert_eq!(path, "/ideogram/v4.5/edit");
    assert_eq!(body["image_url"], "cast");
    assert_eq!(body["mask_url"], "mask");
    assert_eq!(body["image_size"], "auto");
    assert!(body.get("reference_image_urls").is_none());
    spec["input"]["parameters"]["maskIndex"] = json!(0);
    assert!(models::check(&spec).is_err());
}
struct Api {
    calls: RefCell<Vec<Request>>,
    kind: &'static str,
    bad: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = r.url.clone();
        self.calls.borrow_mut().push(r.clone());
        if u.contains("/attachment/") {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            return Ok(if self.bad {
                b"bad fixture".to_vec()
            } else {
                b"fixture image bytes".to_vec()
            });
        }
        if u.contains(".fal.media/") {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            return Ok(b"output bytes".to_vec());
        }
        let v = if r.method == "POST" {
            if self.kind == "sync" {
                json!({"data":[{"b64_json":"b3V0cHV0IGJ5dGVz"}]})
            } else {
                json!({"request_id":"fixture-job"})
            }
        } else if u.ends_with("/status") {
            json!({"status":"COMPLETED"})
        } else if self.kind == "image" {
            json!({"images":[{"url":"https://v3.fal.media/image.jpeg"}]})
        } else {
            json!({"audio":"https://v3.fal.media/audio.wav"})
        };
        Ok(v.to_string().into_bytes())
    }
}
#[test]
fn fal_images_and_audio_download_without_credentials() {
    for kind in ["image", "audio"] {
        let spec = if kind == "image" {
            fixtures()[0]["spec"].clone()
        } else {
            media_spec(
                "studio/media/fal/minimax/music-3",
                json!({"prompt":"A gentle score","lyrics":"[instrumental]","duration":30}),
            )
        };
        let api = Api {
            calls: RefCell::default(),
            kind,
            bad: false,
        };
        let mut router = Router::new(&api, json!({"FAL_KEY":"private-test-key"}));
        let id = router.submit(&spec).unwrap();
        assert_eq!(router.poll(&id, &spec).unwrap(), "COMPLETED");
        assert_eq!(router.result(&id, &spec).unwrap().media, b"output bytes");
    }
}
#[test]
fn hash_mismatch_prevents_all_paid_submissions() {
    let api = Api {
        calls: RefCell::default(),
        kind: "image",
        bad: true,
    };
    let mut router = Router::new(&api, json!({"FAL_KEY":"private-test-key"}));
    assert_eq!(
        router.submit(&fixtures()[1]["spec"]),
        Err(Error("reference_integrity_mismatch"))
    );
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn synchronous_image_recovery_never_reposts_after_restart() {
    let spec = fixtures()[40]["spec"].clone();
    assert_eq!(
        spec["model"],
        "studio/media/openrouter/openai/gpt-image-2.5-sunburst"
    );
    let api = Api {
        calls: RefCell::default(),
        kind: "sync",
        bad: false,
    };
    let mut router = Router::new(&api, json!({"OPENROUTER_API_KEY":"private-test-key"}));
    let id = router.submit(&spec).unwrap();
    assert_eq!(router.poll(&id, &spec).unwrap(), "COMPLETED");
    assert_eq!(router.result(&id, &spec).unwrap().media, b"output bytes");
    let mut restarted = Router::new(&api, json!({"OPENROUTER_API_KEY":"private-test-key"}));
    assert!(restarted.poll(&id, &spec).is_err());
    assert!(restarted.result(&id, &spec).is_err());
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn no_unverified_frame_count_can_reach_provider() {
    let mut spec = media_spec("studio/media/fal/minimax/h3-max/text-to-video", json!({}));
    let mut a = fixtures()[1]["spec"]["input"]["references"][0].clone();
    a["role"] = json!("video");
    a["mime"] = json!("video/mp4");
    a["durationMs"] = json!(5000);
    a["frameCount"] = json!(0);
    spec["input"]["references"] = json!([a]);
    let api = Api {
        calls: RefCell::default(),
        kind: "image",
        bad: false,
    };
    let mut router = Router::new(&api, json!({"FAL_KEY":"private-test-key"}));
    assert_eq!(
        router.submit(&spec),
        Err(Error("reference_verification_required"))
    );
    assert!(api.calls.borrow().is_empty());
}

#[test]
fn ideogram_vector_preserves_svg_contract_and_native_inputs() {
    let model = "studio/media/fal/ideogram/vector/text-to-vector";
    let mut spec = json!({"model":model,"input":{"prompt":"A logo","seed":7,"duration":1,"resolution":"1k","aspect":"1:1","references":[],"promptExpansion":"balanced","parameters":{"negative":"","quality":"high","editPrecision":"none","maskIndex":-1}}});
    models::check(&spec).unwrap();
    let (path, body) = studio_media::body(&spec, &[]).unwrap();
    assert_eq!(path, "ideogram/vector/text-to-vector");
    assert_eq!(
        body,
        json!({"prompt":"A logo","seed":7,"num_images":1,"aspect_ratio":"1:1","quality":"high"})
    );
    assert_eq!(models::info(model).unwrap()["mime"], "image/svg+xml");
    spec["input"]["aspect"] = json!("21:9");
    assert!(models::check(&spec).is_err());
    spec["input"]["aspect"] = json!("1:1");
    spec["input"]["prompt"] = json!("x".repeat(9501));
    assert!(models::check(&spec).is_err());
}
