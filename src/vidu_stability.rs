//! Direct Vidu and Stability AI transports for the reviewed native contracts.
use crate::{
    Error, Result, native_media,
    native_provider::{bundle, mime, part},
    need,
    protocol::{decode, digest, id, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};

const VIDU_MEDIA: &[&str] = &["prod-ss-vidu.s3.cn-northwest-1.amazonaws.com.cn"];
const MEDIA_LIMIT: usize = 94 * 1024 * 1024;

pub(crate) fn check_vidu_references(spec: &Value, model: &Value) -> Result<()> {
    let Some(roles) = model["referenceRoles"].as_array() else {
        return Ok(());
    };
    let refs = spec["input"]["references"].as_array().unwrap();
    let slots = spec["input"]["parameters"]["images"]
        .as_array()
        .ok_or(Error("invalid_native_media_references"))?;
    need(
        refs.len() == roles.len()
            && slots.len() == roles.len()
            && slots.iter().zip(roles).all(|(slot, role)| {
                slot.as_str()
                    .and_then(|s| s.strip_prefix("@reference:"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .and_then(|index| refs.get(index))
                    .is_some_and(|r| {
                        r["role"] == *role && r["bytes"].as_u64().is_some_and(|n| n <= 50_000_000)
                    })
            }),
        "invalid_native_media_references",
    )
}

impl Router<'_> {
    pub(crate) fn vidu_submit(&self, spec: &Value, model: &Value, body: &Value) -> Result<String> {
        let response =
            self.http
                .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
        native_media::reference(spec, s(&response["task_id"])?)
    }

    fn vidu_creation(&self, reference: &str, spec: &Value, model: &Value) -> Result<Value> {
        let id = native_media::reference_id(spec, reference)?;
        let path = format!("/ent/v2/tasks/{id}/creations");
        let response = self.http.json(self.native_request(model, &path, None)?)?;
        need(response["id"] == id, "provider_id_mismatch")?;
        if response["state"] == "failed" {
            return Err(self.provider_failure(
                Error("provider_generation_failed"),
                "vidu",
                &response,
            ));
        }
        Ok(response)
    }

    pub(crate) fn vidu_poll(&self, reference: &str, spec: &Value, model: &Value) -> Result<String> {
        let response = self.vidu_creation(reference, spec, model)?;
        match response["state"].as_str() {
            Some("created" | "queueing" | "processing") => Ok("IN_PROGRESS".into()),
            Some("success") => Ok("COMPLETED".into()),
            _ => Err(Error("invalid_provider_status")),
        }
    }

    pub(crate) fn vidu_result(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<Output> {
        let response = self.vidu_creation(reference, spec, model)?;
        need(response["state"] == "success", "provider_result_not_ready")?;
        let creations = response["creations"]
            .as_array()
            .ok_or(Error("provider_no_media"))?;
        need(creations.len() == 1, "invalid_provider_media")?;
        let media = self.download_bounded(
            "vidu",
            s(&creations[0]["url"])?,
            VIDU_MEDIA,
            None,
            &[],
            MEDIA_LIMIT,
        )?;
        need(media.get(4..8) == Some(b"ftyp"), "invalid_provider_media")?;
        let media = bundle(
            vec![part("video", "video/mp4", &media)?],
            vec![],
            Value::Null,
        )?;
        let response_hash = result_hash(spec, reference, &media)?;
        Ok(Output {
            media,
            response_hash,
        })
    }

    pub(crate) fn stability_submit(
        &mut self,
        spec: &Value,
        model: &Value,
        body: &Value,
    ) -> Result<String> {
        self.speech = None;
        let boundary = format!("EveryFrame{}", id());
        let form = body
            .as_object()
            .ok_or(Error("invalid_native_media_fields"))?;
        let mut data = Vec::new();
        for (name, value) in form {
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            data.extend_from_slice(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ).as_bytes());
        }
        data.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let mut req = self.native_request(model, s(&model["apiPath"])?, None)?;
        req.method = "POST".into();
        req.timeout = 540;
        req.body = Some(data);
        req.headers.push((
            "content-type".into(),
            format!("multipart/form-data; boundary={boundary}"),
        ));
        let image = model["kind"] == "image";
        req.headers.push((
            "accept".into(),
            if image { "application/json" } else { "audio/*" }.into(),
        ));
        self.guard_submission(&mut req)?;
        let format = s(&body["output_format"])?;
        let (bytes, kind, output_mime) = if image {
            let response = self.http.json(req)?;
            if response["finish_reason"] == "CONTENT_FILTERED" {
                return Err(Error("provider_content_rejected"));
            }
            let bytes = decode(s(&response["image"])?)?;
            let kind = mime(&bytes)?;
            need(kind == format!("image/{format}"), "invalid_provider_media")?;
            (bytes, "image", kind)
        } else {
            req.limit = MEDIA_LIMIT;
            let bytes = self.http.bytes(req)?;
            let output_mime = match format {
                "wav" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") => {
                    "audio/wav"
                }
                "mp3"
                    if bytes.starts_with(b"ID3")
                        || (bytes.first() == Some(&0xff)
                            && bytes.get(1).is_some_and(|byte| byte & 0xe0 == 0xe0)) =>
                {
                    "audio/mpeg"
                }
                _ => return Err(Error("invalid_provider_media")),
            };
            (bytes, "audio", output_mime)
        };
        let media = bundle(vec![part(kind, output_mime, &bytes)?], vec![], Value::Null)?;
        let reference = native_media::reference(spec, &format!("sync_{}", id()))?;
        let hash = result_hash(spec, &reference, &media)?;
        self.speech = Some((reference.clone(), digest(spec)?, media, hash));
        Ok(reference)
    }
}

fn result_hash(spec: &Value, reference: &str, bytes: &[u8]) -> Result<String> {
    digest(&json!({"inputHash":digest(spec)?,"reference":reference,"outputHash":sha(bytes)}))
}
