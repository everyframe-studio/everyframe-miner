//! Native media transports. A submission is never retried here. Deferred xAI
//! jobs resume by GET; synchronous outputs are bound to the original input and
//! fail closed if the process loses them, as with the existing speech adapter.
use crate::{
    Error, Result, native_media, need,
    network::Request,
    protocol::{b64, decode, digest, id, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};
const OUTPUT_LIMIT: usize = 128 * 1024 * 1024;
const JSON_LIMIT: usize = OUTPUT_LIMIT * 4 / 3 + 1024 * 1024;

fn origin(model: &Value) -> Result<&'static str> {
    match s(&model["provider"])? {
        "ltx" => Ok("api.ltx.io"),
        "openai" => Ok("api.openai.com"),
        "openrouter" => Ok("openrouter.ai"),
        "fal" => Ok("queue.fal.run"),
        "xai" => Ok("api.x.ai"),
        "bfl" => Ok("api.bfl.ai"),
        "minimax" => Ok("api.minimax.io"),
        "luma-agents" => Ok("agents.lumalabs.ai"),
        "kling" => Ok("api-singapore.klingai.com"),
        "runway" => Ok("api.dev.runwayml.com"),
        "elevenlabs" => Ok("api.elevenlabs.io"),
        "wavespeed" => Ok("api.wavespeed.ai"),
        _ => Err(Error("unsupported_provider")),
    }
}
pub(crate) fn mime(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Ok("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok("image/webp")
    } else {
        Err(Error("invalid_provider_media"))
    }
}
fn audio_mime(spec: &Value, model: &Value) -> Result<&'static str> {
    let p = &spec["input"]["parameters"];
    let format = if model["adapter"] == "xai-tts" {
        &p["output_format"]["codec"]
    } else {
        &p["audio"]["format"]
    };
    match s(format)? {
        "mp3" => Ok("audio/mpeg"),
        "wav" => Ok("audio/wav"),
        "flac" => Ok("audio/flac"),
        "aac" => Ok("audio/aac"),
        "opus" => Ok("audio/ogg"),
        "pcm16" | "pcm" => Ok("audio/l16"),
        "alaw" => Ok("audio/alaw"),
        "mulaw" => Ok("audio/mulaw"),
        _ => Err(Error("invalid_native_audio_format")),
    }
}
pub(crate) fn bundle(parts: Vec<Value>, text: Vec<String>, timestamps: Value) -> Result<Vec<u8>> {
    need(!parts.is_empty() && parts.len() <= 10, "provider_no_media")?;
    let data = serde_json::to_vec(&json!({"kind":"native-media-bundle","version":1,"parts":parts,"text":text,"timestamps":timestamps}))
        .map_err(|_| Error("invalid_provider_media"))?;
    need(data.len() <= OUTPUT_LIMIT, "invalid_media_size")?;
    Ok(data)
}
pub(crate) fn part(kind: &str, mime: &str, data: &[u8]) -> Result<Value> {
    need(
        !data.is_empty() && data.len() <= OUTPUT_LIMIT,
        "invalid_media_size",
    )?;
    Ok(json!({"type":kind,"mime":mime,"data":b64(data)}))
}
fn inline(value: &Value) -> Result<Vec<u8>> {
    let encoded = s(value)?;
    need(encoded.len() <= JSON_LIMIT, "invalid_media_size")?;
    let bytes = decode(encoded)?;
    need(
        !bytes.is_empty() && bytes.len() <= OUTPUT_LIMIT,
        "invalid_media_size",
    )?;
    Ok(bytes)
}
impl Router<'_> {
    pub(crate) fn native_request(
        &self,
        model: &Value,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Request> {
        let host = origin(model)?;
        let mut request = Request::get(format!("https://{host}{path}"), &[host]);
        request.headers = self.auth(s(&model["provider"])?)?;
        request.limit = JSON_LIMIT;
        request.timeout = if body.is_some() { 600 } else { 60 };
        if let Some(body) = body {
            request = request.json("POST", body);
            self.guard_submission(&mut request)?;
        }
        Ok(request)
    }
    pub(crate) fn native_references(&self, spec: &Value) -> Result<Vec<String>> {
        let mut urls = vec![];
        for reference in spec["input"]["references"].as_array().unwrap() {
            let mut request = Request::get(
                s(&reference["url"])?,
                &["api.everyframe.studio", "app.everyframe.studio"],
            );
            request.limit = reference["bytes"]
                .as_u64()
                .ok_or(Error("invalid_reference_size"))? as usize;
            request.timeout = 180;
            let bytes = self.http.bytes(request)?;
            need(
                bytes.len() as u64 == reference["bytes"].as_u64().unwrap()
                    && sha(&bytes) == s(&reference["sha256"])?,
                "reference_integrity_mismatch",
            )?;
            let claimed = s(&reference["mime"])?;
            if ["image/heic", "image/heif"].contains(&claimed) {
                need(
                    bytes.get(4..8) == Some(b"ftyp"),
                    "reference_integrity_mismatch",
                )?;
            } else if claimed.starts_with("image/") {
                need(mime(&bytes)? == claimed, "reference_integrity_mismatch")?;
            }
            // Forward exactly the content-addressed bytes, never a mutable URL
            // or caller-supplied provider file ID.
            let model = native_media::info(s(&spec["model"])?)?;
            urls.push(if model["referenceTransport"] == "owned-url" {
                s(&reference["url"])?.to_string()
            } else {
                format!("data:{claimed};base64,{}", b64(&bytes))
            });
        }
        Ok(urls)
    }
    pub(crate) fn native_submit(&mut self, spec: &Value, model: &Value) -> Result<String> {
        if model["asianMedia"] == true {
            return self.asian_submit(spec, model);
        }
        if model["h3Image"] == true {
            return self.h3_submit(spec, model);
        }
        if model["gptImage26"] == true {
            return self.gpt26_submit(spec, model);
        }
        let urls = self.native_references(spec)?;
        let (path, body) = native_media::body(spec, &urls)?;
        if model["nativeSchema"].is_object() {
            return self.schema_submit(spec, model, &body);
        }
        let request = self.native_request(model, &path, Some(&body))?;
        let adapter = s(&model["adapter"])?;
        if ["xai-images", "xai-videos"].contains(&adapter) {
            let data = self.http.json(request)?;
            need(data["error"].is_null(), "provider_rejected")?;
            return native_media::reference(spec, s(&data["request_id"])?);
        }
        self.speech = None;
        let mut parts = vec![];
        let mut text = vec![];
        let mut timestamps = Value::Null;
        if adapter == "xai-tts" && spec["input"]["parameters"]["with_timestamps"] != true {
            let bytes = self.http.bytes(request)?;
            parts.push(part("audio", audio_mime(spec, model)?, &bytes)?);
        } else {
            let data = self.http.json(request)?;
            need(data["error"].is_null(), "provider_rejected")?;
            if adapter == "openai-images" {
                let images = data["data"].as_array().ok_or(Error("provider_no_media"))?;
                need(
                    images.len() as u64 == spec["input"]["parameters"]["n"].as_u64().unwrap(),
                    "provider_no_media",
                )?;
                for image in images {
                    let bytes = inline(&image["b64_json"])?;
                    let expected = format!(
                        "image/{}",
                        s(&spec["input"]["parameters"]["output_format"])?
                    );
                    need(mime(&bytes)? == expected, "invalid_provider_media")?;
                    parts.push(part("image", &expected, &bytes)?);
                    if let Some(prompt) = image["revised_prompt"].as_str() {
                        text.push(prompt.to_string());
                    }
                }
            } else if adapter == "openai-audio" {
                need(
                    data["model"] == model["apiModel"]
                        || data["model"].as_str().is_some_and(|v| {
                            v.starts_with(&format!("{}-", model["apiModel"].as_str().unwrap()))
                        }),
                    "provider_model_mismatch",
                )?;
                let choices = data["choices"]
                    .as_array()
                    .ok_or(Error("provider_no_media"))?;
                need(
                    choices.len() == 1 && choices[0]["message"]["refusal"].is_null(),
                    "provider_no_media",
                )?;
                let audio = &choices[0]["message"]["audio"];
                let bytes = inline(&audio["data"])?;
                let bytes = if audio_mime(spec, model)? == "audio/wav" {
                    crate::wav::normalize_openai_wav(bytes)?
                } else {
                    bytes
                };
                parts.push(part("audio", audio_mime(spec, model)?, &bytes)?);
                if let Some(transcript) = audio["transcript"].as_str() {
                    text.push(transcript.to_string());
                }
            } else {
                parts.push(part(
                    "audio",
                    audio_mime(spec, model)?,
                    &inline(&data["audio"])?,
                )?);
                timestamps = data["audio_timestamps"].clone();
                need(!timestamps.is_null(), "invalid_provider_media")?;
            }
        }
        let media = bundle(parts, text, timestamps)?;
        let reference = native_media::reference(spec, &format!("sync_{}", id()))?;
        let hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        self.speech = Some((reference.clone(), digest(spec)?, media, hash));
        Ok(reference)
    }
    fn native_read(&self, reference: &str, spec: &Value, model: &Value) -> Result<Value> {
        let id = native_media::reference_id(spec, reference)?;
        let kind = match s(&model["adapter"])? {
            "xai-images" => "images",
            "xai-videos" => "videos",
            _ => return Err(Error("provider_ref_namespace")),
        };
        let data =
            self.http
                .json(self.native_request(model, &format!("/v1/{kind}/{id}"), None)?)?;
        native_media::state(spec, id, &data)?;
        Ok(data)
    }
    pub(crate) fn native_poll(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<String> {
        if model["asianMedia"] == true {
            return self.asian_poll(reference, spec, model);
        }
        if model["h3Image"] == true {
            return self.h3_poll(reference, spec, model);
        }
        if model["gptImage26"] == true {
            return self.gpt26_poll(reference, spec, model);
        }
        if model["nativeSchema"].is_object() {
            return self.schema_poll(reference, spec, model);
        }
        let id = native_media::reference_id(spec, reference)?;
        if ["openai-images", "openai-audio", "xai-tts"].contains(&s(&model["adapter"])?) {
            need(
                id.starts_with("sync_")
                    && self.speech.as_ref().is_some_and(|(r, h, _, _)| {
                        r == reference && digest(spec).is_ok_and(|v| v == *h)
                    }),
                "synchronous_result_unavailable",
            )?;
            return Ok("COMPLETED".into());
        }
        let data = self.native_read(reference, spec, model)?;
        let state = native_media::state(spec, id, &data)?;
        if state == "FAILED" {
            return Err(self.provider_failure(
                Error("provider_generation_failed"),
                s(&model["provider"])?,
                &data,
            ));
        }
        Ok(state.into())
    }
    pub(crate) fn native_result(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<Output> {
        if model["asianMedia"] == true {
            return self.asian_result(reference, spec, model);
        }
        if model["h3Image"] == true {
            return self.h3_result(reference, spec, model);
        }
        if model["gptImage26"] == true {
            return self.gpt26_result(reference, spec, model);
        }
        if model["nativeSchema"].is_object() {
            return self.schema_result(reference, spec, model);
        }
        if ["openai-images", "openai-audio", "xai-tts"].contains(&s(&model["adapter"])?) {
            self.native_poll(reference, spec, model)?;
            let (_, _, media, hash) = self.speech.as_ref().unwrap();
            return Ok(Output {
                media: media.clone(),
                response_hash: hash.clone(),
            });
        }
        let data = self.native_read(reference, spec, model)?;
        need(
            native_media::state(spec, native_media::reference_id(spec, reference)?, &data)?
                == "COMPLETED",
            "provider_not_complete",
        )?;
        let mut parts = vec![];
        let mut text = vec![];
        if model["kind"] == "image" {
            let images = data["data"].as_array().ok_or(Error("provider_no_media"))?;
            need(
                images.len() as u64 == spec["input"]["parameters"]["n"].as_u64().unwrap(),
                "provider_no_media",
            )?;
            for image in images {
                let bytes = self.download_bounded(
                    s(&model["provider"])?,
                    s(&image["url"])?,
                    &["imgen.x.ai"],
                    None,
                    &[],
                    20 * 1024 * 1024,
                )?;
                let mime = mime(&bytes)?;
                if let Some(claimed) = image["mime_type"].as_str() {
                    need(claimed == mime, "invalid_provider_media")?;
                }
                parts.push(part("image", mime, &bytes)?);
                if let Some(prompt) = image["revised_prompt"].as_str() {
                    text.push(prompt.to_string());
                }
            }
        } else {
            let bytes = self.download_bounded(
                s(&model["provider"])?,
                s(&data["video"]["url"])?,
                &["vidgen.x.ai"],
                None,
                &[],
                80 * 1024 * 1024,
            )?;
            parts.push(part("video", "video/mp4", &bytes)?);
        }
        let media = bundle(parts, text, Value::Null)?;
        let response_hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        Ok(Output {
            media,
            response_hash,
        })
    }
}
