//! Fixed, non-secret diagnostics; never infer rejection from a lost response.
pub fn rejected(code: &str) -> Option<&'static str> {
    match code {
        "http_401" | "vivix_auth_rejected" => Some("provider_auth_rejected"),
        "http_402" | "vivix_billing_required" => Some("provider_billing_required"),
        "http_429" | "vivix_capacity_limited" => Some("provider_capacity_limited"),
        "provider_key_required" => Some("provider_key_required"),
        "vivix_content_rejected" => Some("provider_content_rejected"),
        // Generic 403 may be moderation. It must never cause cross-miner retry.
        "http_400"
        | "http_403"
        | "http_404"
        | "http_405"
        | "http_422"
        | "vivix_invalid_parameters" => Some("provider_invalid_request"),
        _ => None,
    }
}
pub fn uncertain(code: &str) -> &'static str {
    match code {
        "request_timeout" => "request_timeout",
        "connection_failed" => "connection_failed",
        "remote_request_failed" => "remote_request_failed",
        "response_read_failed" => "response_read_failed",
        "invalid_provider_response" => "invalid_provider_response",
        "invalid_provider_ref" => "invalid_provider_ref",
        "http_500" | "http_502" | "http_503" | "http_504" | "vivix_unavailable" => {
            "provider_unavailable"
        }
        _ => "submission_unknown",
    }
}
pub fn vivix_error(code: i64) -> crate::Error {
    crate::Error(match code {
        10001 | 10003 => "vivix_auth_rejected",
        10004 | 10005 => "vivix_billing_required",
        10008 | 30008 => "vivix_capacity_limited",
        20004 | 20008 => "vivix_invalid_parameters",
        20005 => "vivix_job_not_found",
        30001 => "vivix_content_rejected",
        50001 | 50003 => "vivix_unavailable",
        _ => "provider_rejected",
    })
}
pub fn vivix_http_error(status: u16, bytes: &[u8]) -> crate::Error {
    // 5xx remains ambiguous even when a contradictory body contains a client code.
    if status < 500
        && bytes.len() <= 64000
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes)
        && let Some(code) = v["code"].as_i64().filter(|code| *code != 0)
    {
        return vivix_error(code);
    }
    crate::diagnostics::http_error(status)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vivix_non_success_envelopes_preserve_fixed_codes_not_messages() {
        assert_eq!(
            vivix_http_error(403, br#"{"code":30001,"message":"secret prompt"}"#).0,
            "vivix_content_rejected"
        );
        assert_eq!(
            vivix_http_error(403, br#"{"code":10003,"message":"secret key"}"#).0,
            "vivix_auth_rejected"
        );
        assert_eq!(
            vivix_http_error(402, br#"{"code":10005}"#).0,
            "vivix_billing_required"
        );
        assert_eq!(vivix_http_error(503, br#"{"code":10005}"#).0, "http_503");
        assert_eq!(vivix_http_error(403, b"invalid JSON").0, "http_403");
        assert_eq!(vivix_http_error(403, &vec![b'x'; 64001]).0, "http_403");
    }
    #[test]
    fn ambiguity_and_moderation_never_become_retryable_rejections() {
        for code in [
            "request_timeout",
            "http_408",
            "http_409",
            "http_500",
            "http_503",
            "invalid_provider_response",
            "invalid_provider_ref",
            "secret upstream body",
        ] {
            assert_eq!(rejected(code), None);
        }
        assert_eq!(rejected("http_403"), Some("provider_invalid_request"));
        assert_eq!(
            rejected(vivix_error(30001).0),
            Some("provider_content_rejected")
        );
        assert_eq!(uncertain("secret upstream body"), "submission_unknown");
        assert_eq!(
            rejected(vivix_error(10005).0),
            Some("provider_billing_required")
        );
    }
}
