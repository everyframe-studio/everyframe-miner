use everyframe_miner::providers::failure_category;
use everyframe_miner::worker::report_category;
use serde_json::json;

#[test]
fn provider_failures_map_to_fixed_categories() {
    assert_eq!(
        failure_category("bfl", &json!({"status":"Content Moderated"})),
        "provider_content_rejected"
    );
    assert_eq!(
        failure_category("vivix", &json!({"status":"failed","code":10005})),
        "provider_billing_required"
    );
    assert_eq!(
        failure_category("fal", &json!({"error":"Content policy violation"})),
        "provider_content_rejected"
    );
    assert_eq!(
        failure_category(
            "fal",
            &json!({"detail":[{"type":"literal_error","msg":"bad duration"}]})
        ),
        "provider_invalid_request"
    );
    assert_eq!(
        failure_category(
            "openrouter",
            &json!({"status":"failed","error":{"message":"Insufficient credits"}})
        ),
        "provider_billing_required"
    );
    assert_eq!(
        failure_category("fal", &json!({"error":"Request timed out"})),
        "provider_timeout"
    );
    assert_eq!(
        failure_category("fal", &json!({"error":"Internal error"})),
        "provider_generation_failed"
    );
}

#[test]
fn reports_never_carry_free_text() {
    for reason in [
        "provider_content_rejected",
        "vivix_billing_required",
        "provider_no_media",
        "something we never saw",
    ] {
        let category = report_category(reason);
        assert!(
            category.starts_with("provider_") || category == "output_invalid",
            "{category}"
        );
    }
    assert_eq!(
        report_category("vivix_billing_required"),
        "provider_billing_required"
    );
    assert_eq!(
        report_category("anything else"),
        "provider_generation_failed"
    );
}
