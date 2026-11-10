//! Direct asynchronous video and synchronous image adapters. Submission is
//! never retried; restored video jobs poll their original task ID only.
use crate::{
    Error, Result, asian_media, native_media, native_provider, need,
    network::Request,
    protocol::{decode, digest, id, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};

fn host(provider: &str) -> Result<&'static str> {
    match provider {
        "alibaba" => Ok("dashscope-intl.aliyuncs.com"),
        "byteplus" => Ok("ark.ap-southeast.bytepluses.com"),
        "volcengine" => Ok("ark.cn-beijing.volces.com"),
        _ => Err(Error("unsupported_provider")),
    }
}
pub(crate) fn api_host(host: &str) -> bool {
    matches!(
        host,
        "ark.ap-southeast.bytepluses.com" | "ark.cn-beijing.volces.com"
    ) || host
        .strip_suffix(".ap-southeast-1.maas.aliyuncs.com")
        .is_some_and(|workspace| crate::invitation::valid_workspace(&json!(workspace)))
}
pub(crate) fn http_error(status: u16, bytes: &[u8]) -> Error {
    if (400..500).contains(&status)
        && !matches!(status, 408 | 429)
        && bytes.len() <= 64000
        && let Ok(data) = serde_json::from_slice::<Value>(bytes)
    {
        if !data["id"].is_null() || !data["output"]["task_id"].is_null() {
            return Error("provider_submission_uncertain");
        }
        if let Some(error) = failure(&data) {
            return error;
        }
    }
    crate::diagnostics::http_error(status)
}
fn failure(data: &Value) -> Option<Error> {
    let code = data["error"]["code"]
        .as_str()
        .or_else(|| data["code"].as_str())?;
    Some(Error(match code {
        "InvalidApiKey" | "AuthenticationError" | "InvalidAccessKey" => "provider_auth_rejected",
        "Arrearage" | "InsufficientBalance" | "AccountOverdueError" | "QuotaExhausted" => {
            "provider_billing_required"
        }
        "DataInspectionFailed"
        | "InputTextSensitiveContentDetected"
        | "InputImageSensitiveContentDetected"
        | "OutputVideoSensitiveContentDetected"
        | "OutputImageSensitiveContentDetected" => "provider_content_rejected",
        "Throttling" | "RateLimitExceeded" | "RequestLimitExceeded" => "provider_capacity_limited",
        "InvalidParameter" | "InvalidParameterValue" | "MissingParameter" => {
            "provider_invalid_request"
        }
        _ => "provider_generation_failed",
    }))
}
fn state(data: &Value, model: &Value, expected: &str) -> Result<&'static str> {
    let alibaba = model["provider"] == "alibaba";
    let d = if alibaba { &data["output"] } else { data };
    need(
        d[if alibaba { "task_id" } else { "id" }] == expected,
        "provider_id_mismatch",
    )?;
    if let Some(returned) = data.get("model") {
        need(returned == &model["apiModel"], "provider_model_mismatch")?;
    }
    match d[if alibaba { "task_status" } else { "status" }].as_str() {
        Some("PENDING" | "queued") => Ok("IN_QUEUE"),
        Some("RUNNING" | "running") => Ok("IN_PROGRESS"),
        Some("SUCCEEDED" | "succeeded") if data["error"].is_null() => Ok("COMPLETED"),
        Some("FAILED" | "CANCELED" | "UNKNOWN" | "failed" | "cancelled" | "expired") => {
            Err(failure(data)
                .or_else(|| failure(d))
                .unwrap_or(Error("provider_generation_failed")))
        }
        _ => Err(Error("invalid_provider_status")),
    }
}
impl Router<'_> {
    fn asian_request(&self, model: &Value, path: &str, body: Option<&Value>) -> Result<Request> {
        let provider = s(&model["provider"])?;
        let h = if provider == "alibaba" {
            let workspace = &self.credentials[crate::invitation::ALIBABA_WORKSPACE_KEY];
            need(
                crate::invitation::valid_workspace(workspace),
                "alibaba_workspace_required",
            )?;
            format!("{}.ap-southeast-1.maas.aliyuncs.com", s(workspace)?)
        } else {
            host(provider)?.to_string()
        };
        let mut req = Request::get(format!("https://{h}{path}"), &[&h]);
        req.headers = self.auth(provider)?;
        req.timeout = if model["kind"] == "image" { 600 } else { 60 };
        req.limit = if model["kind"] == "image" {
            128 * 1024 * 1024
        } else {
            1024 * 1024
        };
        if let Some(b) = body {
            req = req.json("POST", b);
            if provider == "alibaba" && model["kind"] == "video" {
                req.headers
                    .push(("X-DashScope-Async".into(), "enable".into()));
            }
            self.guard_submission(&mut req)?;
        }
        Ok(req)
    }
    fn asian_download(&self, url: &str, provider: &str, limit: usize) -> Result<Vec<u8>> {
        let hosts: &[&str] = match provider {
            "alibaba" => &["*.aliyuncs.com"],
            "byteplus" => &["*.bytepluses.com", "*.bytepluscdn.com", "*.ibytedtos.com"],
            "volcengine" => &["*.volces.com", "*.volccdn.com"],
            _ => return Err(Error("unsupported_provider")),
        };
        self.download_bounded(url, hosts, None, &[], limit)
    }
    pub(crate) fn asian_submit(&mut self, spec: &Value, model: &Value) -> Result<String> {
        let urls = self.native_references(spec)?;
        let body = asian_media::body(spec, model, &urls)?;
        if model["kind"] == "image" {
            self.speech = None;
        }
        let data =
            self.http
                .json(self.asian_request(model, s(&model["apiPath"])?, Some(&body))?)?;
        if let Some(e) = failure(&data) {
            need(
                data["id"].is_null() && data["output"]["task_id"].is_null(),
                "invalid_provider_response",
            )?;
            return Err(self.provider_failure(e, s(&model["provider"])?, &data));
        }
        if model["kind"] == "video" {
            let task = if model["provider"] == "alibaba" {
                &data["output"]["task_id"]
            } else {
                &data["id"]
            };
            return native_media::reference(spec, s(task)?);
        }
        if let Some(returned) = data.get("model") {
            need(returned == &model["apiModel"], "provider_model_mismatch")?;
        }
        let images = data["data"].as_array().ok_or(Error("provider_no_media"))?;
        need(images.len() == 1, "provider_output_count_mismatch")?;
        let item = &images[0];
        need(item["error"].is_null(), "provider_generation_failed")?;
        let bytes = if let Some(b64) = item["b64_json"].as_str() {
            need(b64.len() <= 90 * 1024 * 1024, "invalid_media_size")?;
            decode(b64)?
        } else {
            self.asian_download(s(&item["url"])?, s(&model["provider"])?, 64 * 1024 * 1024)?
        };
        let mime = native_provider::mime(&bytes)?;
        let media = native_provider::bundle(
            vec![native_provider::part("image", mime, &bytes)?],
            vec![],
            Value::Null,
        )?;
        let reference = native_media::reference(spec, &format!("sync_{}", id()))?;
        let hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        self.speech = Some((reference.clone(), digest(spec)?, media, hash));
        Ok(reference)
    }
    fn asian_read(&self, reference: &str, spec: &Value, model: &Value) -> Result<Value> {
        let id = native_media::reference_id(spec, reference)?;
        let path = if model["provider"] == "alibaba" {
            format!("/api/v1/tasks/{id}")
        } else {
            format!("/api/v3/contents/generations/tasks/{id}")
        };
        let data = self.http.json(self.asian_request(model, &path, None)?)?;
        state(&data, model, id)?;
        Ok(data)
    }
    pub(crate) fn asian_poll(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<String> {
        native_media::reference_id(spec, reference)?;
        if model["kind"] == "image" {
            need(
                self.speech.as_ref().is_some_and(|(r, h, _, _)| {
                    r == reference && digest(spec).is_ok_and(|v| v == *h)
                }),
                "synchronous_result_unavailable",
            )?;
            return Ok("COMPLETED".into());
        }
        let data = self.asian_read(reference, spec, model)?;
        Ok(state(&data, model, native_media::reference_id(spec, reference)?)?.into())
    }
    pub(crate) fn asian_result(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<Output> {
        if model["kind"] == "image" {
            self.asian_poll(reference, spec, model)?;
            let (_, _, media, response_hash) = self.speech.as_ref().unwrap();
            return Ok(Output {
                media: media.clone(),
                response_hash: response_hash.clone(),
            });
        }
        let data = self.asian_read(reference, spec, model)?;
        need(
            state(&data, model, native_media::reference_id(spec, reference)?)? == "COMPLETED",
            "provider_not_complete",
        )?;
        let url = if model["provider"] == "alibaba" {
            &data["output"]["video_url"]
        } else {
            &data["content"]["video_url"]
        };
        let bytes = self.asian_download(s(url)?, s(&model["provider"])?, 64 * 1024 * 1024)?;
        let media = native_provider::bundle(
            vec![native_provider::part("video", "video/mp4", &bytes)?],
            vec![],
            Value::Null,
        )?;
        let response_hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        Ok(Output {
            media,
            response_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn error_codes_are_scoped_and_ambiguous_requests_are_not_classified_as_free() {
        assert!(api_host("workspace-1.ap-southeast-1.maas.aliyuncs.com"));
        assert!(!api_host(
            "workspace-1.ap-southeast-1.maas.aliyuncs.com.attacker.com"
        ));
        assert!(!api_host("evil.foo.ap-southeast-1.maas.aliyuncs.com"));
        let body = br#"{"error":{"code":"InsufficientBalance","message":"private"}}"#;
        assert_eq!(http_error(400, body).0, "provider_billing_required");
        assert_eq!(http_error(503, body).0, "http_503");
        assert_eq!(http_error(408, body).0, "http_408");
        assert_eq!(http_error(429, body).0, "http_429");
        assert_eq!(
            http_error(
                400,
                br#"{"id":"maybe-paid","error":{"code":"InsufficientBalance"}}"#
            )
            .0,
            "provider_submission_uncertain"
        );
    }
}
