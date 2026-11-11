use everyframe_miner::{models, native_media, worker_diagnostic};
use serde_json::json;

#[test]
fn failure_metadata_preserves_stage_and_code_without_payloads_or_urls() {
    let spec = json!({"model":"studio/wavespeed/bytedance/seedream-v5.5","input":{
        "prompt":"PRIVATE PROMPT", "parameters":{}, "references":[]}});
    let reference = native_media::reference(&spec, "provider_task_123").unwrap();
    let d = worker_diagnostic::report(
        "output_retrieval",
        "untrusted_destination",
        &spec,
        Some(&reference),
    );
    assert_eq!(d["phase"], "output_retrieval");
    assert_eq!(d["code"], "untrusted_destination");
    assert_eq!(d["provider"], "wavespeed");
    assert_eq!(d["providerCompleted"], true);
    assert_eq!(d["providerRequestId"], "provider_task_123");
    assert!(!d.to_string().contains("PRIVATE"));
    assert!(d.to_string().len() < 512);
    let d = worker_diagnostic::report(
        "output_retrieval",
        "http_403",
        &spec,
        Some("https://private/file?token=secret"),
    );
    assert_eq!(d["httpStatus"], 403);
    assert!(d["providerRequestId"].is_null());
    assert!(!d.to_string().contains("secret"));
}

#[test]
fn diagnostics_do_not_invent_completion_or_forward_untrusted_codes() {
    let spec = json!({"model":models::DEFAULT_MODEL,"input":{}});
    let d = worker_diagnostic::report("provider_poll", "invalid_provider_status", &spec, None);
    assert_eq!(d["providerCompleted"], false);
    let d = worker_diagnostic::report("PRIVATE STAGE", "error: sk-secret-value", &spec, None);
    assert_eq!(d["code"], "unknown_worker_error");
    assert_eq!(d["phase"], "provider_poll");
    assert!(!d.to_string().contains("PRIVATE"));
    assert!(!d.to_string().contains("secret"));
}
