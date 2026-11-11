//! Buffered OpenAI/OpenRouter output and resumable Fal queue output. No POST retries.
use crate::{
    Error, Result, gpt_image26, native_media,
    native_provider::{bundle, mime, part},
    need,
    protocol::{decode, digest, id, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};

fn image_mime(bytes: &[u8], format: &str) -> Result<&'static str> {
    if format == "gif" {
        need(
            bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a"),
            "invalid_provider_media",
        )?;
        Ok("image/gif")
    } else if format == "svg" {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error("invalid_provider_media"))?
            .trim_start_matches('\u{feff}')
            .trim_start();
        need(
            text.starts_with("<svg") || text.starts_with("<?xml"),
            "invalid_provider_media",
        )?;
        Ok("image/svg+xml")
    } else {
        let actual = mime(bytes)?;
        need(
            actual == format!("image/{format}"),
            "invalid_provider_media",
        )?;
        Ok(actual)
    }
}
impl Router<'_> {
    fn gpt26_queue_path(&self, spec: &Value, m: &Value, reference: &str) -> Result<String> {
        let refs = spec["input"]["references"].as_array().unwrap();
        let (path, _) = gpt_image26::body(spec, m, &vec![String::new(); refs.len()])?;
        // Fal queue reads use the owner/app root, excluding endpoint subpaths.
        let app = path
            .trim_start_matches('/')
            .split('/')
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
        Ok(format!(
            "/{app}/requests/{}",
            native_media::reference_id(spec, reference)?
        ))
    }
    pub(crate) fn gpt26_submit(&mut self, spec: &Value, m: &Value) -> Result<String> {
        let urls = self.native_references(spec)?;
        let (path, body) = gpt_image26::body(spec, m, &urls)?;
        let data = self
            .http
            .json(self.native_request(m, &path, Some(&body))?)?;
        if !data["error"].is_null() {
            return Err(self.provider_failure(
                Error("provider_rejected"),
                s(&m["provider"])?,
                &data,
            ));
        }
        if m["provider"] == "fal" {
            return native_media::reference(spec, s(&data["request_id"])?);
        }
        self.speech = None;
        let media = self.gpt26_images(spec, m, &data)?;
        let reference = native_media::reference(spec, &format!("sync_{}", id()))?;
        let hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        self.speech = Some((reference.clone(), digest(spec)?, media, hash));
        Ok(reference)
    }
    fn gpt26_images(&self, spec: &Value, m: &Value, data: &Value) -> Result<Vec<u8>> {
        let fal = m["provider"] == "fal";
        let images = data[if fal { "images" } else { "data" }]
            .as_array()
            .ok_or(Error("provider_no_media"))?;
        need(
            images.len() as u64 == spec["input"]["parameters"]["n"].as_u64().unwrap(),
            "provider_no_media",
        )?;
        let mut parts = vec![];
        for image in images {
            let bytes = if fal {
                // Credentials belong only on the queue host, never delivery hosts.
                self.download_bounded(
                    s(&image["url"])?,
                    &[
                        "fal.media",
                        "v3.fal.media",
                        "v3b.fal.media",
                        "v3c.fal.media",
                        "v3d.fal.media",
                    ],
                    None,
                    &[],
                    64 * 1024 * 1024,
                )?
            } else {
                decode(s(&image["b64_json"])?)?
            };
            let actual = image_mime(&bytes, s(&spec["input"]["parameters"]["output_format"])?)?;
            if let Some(claimed) = image[if fal { "content_type" } else { "media_type" }].as_str() {
                need(claimed == actual, "invalid_provider_media")?;
            }
            parts.push(part("image", actual, &bytes)?);
        }
        bundle(parts, vec![], Value::Null)
    }
    pub(crate) fn gpt26_poll(&self, reference: &str, spec: &Value, m: &Value) -> Result<String> {
        if m["provider"] != "fal" {
            let id = native_media::reference_id(spec, reference)?;
            need(
                id.starts_with("sync_")
                    && self.speech.as_ref().is_some_and(|(r, h, _, _)| {
                        r == reference && digest(spec).is_ok_and(|v| v == *h)
                    }),
                "synchronous_result_unavailable",
            )?;
            return Ok("COMPLETED".into());
        }
        let path = self.gpt26_queue_path(spec, m, reference)?;
        let data = self
            .http
            .json(self.native_request(m, &format!("{path}/status"), None)?)?;
        if let Some(returned) = data.get("request_id") {
            need(
                returned == native_media::reference_id(spec, reference)?,
                "provider_id_mismatch",
            )?;
        }
        match data["status"].as_str() {
            Some("IN_QUEUE" | "IN_PROGRESS") => Ok("IN_PROGRESS".into()),
            Some("COMPLETED") => Ok("COMPLETED".into()),
            Some("FAILED") => {
                Err(self.provider_failure(Error("provider_generation_failed"), "fal", &data))
            }
            _ => Err(Error("invalid_provider_status")),
        }
    }
    pub(crate) fn gpt26_result(&self, reference: &str, spec: &Value, m: &Value) -> Result<Output> {
        need(
            self.gpt26_poll(reference, spec, m)? == "COMPLETED",
            "provider_not_complete",
        )?;
        if m["provider"] != "fal" {
            let (_, _, media, hash) = self.speech.as_ref().unwrap();
            return Ok(Output {
                media: media.clone(),
                response_hash: hash.clone(),
            });
        }
        let path = self.gpt26_queue_path(spec, m, reference)?;
        let data = self.http.json(self.native_request(m, &path, None)?)?;
        if !data["error"].is_null() {
            return Err(self.provider_failure(Error("provider_generation_failed"), "fal", &data));
        }
        let media = self.gpt26_images(spec, m, &data)?;
        let response_hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        Ok(Output {
            media,
            response_hash,
        })
    }
}
