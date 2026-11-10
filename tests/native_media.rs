use everyframe_miner::native_media::{self, MODELS};
use serde_json::{Value, json};

fn spec(model: &str, parameters: Value, references: Value) -> Value {
    native_media::spec(&json!({"model":model,"prompt":"A quiet river at sunrise.","parameters":parameters,"references":references})).unwrap()
}
fn asset(role: &str) -> Value {
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_fixture?t={}","a".repeat(32)),
        "sha256":"a".repeat(64),"bytes":123,"mime":"image/png","role":role,
        "width":1024,"height":1024,"durationMs":null,"frameCount":null})
}

#[test]
fn every_native_model_has_a_valid_default_and_locked_provider_endpoint() {
    assert_eq!(
        MODELS
            .as_object()
            .unwrap()
            .values()
            .filter(|m| !m["nativeSchema"].is_object() && m["asianMedia"] != true)
            .count(),
        16
    );
    for (model, contract) in MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| !m["nativeSchema"].is_object() && m["asianMedia"] != true)
    {
        let value = spec(model, json!({}), json!([]));
        let (path, body) = native_media::body(&value, &[]).unwrap();
        assert!(
            path.starts_with("/v1/") || contract["gptImage26"] == true,
            "{model}"
        );
        assert!(body.get("api_key").is_none());
        if contract["adapter"] != "xai-tts" && contract["provider"] != "fal" {
            assert_eq!(body["model"], contract["apiModel"]);
        }
        assert!(native_media::spec(&json!({"model":model,"prompt":"hi","parameters":{"endpoint":"https://evil.example"}})).is_err());
    }
}

#[test]
fn openai_images_cover_custom_dimensions_all_qualities_and_reference_edits() {
    let model = "direct/openai/gpt-image-2.5-sunburst";
    for quality in ["low", "medium", "high", "xhigh", "max", "auto"] {
        let value = spec(
            model,
            json!({"size":"3840x2160","quality":quality,"n":10}),
            json!([]),
        );
        let (_, body) = native_media::body(&value, &[]).unwrap();
        assert_eq!(body["size"], "3840x2160");
        assert_eq!(body["n"], 10);
    }
    for size in ["3841x2160", "16x16", "4096x4096", "1024x8192", "-1x1024"] {
        assert!(
            native_media::spec(&json!({"model":model,"prompt":"test","parameters":{"size":size}}))
                .is_err()
        );
    }
    let refs = json!([asset("image"), asset("mask")]);
    let value = spec(
        model,
        json!({"background":"transparent","output_format":"webp","output_compression":80}),
        refs,
    );
    let (path, body) =
        native_media::body(&value, &["image-data".into(), "mask-data".into()]).unwrap();
    assert_eq!(path, "/v1/images/edits");
    assert_eq!(body["images"][0]["image_url"], "image-data");
    assert_eq!(body["mask"]["image_url"], "mask-data");
    assert!(native_media::spec(&json!({"model":model,"prompt":"test","parameters":{"background":"transparent","output_format":"jpeg"}})).is_err());
}

#[test]
fn xai_images_use_deferred_recovery_and_keep_edit_references() {
    let model = "direct/xai/grok-imagine-image-2.0";
    let value = spec(
        model,
        json!({"quality":"medium","resolution":"2k","n":3}),
        json!([asset("image")]),
    );
    let (path, body) = native_media::body(&value, &["image-data".into()]).unwrap();
    assert_eq!(path, "/v1/images/edits");
    assert_eq!(body["deferred"], true);
    assert_eq!(body["response_format"], "url");
    assert_eq!(body["images"][0]["url"], "image-data");
    assert!(
        native_media::spec(&json!({"model":model,"prompt":"test","parameters":{"quality":"max"}}))
            .is_err()
    );
}

#[test]
fn deferred_results_are_bound_to_the_input_and_checked_before_use() {
    let value = spec("direct/xai/grok-imagine-video-1.5", json!({}), json!([]));
    let reference = native_media::reference(&value, "task_123").unwrap();
    assert_eq!(
        native_media::reference_id(&value, &reference).unwrap(),
        "task_123"
    );
    let different = spec(
        "direct/xai/grok-imagine-video-1.5",
        json!({"duration":8}),
        json!([]),
    );
    assert!(native_media::reference_id(&different, &reference).is_err());
    assert_eq!(
        native_media::state(
            &value,
            "task_123",
            &json!({"request_id":"task_123","status":"done","model":"grok-imagine-video-1.5"})
        )
        .unwrap(),
        "COMPLETED"
    );
    assert!(
        native_media::state(
            &value,
            "task_123",
            &json!({"request_id":"other_task","status":"done"})
        )
        .is_err()
    );
    assert!(
        native_media::state(
            &value,
            "task_123",
            &json!({"status":"done","model":"grok-imagine-video"})
        )
        .is_err()
    );
}

#[test]
fn xai_video_uses_supported_references_frames_and_separate_edit_routes() {
    let model = "direct/xai/grok-imagine-video-1.5";
    let mut keyframe = asset("keyframe");
    keyframe["timestampSeconds"] = json!(2.5);
    let value = spec(
        model,
        json!({"reference_voices":["ara"]}),
        json!([asset("image"), asset("first"), asset("last"), keyframe]),
    );
    let (path, body) = native_media::body(&value, &vec!["image-data".into(); 4]).unwrap();
    assert_eq!(path, "/v1/videos/generations");
    assert_eq!(body["keyframes"][0]["timestamp_s"], 2.5);
    assert_eq!(body["reference_audios"][0]["voice_id"], "ara");
    assert_eq!(body["reference_images"].as_array().unwrap().len(), 1);
    assert!(native_media::spec(&json!({"model":model,"prompt":"test","parameters":{"resolution":"1080p"},"references":[asset("image")]})).is_err());
    let mut source = asset("video");
    source["mime"] = json!("video/mp4");
    source["durationMs"] = json!(8000);
    let edit = spec(
        "direct/xai/grok-imagine-video",
        json!({"task":"edit"}),
        json!([source]),
    );
    let (path, body) = native_media::body(&edit, &["video-data".into()]).unwrap();
    assert_eq!(path, "/v1/videos/edits");
    assert!(body.get("duration").is_none());
    assert!(body.get("task").is_none());
    assert!(body.get("resolution").is_none());
}

#[test]
fn audio_settings_preserve_voice_and_provider_supported_formats() {
    let audio = spec(
        "direct/openai/gpt-audio-1.5",
        json!({"audio":{"voice":{"id":"voice_fixture"},"format":"flac"},"max_completion_tokens":16384}),
        json!([]),
    );
    let (path, body) = native_media::body(&audio, &[]).unwrap();
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(body["audio"]["voice"]["id"], "voice_fixture");
    assert_eq!(body["modalities"], json!(["text", "audio"]));
    for codec in ["mp3", "wav", "pcm", "mulaw", "alaw"] {
        let format = if codec == "mp3" {
            json!({"codec":codec,"sample_rate":48000,"bit_rate":192000})
        } else {
            json!({"codec":codec,"sample_rate":48000})
        };
        let speech = spec(
            "direct/xai/tts",
            json!({"voice_id":"custom_voice","output_format":format,"speed":0.7}),
            json!([]),
        );
        let (path, body) = native_media::body(&speech, &[]).unwrap();
        assert_eq!(path, "/v1/tts");
        assert_eq!(body["output_format"]["codec"], codec);
        assert!(body.get("model").is_none());
        assert!(body.get("prompt").is_none());
    }
}

#[test]
fn coordinator_and_worker_agree_on_signed_native_specs() {
    let vectors: Value = serde_json::from_str(include_str!("fixtures/native-media.json")).unwrap();
    for vector in vectors.as_array().unwrap() {
        let spec = everyframe_miner::models::spec(&vector["input"]).unwrap();
        assert_eq!(spec, vector["spec"]);
        assert_eq!(
            everyframe_miner::protocol::digest(&spec).unwrap(),
            vector["hash"]
        );
        everyframe_miner::models::check(&spec).unwrap();
    }
}
