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
fn routing_categories_remain_fixed_when_provider_text_is_unrecognized() {
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

#[test]
fn provider_refusals_become_fixed_codes() {
    use everyframe_miner::providers::refusal;
    // Seedance through OpenRouter and fal: the real-person check.
    assert_eq!(
        refusal(
            "openrouter",
            &json!({"error":{"code":"InputImageSensitiveContentDetected.PrivacyInformation","message":"The request failed because the input image 'content[1]' may contain real person."}})
        ),
        Some("real_person")
    );
    assert_eq!(
        refusal(
            "fal",
            &json!({"detail":[{"msg":"The images or videos provided may contain likenesses of real people or other private information that cannot be processed.","type":"content_policy_violation"}]})
        ),
        Some("real_person")
    );
    // OpenRouter wraps the upstream body as a string in metadata.raw.
    assert_eq!(
        refusal(
            "openrouter",
            &json!({"error":{"code":400,"message":"Provider returned error","metadata":{"raw":"{\"error\":{\"code\":\"InputImageSensitiveContentDetected.PrivacyInformation\"}}"}}})
        ),
        Some("real_person")
    );
    assert_eq!(
        refusal(
            "fal",
            &json!({"error":"The request failed because the output video may be related to copyright restrictions."})
        ),
        Some("copyright")
    );
    assert_eq!(
        refusal(
            "fal",
            &json!({"detail":[{"msg":"Video pixel count must be greater than or equal to 407696","type":"value_error"}]})
        ),
        Some("reference_too_small")
    );
    assert_eq!(
        refusal(
            "fal",
            &json!({"detail":[{"type":"string_too_long","loc":["body","prompt"],"msg":"String should have at most 4000 characters","input":"x"}]})
        ),
        Some("prompt_too_long")
    );
    assert_eq!(
        refusal(
            "fal",
            &json!({"detail":[{"msg":"The content could not be processed because it contained material flagged by a content checker.","type":"content_policy_violation"}]})
        ),
        Some("content_policy")
    );
    assert_eq!(
        refusal(
            "pika",
            &json!({"status":"failed","error":{"code":"content_moderation","message":"real person"}})
        ),
        Some("content_policy")
    );
    // Pika prose never picks a code; neither do ordinary failures.
    assert_eq!(
        refusal(
            "pika",
            &json!({"status":"failed","error":{"code":"invalid_input","message":"may contain real person"}})
        ),
        None
    );
    assert_eq!(refusal("fal", &json!({"error":"Internal error"})), None);
    assert_eq!(
        refusal(
            "openrouter",
            &json!({"error":{"code":"invalid_value","message":"aspect_ratio must be one of 16:9, 9:16"}})
        ),
        None
    );
}

#[test]
fn refusals_ignore_echoed_request_fields() {
    use everyframe_miner::providers::refusal;
    // The prompt echoed in `input` must not choose the customer message.
    assert_eq!(
        refusal(
            "fal",
            &json!({"detail":[{"type":"value_error","msg":"Invalid duration","input":"a celebrity look-alike, copyright free"}]})
        ),
        None
    );
}

#[test]
fn a_refusal_never_changes_the_failure_category() {
    use everyframe_miner::submission::with_body_refusal;
    let body = br#"{"error":{"code":"InputImageSensitiveContentDetected.PrivacyInformation","message":"may contain real person"}}"#;
    let e = with_body_refusal(
        everyframe_miner::Error("http_400"),
        400,
        "openrouter.ai",
        body,
    );
    assert_eq!(e.0, "http_400");
    assert_eq!(e.refusal(), Some("real_person"));
    // 5xx, rate limits, oversize or malformed bodies are never tagged.
    for (status, bytes) in [
        (500u16, &body[..]),
        (429, &body[..]),
        (400, &b"not json"[..]),
    ] {
        assert_eq!(
            with_body_refusal(
                everyframe_miner::Error("http_x"),
                status,
                "openrouter.ai",
                bytes
            )
            .refusal(),
            None
        );
    }
    let big = vec![b' '; 64001];
    assert_eq!(
        with_body_refusal(
            everyframe_miner::Error("http_400"),
            400,
            "openrouter.ai",
            &big
        )
        .refusal(),
        None
    );
}
