//! Explicitly funded, opt-in release canary. Never part of ordinary CI.
use everyframe_miner::{
    models,
    network::PublicHttp,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf, process::Command, time::Duration};

#[test]
#[ignore = "requires explicit paid-test approval, OpenRouter credentials and a fresh evidence directory"]
fn live_openrouter_expansion_outputs() {
    assert_eq!(
        std::env::var("EVERYFRAME_APPROVE_PAID_CANARY").as_deref(),
        Ok("openrouter-two-jobs-0.60-usd")
    );
    let root = PathBuf::from(std::env::var("EVERYFRAME_CANARY_OUTPUT_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    let key = std::env::var("OPENROUTER_API_KEY").expect("OpenRouter key required");
    assert!(!key.is_empty());
    let mut router = Router::new(&PublicHttp, json!({"OPENROUTER_API_KEY":key}));
    for (suffix, estimate_usd) in [("fast", 0.40), ("lite", 0.20)] {
        let model = format!("direct/openrouter/veo-3.1-{suffix}");
        let spec = models::spec(&json!({
            "model": model,
            "prompt": "Everyframe release verification test. A tiny silver paper boat floating gently across a quiet blue pond at sunrise. One continuous steady wide shot, realistic soft ripples, gentle water sounds. No text, people or logos."
        })).unwrap();
        // Create-new intent is the no-resubmit boundary, including after a crash
        // or ambiguous POST. A rerun must never create a second paid request.
        let path = root.join(format!("{suffix}-intent.json"));
        let mut intent = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("prior intent exists: reconcile it; never retry this paid submission");
        write!(intent, "{}", json!({"model":model,"estimateUsd":estimate_usd,"spec":spec,"at":everyframe_miner::now(),"label":"operator-funded-release-test"})).unwrap();
        intent.sync_all().unwrap();
        let submitted = router.submit_before(&spec, everyframe_miner::now() + 30_000);
        let reference = match submitted {
            Ok(r) => r,
            Err(e) => {
                fs::write(root.join(format!("{suffix}-error.json")),json!({"code":e.0,"submissionUncertain":everyframe_miner::submission::rejected(e.0).is_none()}).to_string()).unwrap();
                panic!("paid submission stopped: {} (no retry)", e.0);
            }
        };
        let mut accepted = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("{suffix}-accepted.json")))
            .unwrap();
        write!(accepted, "{}", json!({"reference":reference,"model":model})).unwrap();
        accepted.sync_all().unwrap();
        let deadline = everyframe_miner::now() + 480_000;
        loop {
            assert!(
                everyframe_miner::now() < deadline,
                "poll deadline elapsed; reconcile accepted request without resubmission"
            );
            let state = router
                .poll(&reference, &spec)
                .expect("poll failed: reconcile accepted request; do not resubmit");
            if state == "COMPLETED" {
                break;
            }
            std::thread::sleep(Duration::from_secs(5));
        }
        let out = router
            .result(&reference, &spec)
            .expect("retrieve failed: accepted request must not be resubmitted");
        let media = root.join(format!("{suffix}.mp4"));
        fs::write(&media, &out.media).unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&media)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let metadata: Value = serde_json::from_slice(&probe.stdout).unwrap();
        let streams = metadata["streams"].as_array().unwrap();
        let video = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
        assert_eq!(video["width"], 1280);
        assert_eq!(video["height"], 720);
        assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
        let duration: f64 = metadata["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((duration - 4.0).abs() < 0.3);
        let proof = json!({"model":model,"bytes":out.media.len(),"sha256":everyframe_miner::protocol::sha(&out.media),"responseHash":out.response_hash,"duration":duration,"width":1280,"height":720,"audio":true,"paidTest":true,"teeVerified":false});
        fs::write(root.join(format!("{suffix}-proof.json")), proof.to_string()).unwrap();
        println!("{proof}");
    }
}
