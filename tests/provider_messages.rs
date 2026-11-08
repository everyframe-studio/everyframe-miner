use everyframe_miner::{Error, provider_error, submission};
use serde_json::json;
#[test]
fn unfamiliar_provider_codes_and_wording_survive_without_a_taxonomy_update() {
    let d=provider_error::extract(&json!({"status":"failed","error":{"code":"NewGeometryRule.2026","message":"Reference mesh requires a fourth anchor."}}),&[]).unwrap();
    assert_eq!(d.code.as_deref(), Some("NewGeometryRule.2026"));
    assert_eq!(
        d.message.as_deref(),
        Some("Reference mesh requires a fourth anchor.")
    );
    assert!(!d.truncated);
    let error = Error("provider_generation_failed")
        .with_refusal(Some("content_policy"))
        .with_provider_error(Some(d));
    assert_eq!(error.0, "provider_generation_failed");
    assert_eq!(error.refusal(), Some("content_policy"));
    assert!(error.provider_error().is_some());
}
#[test]
fn diagnostics_ignore_echoed_inputs_and_redact_credentials_before_truncating() {
    let data = json!({"error":{"code":"UNRECOGNIZED","message":"Original wording api_key=unsafe-value https://private.test/file?token=sign Bearer abc.def ghi-real-key.\u{1b}[31m\u{202e}Human message."},"input":{"message":"DO NOT FORWARD REQUEST ECHO"},"request":{"error":"DO NOT FORWARD REQUEST"}});
    let d = provider_error::extract(&data, &["ghi-real-key".into()]).unwrap();
    let text = d.value().to_string();
    for secret in [
        "unsafe-value",
        "private.test",
        "abc.def",
        "ghi-real-key",
        "DO NOT FORWARD",
        "\u{1b}",
        "\u{202e}",
    ] {
        assert!(!text.contains(secret), "{text}");
    }
    assert!(text.contains("Original wording"));
    assert!(text.contains("Human message"));
    let d = provider_error::extract(
        &json!({"error":{"code":"X".repeat(256),"message":"界".repeat(2000)}}),
        &[],
    )
    .unwrap();
    assert!(d.truncated);
    assert!(d.message.unwrap().len() <= 4096);
    assert_eq!(d.code.unwrap().len(), 128);
}
#[test]
fn upstream_json_raw_metadata_is_extracted_without_shipping_request_bodies() {
    let raw=json!({"error":{"code":"NEW_VALIDATION","message":"An unknown segmentation restriction."},"input":{"prompt":"PRIVATE PROMPT"}}).to_string();
    let d = provider_error::extract(&json!({"error":{"metadata":{"raw":raw}}}), &[]).unwrap();
    assert_eq!(d.code.as_deref(), Some("NEW_VALIDATION"));
    assert_eq!(
        d.message.as_deref(),
        Some("An unknown segmentation restriction.")
    );
    assert!(
        provider_error::extract(
            &json!({"request":{"message":"private"},"input":"private"}),
            &[]
        )
        .is_none()
    );
}
#[test]
fn http_details_do_not_upgrade_a_timeout_or_server_error_into_a_confirmed_rejection() {
    for status in [400, 403, 429, 500, 502] {
        let error = submission::with_body_provider_error(
            Error("http_502"),
            status,
            "generativelanguage.googleapis.com",
            br#"{"error":{"code":"NovelCode","message":"Novel provider explanation"}}"#,
            &[],
        );
        assert_eq!(error.0, "http_502");
        assert!(error.provider_error().is_some());
        assert_eq!(error.refusal(), None);
    }
}
