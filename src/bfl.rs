//! BFL returns the authoritative polling cluster after accepting a paid task.
//! Keep its task ID even when a response URL cannot be used safely.
use crate::{Error, Result, matches, need, network::safe_url, s};
use serde_json::{Value, json};
use url::Url;

pub const POLLING_REJECTED: &str = "bfl_polling_url_rejected";
pub const DELIVERY_REJECTED: &str = "bfl_delivery_url_rejected";

fn label(value: &str) -> bool {
    matches("[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", &json!(value))
}
fn task_id(value: &str) -> Result<()> {
    need(
        matches("[A-Za-z0-9_-]{1,80}", &json!(value)),
        "invalid_provider_ref",
    )
}
fn scope(url: &str, id: &str) -> Result<String> {
    let u = safe_url(url, &["*.bfl.ai".into()]).map_err(|_| Error(POLLING_REJECTED))?;
    need(
        u.path() == "/v1/get_result"
            && u.query_pairs().count() == 1
            && u.query_pairs().any(|(k, v)| k == "id" && v == id),
        POLLING_REJECTED,
    )?;
    let host = u.host_str().ok_or(Error(POLLING_REJECTED))?;
    if host == "api.bfl.ai" {
        return Ok("global".into());
    }
    let region = host
        .strip_prefix("api.")
        .and_then(|h| h.strip_suffix(".bfl.ai"))
        .filter(|r| label(r))
        .ok_or(Error(POLLING_REJECTED))?;
    // Prefix new cluster labels so api.global.bfl.ai cannot collide with the
    // old "global" reference. Old us1/eu1 references remain readable below.
    Ok(format!("h{region}"))
}

pub fn submission_reference(data: &Value) -> Result<String> {
    let id = s(&data["id"])?;
    task_id(id)?;
    let bound = data["polling_url"]
        .as_str()
        .and_then(|u| scope(u, id).ok())
        .map(|region| format!("{region}_{id}"));
    // A malformed URL must not discard an already accepted provider task ID.
    // The quarantine reference is recorded before polling stops the provider.
    Ok(bound
        .filter(|r| r.len() <= 100)
        .unwrap_or_else(|| format!("unresolved_{id}")))
}

pub fn polling_url(bound: &str) -> Result<(String, String)> {
    let (region, id) = bound.split_once('_').ok_or(Error("invalid_provider_ref"))?;
    task_id(id)?;
    need(region != "unresolved", POLLING_REJECTED)?;
    let host = match region {
        "global" => "api.bfl.ai".into(),
        "us1" => "api.us1.bfl.ai".into(),
        "eu1" => "api.eu1.bfl.ai".into(),
        _ => {
            let region = region
                .strip_prefix('h')
                .filter(|r| label(r))
                .ok_or(Error("invalid_provider_ref"))?;
            format!("api.{region}.bfl.ai")
        }
    };
    Ok((id.into(), format!("https://{host}/v1/get_result?id={id}")))
}

pub fn delivery_url(value: &str) -> Result<Url> {
    let url = safe_url(value, &["*.bfl.ai".into()]).map_err(|_| Error(DELIVERY_REJECTED))?;
    let host = url.host_str().ok_or(Error(DELIVERY_REJECTED))?;
    let approved = host == "delivery.bfl.ai"
        || host
            .strip_prefix("delivery.")
            .and_then(|h| h.strip_suffix(".bfl.ai"))
            .is_some_and(label)
        || host
            .strip_prefix("delivery-")
            .and_then(|h| h.strip_suffix(".bfl.ai"))
            .is_some_and(label);
    need(approved, DELIVERY_REJECTED)?;
    Ok(url)
}

pub fn needs_operator(reason: &str) -> bool {
    matches!(reason, POLLING_REJECTED | DELIVERY_REJECTED)
}

/// BFL's shared result schema includes FLUX 3's reasoning/generation phases.
/// Only Ready permits downloading; moderation and empty results are terminal.
pub fn poll_status(value: &Value) -> Result<&'static str> {
    match value.as_str() {
        Some("Pending" | "Reasoning" | "Generating" | "Running" | "Processing") => {
            Ok("IN_PROGRESS")
        }
        Some("Ready") => Ok("COMPLETED"),
        Some("Request Moderated" | "Content Moderated") => Err(Error("provider_content_rejected")),
        Some("Error" | "Failed" | "Task not found" | "No Card Generated") => {
            Err(Error("provider_generation_failed"))
        }
        _ => Err(Error("invalid_provider_status")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_returned_clusters_and_old_references() {
        for region in [
            "us",
            "eu",
            "us1",
            "eu1",
            "us2",
            "eu2",
            "ap-southeast-1",
            "global",
        ] {
            let url = format!("https://api.{region}.bfl.ai/v1/get_result?id=task_123");
            let bound = submission_reference(&json!({"id":"task_123","polling_url":url})).unwrap();
            assert_eq!(polling_url(&bound).unwrap(), ("task_123".into(), url));
        }
        for region in ["global", "us1", "eu1"] {
            let host = if region == "global" {
                "api.bfl.ai".into()
            } else {
                format!("api.{region}.bfl.ai")
            };
            assert_eq!(
                polling_url(&format!("{region}_task_123")).unwrap().1,
                format!("https://{host}/v1/get_result?id=task_123")
            );
        }
    }
    #[test]
    fn accepted_id_survives_unusable_polling_urls() {
        for url in [
            "https://evil.example/v1/get_result?id=task_123",
            "http://api.us.bfl.ai/v1/get_result?id=task_123",
            "https://api.us.bfl.ai:444/v1/get_result?id=task_123",
            "https://key@api.us.bfl.ai/v1/get_result?id=task_123",
            "https://api.us.bfl.ai/admin?id=task_123",
            "https://api.us.bfl.ai/v1/get_result?id=other",
            "https://api.us.bfl.ai/v1/get_result?id=task_123&key=SECRET",
            "https://api.us.bfl.ai.evil.example/v1/get_result?id=task_123",
            "https://api.bad.cluster.bfl.ai/v1/get_result?id=task_123",
            "https://127.0.0.1/v1/get_result?id=task_123",
        ] {
            let bound = submission_reference(&json!({"id":"task_123","polling_url":url})).unwrap();
            assert_eq!(bound, "unresolved_task_123");
            assert_eq!(polling_url(&bound).unwrap_err().0, POLLING_REJECTED);
        }
        assert_eq!(
            submission_reference(&json!({"id":"task_123"})).unwrap(),
            "unresolved_task_123"
        );
        assert!(submission_reference(&json!({"id":"../../bad"})).is_err());
    }
    #[test]
    fn delivery_is_limited_to_bfl_delivery_hosts() {
        for host in [
            "delivery.bfl.ai",
            "delivery-us1.bfl.ai",
            "delivery-eu1.bfl.ai",
            "delivery.us2.bfl.ai",
            "delivery.eu.bfl.ai",
        ] {
            let url = format!("https://{host}/results/task/image.png?sig=PRIVATE");
            assert_eq!(delivery_url(&url).unwrap().as_str(), url);
        }
        for host in [
            "api.bfl.ai",
            "evil.bfl.ai",
            "delivery.us.bfl.ai.evil.example",
            "delivery.foo.bar.bfl.ai",
            "127.0.0.1",
        ] {
            assert_eq!(
                delivery_url(&format!("https://{host}/image.png"))
                    .unwrap_err()
                    .0,
                DELIVERY_REJECTED
            );
        }
    }

    #[test]
    fn handles_every_published_result_status_without_early_completion() {
        for status in [
            "Pending",
            "Reasoning",
            "Generating",
            "Running",
            "Processing",
        ] {
            assert_eq!(poll_status(&json!(status)).unwrap(), "IN_PROGRESS");
        }
        assert_eq!(poll_status(&json!("Ready")).unwrap(), "COMPLETED");
        for status in ["Request Moderated", "Content Moderated"] {
            assert_eq!(
                poll_status(&json!(status)).unwrap_err().0,
                "provider_content_rejected"
            );
        }
        for status in ["Error", "Failed", "Task not found", "No Card Generated"] {
            assert_eq!(
                poll_status(&json!(status)).unwrap_err().0,
                "provider_generation_failed"
            );
        }
        for value in [json!(null), json!(123), json!("unknown"), json!("ready")] {
            assert_eq!(
                poll_status(&value).unwrap_err().0,
                "invalid_provider_status"
            );
        }
    }
}
