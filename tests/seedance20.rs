use everyframe_miner::{asian_media, models, protocol, studio_video};
use serde_json::{Value, json};

fn input(model: &str) -> Value {
    json!({"model":model,"prompt":"A quiet lake","seed":42,"duration":5,"resolution":"4k","aspect":"16:9","references":[],"promptExpansion":"balanced"})
}
#[test]
fn direct_and_studio_requests_match_coordinator_hashes() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/seedance20.json")).unwrap();
    for f in fixtures.as_array().unwrap() {
        let spec = models::spec(&f["request"]).unwrap();
        assert_eq!(spec, f["spec"]);
        assert_eq!(protocol::digest(&spec).unwrap(), f["inputHash"]);
        if spec["model"].as_str().unwrap().starts_with("direct/") {
            assert_eq!(
                protocol::digest(models::info(spec["model"].as_str().unwrap()).unwrap()).unwrap(),
                f["contractHash"]
            );
        }
    }
}
#[test]
fn transports_preserve_4k_without_unknown_fields_or_silent_downgrade() {
    let fal = input("studio/fal/bytedance/seedance-2.0/text-to-video");
    let spec = models::spec(&fal).unwrap();
    let (path, body) = studio_video::body(&spec, &[]).unwrap();
    assert_eq!(path, "/bytedance/seedance-2.0/text-to-video");
    assert_eq!(body["resolution"], "4k");
    assert_eq!(body["duration"], "5");
    assert!(body.get("seed").is_none());
    assert_eq!(body["generate_audio"], true);
    let or = models::spec(&input("studio/openrouter/bytedance/seedance-2.0")).unwrap();
    let (path, body) = studio_video::body(&or, &[]).unwrap();
    assert_eq!(path, "/api/v1/videos");
    assert_eq!(body["resolution"], "4K");
    assert_eq!(body["seed"], 42);
    assert_eq!(body["generate_audio"], true);
    let direct=models::spec(&json!({"model":"direct/byteplus/seedance-2.0","prompt":"A lake","parameters":{"resolution":"4k"}})).unwrap();
    let b = asian_media::body(
        &direct,
        models::info("direct/byteplus/seedance-2.0").unwrap(),
        &[],
    )
    .unwrap();
    assert_eq!(b["model"], "dreamina-seedance-2-0-260128");
    assert_eq!(b["resolution"], "4k");
    for resolution in ["2k", "8k"] {
        let mut bad = fal.clone();
        bad["resolution"] = json!(resolution);
        assert!(models::spec(&bad).is_err());
    }
}
#[test]
fn first_last_and_omni_references_are_preserved_in_their_exact_fields() {
    let mut v = input("studio/fal/bytedance/seedance-2.0/text-to-video");
    let asset = |role: &str| json!({"url":"https://api.everyframe.studio/v1/attachment/att_fixture?t=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sha256":"a".repeat(64),"bytes":128,"mime":if role=="audio"{"audio/wav"}else if role=="video"{"video/mp4"}else{"image/png"},"role":role,"width":1280,"height":720,"durationMs":if role=="audio"||role=="video"{2000}else{0}});
    v["references"] = json!([asset("first"), asset("last")]);
    let s = models::spec(&v).unwrap();
    let (p, b) = studio_video::body(&s, &["first".into(), "last".into()]).unwrap();
    assert!(p.ends_with("/image-to-video"));
    assert_eq!(b["image_url"], "first");
    assert_eq!(b["end_image_url"], "last");
    v["references"] = json!([asset("image"), asset("video"), asset("audio")]);
    let s = models::spec(&v).unwrap();
    let (p, b) = studio_video::body(&s, &["image".into(), "video".into(), "audio".into()]).unwrap();
    assert!(p.ends_with("/reference-to-video"));
    assert_eq!(b["image_urls"], json!(["image"]));
    assert_eq!(b["video_urls"], json!(["video"]));
    assert_eq!(b["audio_urls"], json!(["audio"]));
    v["references"] = json!([asset("audio")]);
    assert!(models::spec(&v).is_err());
    v["references"] = json!([asset("video")]);
    v["references"][0]["width"] = json!(1920);
    v["references"][0]["height"] = json!(1080);
    assert!(models::spec(&v).is_err());
}
