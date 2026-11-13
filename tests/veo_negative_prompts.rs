use everyframe_miner::{models, protocol, studio_video};
use serde_json::json;

#[test]
fn negative_prompt_is_hash_bound_and_preserved_at_both_providers() {
    let negative = " blur, flicker\nNo added text. ✨ ";
    for (id, provider) in [
        ("studio/openrouter/google/veo-3.1-negative", "openrouter"),
        ("studio/fal/fal-ai/veo3.1-negative", "fal"),
    ] {
        for frames in 0..=2 {
            let refs: Vec<_> = ["first", "last"].iter().take(frames).map(|role| json!({
                "url":format!("https://api.everyframe.studio/v1/attachment/att_{role}?t={}","a".repeat(32)),
                "sha256":"a".repeat(64),"bytes":100,"mime":"image/jpeg","role":role,
                "width":1280,"height":720,"durationMs":0
            })).collect();
            let input = json!({"model":id,"prompt":"A boat at sea.","negative":negative,"seed":7,"duration":8,"resolution":"1080p","aspect":"16:9","references":refs,"promptExpansion":"balanced"});
            let spec = models::spec(&input).unwrap();
            let urls: Vec<String> = ["https://example.test/first", "https://example.test/last"]
                .iter()
                .take(frames)
                .map(|s| s.to_string())
                .collect();
            let (path, body) = studio_video::body(&spec, &urls).unwrap();
            assert_eq!(body["prompt"], input["prompt"]);
            assert_eq!(body["generate_audio"], true);
            if provider == "openrouter" {
                assert_eq!(path, "/api/v1/videos");
                assert_eq!(
                    body["provider"]["options"]["google-vertex"]["parameters"]["negativePrompt"],
                    negative
                );
                assert_eq!(body["duration"], 8);
                if frames > 0 {
                    assert_eq!(body["frame_images"][0]["image_url"]["url"], urls[0]);
                }
            } else {
                assert_eq!(body["negative_prompt"], negative);
                assert_eq!(body["duration"], "8s");
                assert_eq!(
                    path,
                    [
                        "/fal-ai/veo3.1",
                        "/fal-ai/veo3.1/image-to-video",
                        "/fal-ai/veo3.1/first-last-frame-to-video"
                    ][frames]
                );
                if frames == 2 {
                    assert_eq!(body["last_frame_url"], urls[1]);
                }
            }
            let mut changed = input.clone();
            changed["negative"] = json!("different");
            assert_ne!(
                protocol::digest(&spec).unwrap(),
                protocol::digest(&models::spec(&changed).unwrap()).unwrap()
            );
            for invalid in [json!(null), json!(7), json!("😀".repeat(10001))] {
                changed["negative"] = invalid;
                assert!(models::spec(&changed).is_err());
            }
            changed.as_object_mut().unwrap().remove("negative");
            assert!(models::spec(&changed).is_err());
        }
    }
}

#[test]
fn old_contract_cannot_silently_discard_a_negative_prompt() {
    let input = json!({"model":"studio/openrouter/google/veo-3.1","prompt":"A boat.","negative":"blur","seed":7,"duration":8,"resolution":"1080p","aspect":"16:9","references":[],"promptExpansion":"balanced"});
    assert!(models::spec(&input).is_err());
}
