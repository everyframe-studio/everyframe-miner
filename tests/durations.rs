use everyframe_miner::{models, protocol::digest};
use serde_json::json;

#[test]
fn supported_durations_preserve_canonical_bodies_and_reject_tampering() {
    for (model, min, max) in [
        (models::DEFAULT_MODEL, 1, 15),
        ("minimax/h3-max/text-to-video", 1, 15),
        ("bytedance/seedance-2.5/text-to-video", 4, 30),
    ] {
        let mut input = json!({"model":model,"prompt":"A cinematic journey"});
        if model.starts_with("minimax/") {
            input["seed"] = json!(42);
        }
        let original = models::spec(&input).unwrap();
        for duration in min..=max {
            input["duration"] = json!(duration);
            let spec = models::spec(&input).unwrap();
            models::check(&spec).unwrap();
            let expected = if model.starts_with("minimax/") {
                json!(duration)
            } else {
                json!(duration.to_string())
            };
            assert_eq!(spec["input"]["duration"], expected);
            assert_eq!(
                digest(&spec).unwrap() == digest(&original).unwrap(),
                duration == 5
            );
            let mut bad = spec.clone();
            bad["input"]["aspect_ratio"] = json!("9:16");
            assert!(models::check(&bad).is_err());
        }
        for invalid in [
            json!(min - 1),
            json!(max + 1),
            json!(5.5),
            json!("5"),
            json!(null),
        ] {
            input["duration"] = invalid;
            assert!(models::spec(&input).is_err());
        }
    }
}
