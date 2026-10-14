//! Free, read-only API key checks used before a key is saved. Each check calls one
//! account or listing endpoint that never starts a generation or spends credit.
//! Only the HTTP status (and, where a provider answers 200 with an error, a fixed
//! numeric code) is inspected; provider response text is never shown or stored.
use crate::network::{Http, Request};
use serde_json::{Value, json};

/// Outcome of a key check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The provider accepted the key.
    Valid,
    /// The provider refused the key (wrong, revoked or missing permission).
    Rejected,
    /// The check could not reach a conclusion (network, rate limit, outage).
    Unverified,
}

struct Probe {
    url: String,
    host: &'static str,
    headers: Vec<(String, String)>,
}

fn probe(provider: &str, key: &str) -> Option<Probe> {
    let bearer = || vec![("Authorization".into(), format!("Bearer {key}"))];
    let (url, host, headers): (String, &str, Vec<(String, String)>) = match provider {
        "phala" => (
            "https://cloud-api.phala.com/api/v1/auth/me".into(),
            "cloud-api.phala.com",
            vec![
                ("X-API-Key".into(), key.into()),
                ("X-Phala-Version".into(), "2026-06-23".into()),
            ],
        ),
        // A generation key reads its own balance here; the billing API needs admin scope.
        "fal" => (
            "https://rest.alpha.fal.ai/billing/user_balance".into(),
            "rest.alpha.fal.ai",
            vec![("Authorization".into(), format!("Key {key}"))],
        ),
        "fal-billing" => (
            "https://api.fal.ai/v1/account/billing?expand=credits".into(),
            "api.fal.ai",
            vec![("Authorization".into(), format!("Key {key}"))],
        ),
        "openrouter" => (
            "https://openrouter.ai/api/v1/key".into(),
            "openrouter.ai",
            bearer(),
        ),
        "openrouter-billing" => (
            "https://openrouter.ai/api/v1/credits".into(),
            "openrouter.ai",
            bearer(),
        ),
        "vivix" => (
            "https://api.vivix.ai/v1/balance".into(),
            "api.vivix.ai",
            bearer(),
        ),
        "minimax" => (
            "https://api.minimax.io/v1/files/list?purpose=video_generation".into(),
            "api.minimax.io",
            bearer(),
        ),
        "bfl" => (
            "https://api.bfl.ai/v1/credits".into(),
            "api.bfl.ai",
            vec![("x-key".into(), key.into())],
        ),
        "replicate" => (
            "https://api.replicate.com/v1/account".into(),
            "api.replicate.com",
            bearer(),
        ),
        "google" => (
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1".into(),
            "generativelanguage.googleapis.com",
            vec![("x-goog-api-key".into(), key.into())],
        ),
        "runway" => (
            "https://api.dev.runwayml.com/v1/organization".into(),
            "api.dev.runwayml.com",
            {
                let mut h = bearer();
                h.push(("X-Runway-Version".into(), "2024-11-06".into()));
                h
            },
        ),
        "luma" => (
            "https://api.lumalabs.ai/dream-machine/v1/generations?limit=1".into(),
            "api.lumalabs.ai",
            bearer(),
        ),
        "elevenlabs" => (
            "https://api.elevenlabs.io/v1/user".into(),
            "api.elevenlabs.io",
            vec![("xi-api-key".into(), key.into())],
        ),
        _ => return None,
    };
    Some(Probe { url, host, headers })
}

/// Maps a failed request to a verdict. Authentication failures are rejections;
/// everything else (timeouts, 429, 5xx) leaves the key unverified.
fn classify_error(provider: &str, code: &str) -> Verdict {
    match (provider, code) {
        // A forbidden billing query does not prove generation access.
        ("fal", "http_403") => Verdict::Unverified,
        // Vivix answers a funded-or-not question; a billing code still proves the key.
        ("vivix", "vivix_billing_required") => Verdict::Valid,
        ("fal-billing", "http_403") => Verdict::Rejected,
        (_, "http_401" | "vivix_auth_rejected") => Verdict::Rejected,
        _ => Verdict::Unverified,
    }
}

/// Checks one key. Returns the verdict and a fixed reason code for display.
pub fn check(http: &dyn Http, provider: &str, key: &str) -> (Verdict, &'static str) {
    let Some(p) = probe(provider, key) else {
        return (Verdict::Unverified, "key_check_unsupported");
    };
    let mut req = Request::get(p.url, &[p.host]);
    req.headers = p.headers;
    req.timeout = 8;
    req.limit = 256_000;
    match http.json(req) {
        Ok(v) => {
            // MiniMax answers 200 with base_resp.status_code (1004 = authentication).
            if provider == "minimax" {
                return match v["base_resp"]["status_code"].as_i64() {
                    Some(0) => (Verdict::Valid, "key_accepted"),
                    Some(1004) => (Verdict::Rejected, "key_rejected"),
                    _ => (Verdict::Unverified, "key_check_inconclusive"),
                };
            }
            let recognized = match provider {
                "vivix" => v["code"] == 0 && v["data"]["balances"].is_array(),
                "openrouter" => v["data"]["label"].is_string(),
                "openrouter-billing" => v["data"]["total_credits"].is_number(),
                "elevenlabs" => v["user_id"].is_string(),
                "fal" => v.is_number(),
                "fal-billing" => v["credits"]["current_balance"].is_number(),
                "phala" => v["credits"].is_object() || v["user"].is_object(),
                "google" => v["models"].is_array(),
                "replicate" => v["username"].is_string(),
                "bfl" => v["credits"].is_number(),
                "runway" => v["creditBalance"].is_number(),
                "luma" => v["generations"].is_array(),
                _ => false,
            };
            if !recognized {
                return (Verdict::Unverified, "key_check_inconclusive");
            }
            (Verdict::Valid, "key_accepted")
        }
        // Any response body that is not JSON still means the request got through.
        Err(e) if e.0 == "invalid_remote_json" => (Verdict::Unverified, "key_check_inconclusive"),
        Err(e) => match classify_error(provider, e.0) {
            Verdict::Valid => (Verdict::Valid, "key_accepted"),
            Verdict::Rejected => (
                Verdict::Rejected,
                if provider == "fal-billing" {
                    "key_not_admin"
                } else {
                    "key_rejected"
                },
            ),
            Verdict::Unverified => (
                Verdict::Unverified,
                if e.0 == "http_403" {
                    "key_check_permission"
                } else {
                    "key_check_unreachable"
                },
            ),
        },
    }
}

pub fn verdict_json(verdict: Verdict, reason: &str) -> Value {
    json!({
        "result": match verdict {
            Verdict::Valid => "valid",
            Verdict::Rejected => "rejected",
            Verdict::Unverified => "unverified",
        },
        "reason": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, Result};
    struct Fixed(std::result::Result<&'static str, &'static str>);
    impl Http for Fixed {
        fn bytes(&self, _: Request) -> Result<Vec<u8>> {
            self.0.map(|b| b.as_bytes().to_vec()).map_err(Error)
        }
    }
    #[test]
    fn classifies_provider_answers() {
        let ok = Fixed(Ok(r#"{"data":{"label":"test"}}"#));
        assert_eq!(check(&ok, "openrouter", "k").0, Verdict::Valid);
        assert_eq!(
            check(&Fixed(Err("http_401")), "openrouter", "k").0,
            Verdict::Rejected
        );
        assert_eq!(
            check(&Fixed(Err("http_403")), "fal", "k").0,
            Verdict::Unverified
        );
        assert_eq!(
            check(&Fixed(Err("http_403")), "fal-billing", "k"),
            (Verdict::Rejected, "key_not_admin")
        );
        assert_eq!(
            check(&Fixed(Err("http_400")), "google", "k").0,
            Verdict::Unverified
        );
        assert_eq!(
            check(&Fixed(Err("http_400")), "replicate", "k").0,
            Verdict::Unverified
        );
        assert_eq!(
            check(&Fixed(Err("request_timeout")), "luma", "k").0,
            Verdict::Unverified
        );
        assert_eq!(
            check(&Fixed(Err("http_429")), "runway", "k").0,
            Verdict::Unverified
        );
        assert_eq!(
            check(
                &Fixed(Ok(r#"{"base_resp":{"status_code":1004}}"#)),
                "minimax",
                "k"
            )
            .0,
            Verdict::Rejected
        );
        assert_eq!(
            check(
                &Fixed(Ok(r#"{"base_resp":{"status_code":0}}"#)),
                "minimax",
                "k"
            )
            .0,
            Verdict::Valid
        );
        assert_eq!(
            check(&Fixed(Err("vivix_auth_rejected")), "vivix", "k").0,
            Verdict::Rejected
        );
        assert_eq!(check(&ok, "unknown", "k").0, Verdict::Unverified);
        for provider in [
            "fal",
            "phala",
            "vivix",
            "openrouter",
            "elevenlabs",
            "bfl",
            "google",
            "runway",
            "luma",
            "replicate",
        ] {
            assert_eq!(
                check(&Fixed(Ok("{}")), provider, "k").0,
                Verdict::Unverified
            );
            assert_eq!(
                check(&Fixed(Ok(r#"{"error":"denied"}"#)), provider, "k").0,
                Verdict::Unverified
            );
        }
    }
}
