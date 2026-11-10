//! Reviewed WaveSpeedAI media transport. Task IDs are bound to the signed input;
//! no submission retry, arbitrary callback, provider URL, or remote schema loading.
use crate::{
    Error, Result, native_media, native_provider, need,
    protocol::{digest, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(crate) struct Cached {
    reference: String,
    at: Instant,
    data: Result<Value>,
}

pub fn error(code: i64) -> Error {
    Error(match code {
        401 => "wavespeed_auth_rejected",
        402 | 1407 => "wavespeed_billing_required",
        429 => "wavespeed_capacity_limited",
        1200 => "wavespeed_content_rejected",
        400 | 1400..=1402 | 4005..=4007 => "wavespeed_invalid_parameters",
        500 | 5000 | 5003 | 5004 => "wavespeed_unavailable",
        _ => "wavespeed_generation_failed",
    })
}

pub fn http_error(status: u16, bytes: &[u8]) -> Error {
    // A task ID means the provider may have started work, regardless of status.
    if bytes.len() <= 64000
        && let Ok(data) = serde_json::from_slice::<Value>(bytes)
        && (!data["data"]["id"].is_null() || !data["id"].is_null())
    {
        return Error("invalid_provider_response");
    }
    // Never turn an ambiguous HTTP response into evidence of non-submission.
    if (400..500).contains(&status)
        && !matches!(status, 408 | 429)
        && bytes.len() <= 64000
        && let Ok(data) = serde_json::from_slice::<Value>(bytes)
        && data["data"]["id"].is_null()
        && data["id"].is_null()
        && let Some(code) = data["code"].as_i64()
        && matches!(code, 401 | 402 | 1200 | 1400..=1402 | 1407 | 4005..=4007)
    {
        return error(code);
    }
    crate::diagnostics::http_error(status)
}

pub fn reference_policy(raw: &Value, spec: &Value, model: &Value) -> Result<()> {
    if let Some(size) = raw.get("size") {
        let (width, height) = s(size)?
            .split_once('*')
            .ok_or(Error("invalid_native_image_size"))?;
        let bounds = &model["nativeSchema"]["properties"]["size"];
        for value in [width, height] {
            let number: u64 = value
                .parse()
                .map_err(|_| Error("invalid_native_image_size"))?;
            need(
                number >= bounds["minimum"].as_u64().unwrap_or(1)
                    && number <= bounds["maximum"].as_u64().unwrap_or(16384),
                "invalid_native_image_size",
            )?;
        }
    }
    let refs = spec["input"]["references"]
        .as_array()
        .ok_or(Error("invalid_native_media_references"))?;
    need(
        refs.len() as u64
            <= model["nativeCapabilities"]["maxReferences"]
                .as_u64()
                .unwrap_or(0),
        "invalid_native_media_references",
    )?;
    for r in refs.iter().filter(|r| r["role"] != "audio") {
        need(
            r["width"].as_u64().is_some_and(|n| n > 0 && n <= 16384)
                && r["height"].as_u64().is_some_and(|n| n > 0 && n <= 16384),
            "invalid_reference_dimensions",
        )?;
    }
    for (field, role) in model["referenceRoles"]
        .as_object()
        .ok_or(Error("invalid_native_schema"))?
    {
        let name = field.trim_start_matches('/').split('/').next().unwrap();
        let values: Vec<&Value> = match raw.get(name) {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(a)) => a.iter().collect(),
            Some(value) => vec![value],
        };
        for value in values {
            let index: usize = s(value)?
                .strip_prefix("@reference:")
                .ok_or(Error("unverified_native_reference"))?
                .parse()
                .map_err(|_| Error("invalid_native_reference_slot"))?;
            let r = refs
                .get(index)
                .ok_or(Error("invalid_native_reference_slot"))?;
            need(r["role"] == *role, "invalid_reference_role")?;
            if role == "video" || role == "audio" {
                need(
                    r["durationMs"]
                        .as_u64()
                        .is_some_and(|v| v > 0 && v <= 30000),
                    "invalid_reference_duration",
                )?;
            }
        }
    }
    if raw.get("last_image").is_some() || raw.get("keyframes").is_some() {
        need(
            raw.get("image").is_some(),
            "invalid_native_media_references",
        )?;
    }
    if model["apiModel"]
        .as_str()
        .is_some_and(|id| id.starts_with("kwaivgi/kling-v4.0/"))
    {
        let frames = usize::from(raw.get("image").is_some())
            + usize::from(raw.get("last_image").is_some())
            + raw["keyframes"].as_array().map_or(0, Vec::len);
        need(frames <= 10, "invalid_native_media_references")?;
    }
    need(
        !(raw.get("mask_image").is_some() && raw.get("markup_image").is_some()),
        "invalid_native_media_references",
    )?;
    Ok(())
}

impl Router<'_> {
    fn wavespeed_response(
        &self,
        value: Value,
        model: &Value,
        expected: Option<&str>,
    ) -> Result<Value> {
        let data = &value["data"];
        if data["id"].is_string() {
            let id = s(&data["id"])?;
            need(
                crate::matches("[A-Za-z0-9_-]{1,100}", &json!(id)),
                "invalid_provider_ref",
            )?;
            if let Some(expected) = expected {
                need(id == expected, "provider_id_mismatch")?;
            }
            need(
                data["model"] == model["apiModel"],
                "provider_model_mismatch",
            )?;
            need(
                data["status"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty() && v.len() <= 80),
                "invalid_provider_status",
            )?;
            return Ok(data.clone());
        }
        if value["code"] != 200
            && let Some(code) = value["code"].as_i64()
        {
            let e = if matches!(code, 401 | 402 | 429 | 1200 | 1400..=1402 | 1407 | 4005..=4007 | 5000 | 5003 | 5004)
            {
                error(code)
            } else {
                Error("invalid_provider_response")
            };
            return Err(self.provider_failure(e, "wavespeed", &value));
        }
        Err(Error("invalid_provider_response"))
    }
    pub(crate) fn wavespeed_submit(
        &mut self,
        spec: &Value,
        model: &Value,
        body: &Value,
    ) -> Result<String> {
        let mut request = self.native_request(model, s(&model["apiPath"])?, Some(body))?;
        request.timeout = 60;
        request.limit = 512 * 1024;
        let data = self.wavespeed_response(self.http.json(request)?, model, None)?;
        let reference = native_media::reference(spec, s(&data["id"])?)?;
        self.wavespeed_cache.replace(Some(Cached {
            reference: reference.clone(),
            at: Instant::now(),
            data: Ok(data),
        }));
        Ok(reference)
    }
    fn wavespeed_read(&self, reference: &str, spec: &Value, model: &Value) -> Result<Value> {
        let id = native_media::reference_id(spec, reference)?;
        if let Some(cached) = self.wavespeed_cache.borrow().as_ref()
            && cached.reference == reference
            && cached.at.elapsed() < Duration::from_secs(2)
        {
            return cached.data.clone();
        }
        let mut request =
            self.native_request(model, &format!("/api/v3/predictions/{id}/result"), None)?;
        request.timeout = 30;
        request.limit = 512 * 1024;
        let data = self
            .http
            .json(request)
            .and_then(|value| self.wavespeed_response(value, model, Some(id)));
        self.wavespeed_cache.replace(Some(Cached {
            reference: reference.into(),
            at: Instant::now(),
            data: data.clone(),
        }));
        data
    }
    pub(crate) fn wavespeed_poll(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<String> {
        let data = self.wavespeed_read(reference, spec, model)?;
        match data["status"].as_str() {
            Some("completed") => Ok("COMPLETED".into()),
            Some("failed" | "cancelled" | "timeout" | "deleted") => {
                let code = error(data["code"].as_i64().unwrap_or(1403));
                // An explicit terminal task state must not enter the transient poll loop.
                let code = if crate::submission::transient_poll(code.0) {
                    Error("wavespeed_generation_failed")
                } else {
                    code
                };
                Err(self.provider_failure(code, "wavespeed", &data))
            }
            Some("created" | "queued") => Ok("IN_QUEUE".into()),
            _ => Ok("IN_PROGRESS".into()),
        }
    }
    pub(crate) fn wavespeed_result(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<Output> {
        let data = self.wavespeed_read(reference, spec, model)?;
        need(data["status"] == "completed", "provider_not_complete")?;
        let outputs = data["outputs"]
            .as_array()
            .ok_or(Error("provider_no_media"))?;
        need(outputs.len() == 1, "provider_no_media")?;
        let url = outputs[0]
            .as_str()
            .or_else(|| outputs[0]["url"].as_str())
            .ok_or(Error("provider_no_media"))?;
        let bytes = self.download_bounded(
            url,
            &["static.wavespeed.ai", "cdn.wavespeed.ai"],
            None,
            &[],
            80 * 1024 * 1024,
        )?;
        let kind = s(&model["kind"])?;
        let mime = match kind {
            "image" => {
                let format = spec["input"]["parameters"]["output_format"]
                    .as_str()
                    .unwrap_or("jpeg");
                if format == "gif" {
                    need(
                        bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a"),
                        "invalid_provider_media",
                    )?;
                    "image/gif"
                } else if format == "svg" {
                    let text = std::str::from_utf8(&bytes)
                        .map_err(|_| Error("invalid_provider_media"))?
                        .trim_start();
                    need(
                        text.starts_with("<svg") || text.starts_with("<?xml"),
                        "invalid_provider_media",
                    )?;
                    "image/svg+xml"
                } else {
                    native_provider::mime(&bytes)?
                }
            }
            "video" => {
                need(bytes.get(4..8) == Some(b"ftyp"), "invalid_provider_media")?;
                "video/mp4"
            }
            "audio" => match spec["input"]["parameters"]["format"]
                .as_str()
                .unwrap_or("mp3")
            {
                "mp3" => "audio/mpeg",
                "wav" => "audio/wav",
                "flac" => "audio/flac",
                "pcm" => "audio/l16",
                _ => return Err(Error("invalid_provider_media")),
            },
            _ => return Err(Error("invalid_provider_media")),
        };
        let media = native_provider::bundle(
            vec![native_provider::part(kind, mime, &bytes)?],
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
