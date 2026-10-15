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
        "provider_secret_prompt_and_key",
        "output_secret_prompt_and_key",
    ] {
        let category = report_category(reason);
        assert!(
            [
                "provider_content_rejected",
                "provider_invalid_request",
                "provider_billing_required",
                "provider_timeout",
                "provider_unavailable",
                "output_invalid",
                "provider_generation_failed",
            ]
            .contains(&category),
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

#[test]
fn existing_provider_codes_preserve_categories() {
    for (code, category) in [
        ("vivix_content_rejected", "provider_content_rejected"),
        ("vivix_invalid_parameters", "provider_invalid_request"),
        ("fal_billing_required", "provider_billing_required"),
        ("vivix_billing_required", "provider_billing_required"),
        ("http_402", "provider_billing_required"),
        ("http_422", "provider_invalid_request"),
        ("http_403", "provider_invalid_request"),
        ("vivix_unavailable", "provider_unavailable"),
        ("invalid_media_size", "output_invalid"),
    ] {
        assert_eq!(report_category(code), category, "{code}");
    }
}

#[test]
fn structured_types_win_and_echoed_request_fields_are_ignored() {
    for echoed in ["credit", "safety", "nsfw", "timeout", "invalid"] {
        assert_eq!(
            failure_category(
                "fal",
                &json!({"detail":[{
                    "type":"literal_error", "msg":"Input should be 480p or 720p",
                    "input":echoed, "ctx":{"error":echoed}, "loc":["body", echoed]
                }]})
            ),
            "provider_invalid_request",
            "{echoed}"
        );
        assert_eq!(
            failure_category(
                "fal",
                &json!({"error":{
                    "message":"Internal error", "input":echoed, "prompt":echoed,
                    "request":{"error":echoed}, "context":{"message":echoed}
                }})
            ),
            "provider_generation_failed",
            "{echoed}"
        );
    }
    assert_eq!(
        failure_category(
            "fal",
            &json!({"error":{
                "code":"content_policy_violation", "message":"Insufficient credits"
            }})
        ),
        "provider_content_rejected"
    );
    assert_eq!(
        failure_category(
            "fal",
            &json!({"detail":[{
                "type":"literal_error", "msg":"Invalid value 'credit'"
            }]})
        ),
        "provider_invalid_request"
    );
}

#[test]
fn failure_messages_do_not_invent_billing_from_generic_resource_errors() {
    for message in [
        "Insufficient memory",
        "Quota exceeded for concurrent requests",
        "Credit service timed out",
    ] {
        assert_ne!(
            failure_category("fal", &json!({"error":message})),
            "provider_billing_required"
        );
    }
    for message in [
        "Insufficient credits",
        "Insufficient balance",
        "Payment required",
        "User is locked. Reason: Exhausted balance",
    ] {
        assert_eq!(
            failure_category("fal", &json!({"error":message})),
            "provider_billing_required"
        );
    }
}
