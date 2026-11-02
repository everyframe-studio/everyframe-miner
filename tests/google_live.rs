//! Explicitly funded verification only. Ordinary tests never contact Google.
use everyframe_miner::{
    models,
    network::PublicHttp,
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf, process::Command, time::Duration};

#[test]
#[ignore = "requires Google key, explicit $2 approval and fresh private evidence directory"]
fn live_google_media_outputs() {
    assert!(
        everyframe_miner::now()
            < chrono::DateTime::parse_from_rfc3339("2026-11-16T00:00:00Z")
                .unwrap()
                .timestamp_millis(),
        "Review Google pricing again before using this budget"
    );
    assert_eq!(
        std::env::var("EVERYFRAME_APPROVE_PAID_CANARY").as_deref(),
        Ok("google-eight-models-2-usd")
    );
    let root = PathBuf::from(std::env::var("EVERYFRAME_CANARY_OUTPUT_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    let key = std::env::var("GEMINI_API_KEY").expect("Google key must be configured privately");
    let mut router = Router::new(&PublicHttp, json!({"GEMINI_API_KEY":key}));
    let catalog: Value =
        serde_json::from_str(include_str!("../config/google-models.json")).unwrap();
    let caps = [
        585728u64, 135168, 124928, 249856, 507904, 80000, 40960, 28672,
    ];
    assert!(caps.iter().sum::<u64>() <= 2_000_000);
    router.refresh_availability(everyframe_miner::now());
    let available = router.models();
    // Check access for the entire set before spending on any generation.
    for model in catalog.as_object().unwrap().keys() {
        assert!(
            available.contains(model),
            "Google account does not expose {model}"
        );
    }
    for ((model, m), cap) in catalog.as_object().unwrap().iter().zip(caps) {
        let name = model.rsplit('/').next().unwrap();
        let prompt = if m["kind"] == "video" || m["kind"] == "image" {
            "A small blue paper boat on calm water under soft sunlight. Single steady view, no people or text."
        } else if m["apiModel"] == "lyria-3.5" {
            "A gentle original acoustic instrumental with piano and soft guitar, calm and minimal, no vocals."
        } else {
            "This is an EveryFrame provider verification recording."
        };
        let spec = models::spec(&json!({"model":model,"prompt":prompt})).unwrap();
        // Create-new and fsync are the no-resubmission boundary, even after a
        // process crash or an ambiguous provider response.
        let mut intent = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("{name}-intent.json")))
            .expect("existing intent: reconcile; do not resubmit");
        write!(
            intent,
            "{}",
            json!({"model":model,"reservedMicrousd":cap,"spec":spec,"at":everyframe_miner::now()})
        )
        .unwrap();
        intent.sync_all().unwrap();
        let reference = match router.submit_before(&spec, everyframe_miner::now() + 60000) {
            Ok(r) => r,
            Err(e) => {
                fs::write(root.join(format!("{name}-error.json")),json!({"code":e.0,"uncertain":everyframe_miner::submission::rejected(e.0).is_none()}).to_string()).unwrap();
                panic!("Google submission stopped: {}. No retry.", e.0);
            }
        };
        let mut accepted = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("{name}-accepted.json")))
            .unwrap();
        write!(accepted, "{}", json!({"model":model,"reference":reference})).unwrap();
        accepted.sync_all().unwrap();
        let deadline = everyframe_miner::now() + 480000;
        loop {
            assert!(
                everyframe_miner::now() < deadline,
                "deadline: reconcile existing reference"
            );
            let state = router
                .poll(&reference, &spec)
                .expect("poll failed: reconcile existing reference");
            if state == "COMPLETED" {
                break;
            }
            assert_ne!(state, "FAILED", "provider failure: no resubmission");
            std::thread::sleep(Duration::from_secs(5));
        }
        let out = router
            .result(&reference, &spec)
            .expect("result unavailable: reconcile existing reference");
        let ext = match m["mime"].as_str().unwrap() {
            "video/mp4" => "mp4",
            "image/jpeg" => "jpg",
            "audio/mpeg" => "mp3",
            _ => "wav",
        };
        let path = root.join(format!("{name}.{ext}"));
        fs::write(&path, &out.media).unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&path)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        assert!(decode.status.success());
        fs::write(root.join(format!("{name}-probe.json")), probe.stdout).unwrap();
        fs::write(root.join(format!("{name}-result.json")),json!({"model":model,"reference":reference,"outputHash":protocol::sha(&out.media),"responseHash":out.response_hash,"bytes":out.media.len()}).to_string()).unwrap();
    }
}
