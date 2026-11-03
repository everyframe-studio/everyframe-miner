//! One explicitly approved paid request. Never run by ordinary CI.
use everyframe_miner::{
    models,
    network::PublicHttp,
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf, process::Command, time::Duration};

#[test]
#[ignore = "requires a private BFL key, explicit $0.10 approval and a new evidence directory"]
fn bfl_regional_image_round_trip() {
    assert_eq!(
        std::env::var("EVERYFRAME_APPROVE_PAID_CANARY").as_deref(),
        Ok("bfl-one-image-0.10-usd")
    );
    let root = PathBuf::from(std::env::var("EVERYFRAME_CANARY_OUTPUT_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    let key = std::env::var("BFL_API_KEY").expect("BFL key must be configured privately");
    let spec=models::spec(&json!({"model":"studio/native/bfl/flux-3-image",
        "prompt":"A square photograph of a small blue paper boat on still water. Soft daylight, no people, no words or logos."})).unwrap();
    let mut intent = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("intent.json"))
        .expect("An intent already exists. Reconcile it; do not submit another paid request.");
    write!(
        intent,
        "{}",
        json!({"spec":spec,"budgetMicrousd":100000,"at":everyframe_miner::now()})
    )
    .unwrap();
    intent.sync_all().unwrap();
    let mut router = Router::new(&PublicHttp, json!({"BFL_API_KEY":key}));
    let reference = router
        .submit_before(&spec, everyframe_miner::now() + 60000)
        .unwrap();
    let mut accepted = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("accepted.json"))
        .unwrap();
    write!(accepted, "{}", json!({"reference":reference})).unwrap();
    accepted.sync_all().unwrap();
    // Drop the submitting router: polling must work from the recorded reference.
    let credentials = router.credentials.clone();
    drop(router);
    finish_recorded_task(&root, &reference, &spec, credentials);
}

#[test]
#[ignore = "read-only recovery requires a private BFL key and an existing accepted task record"]
fn bfl_resume_recorded_task() {
    assert_eq!(
        std::env::var("EVERYFRAME_APPROVE_BFL_RECOVERY").as_deref(),
        Ok("read-only")
    );
    let root = PathBuf::from(std::env::var("EVERYFRAME_CANARY_OUTPUT_DIR").unwrap());
    let accepted: Value =
        serde_json::from_slice(&fs::read(root.join("accepted.json")).unwrap()).unwrap();
    let intent: Value =
        serde_json::from_slice(&fs::read(root.join("intent.json")).unwrap()).unwrap();
    let key = std::env::var("BFL_API_KEY").expect("BFL key must be configured privately");
    finish_recorded_task(
        &root,
        accepted["reference"].as_str().unwrap(),
        &intent["spec"],
        json!({"BFL_API_KEY": key}),
    );
}

fn finish_recorded_task(root: &std::path::Path, reference: &str, spec: &Value, credentials: Value) {
    let mut resumed = Router::new(&PublicHttp, credentials);
    let end = everyframe_miner::now() + 600000;
    loop {
        assert!(
            everyframe_miner::now() < end,
            "Timed out: recover this task ID; never resubmit."
        );
        if resumed.poll(reference, spec).unwrap() == "COMPLETED" {
            break;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let result = resumed.result(reference, spec).unwrap();
    let bundle: Value = serde_json::from_slice(&result.media).unwrap();
    let parts = bundle["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["type"], "image");
    let bytes = protocol::decode(parts[0]["data"].as_str().unwrap()).unwrap();
    let output = root.join("image.bin");
    fs::write(&output, &bytes).unwrap();
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(decoded.status.success(), "Image decode failed");
    let proof = json!({"model":spec["model"],"reference":reference,"bytes":bytes.len(),"sha256":protocol::sha(&bytes),
        "bundleSha256":protocol::sha(&result.media),"providerResponseHash":result.response_hash,"paidTest":true,"teeVerified":false});
    fs::write(root.join("proof.json"), proof.to_string()).unwrap();
    println!("{proof}");
}
