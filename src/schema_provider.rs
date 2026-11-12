//! Transports for schema-reviewed native APIs. Provider references are bound to
//! the request digest; polling and artifact reads never replay a submission.
use crate::{
    Error, Result, native_media, native_provider, need,
    network::Request,
    protocol::{digest, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};
impl Router<'_> {
    pub(crate) fn schema_submit(
        &mut self,
        spec: &Value,
        model: &Value,
        body: &Value,
    ) -> Result<String> {
        if model["ltx25"] == true && model["provider"] != "wavespeed" {
            return self.ltx25_submit(spec, model, body);
        }
        if model["provider"] == "wavespeed" {
            return self.wavespeed_submit(spec, model, body);
        }
        if model["provider"] == "runway" {
            let data =
                self.http
                    .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
            return native_media::reference(spec, s(&data["id"])?);
        }
        if model["provider"] == "luma-agents" {
            let data =
                self.http
                    .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
            return native_media::reference(spec, s(&data["id"])?);
        }
        if model["provider"] == "elevenlabs" {
            self.speech = None;
            let mut path = s(&model["apiPath"])?.to_string();
            if path.contains("{voice_id}") {
                path = path.replace("{voice_id}", s(&body["voice_id"])?);
            }
            let format = s(&body["output_format"])?;
            path.push_str(&format!("?output_format={format}"));
            if let Some(log) = body["enable_logging"].as_bool() {
                path.push_str(if log {
                    "&enable_logging=true"
                } else {
                    "&enable_logging=false"
                });
            }
            let bytes = if model["adapter"] == "schema-elevenlabs-stream" {
                let path = format!("{}&model_id={}", path, s(&model["apiModel"])?);
                let mut request = self.native_request(model, &path, None)?;
                self.guard_submission(&mut request)?;
                request.timeout = 600;
                let inputs = body["request"]["inputs"]
                    .as_array()
                    .ok_or(Error("invalid_native_media_fields"))?;
                let voices = inputs
                    .iter()
                    .map(|x| s(&x["voice_id"]))
                    .collect::<Result<std::collections::BTreeSet<_>>>()?;
                self.http
                    .dialogue(request, &json!(voices), &body["request"]["inputs"])?
            } else {
                self.http
                    .bytes(self.native_request(model, &path, Some(&body["request"]))?)?
            };
            let codec = format.split('_').next().unwrap();
            let mime = match codec {
                "mp3" | "auto" => "audio/mpeg",
                "wav" => "audio/wav",
                "opus" => "audio/ogg",
                "pcm" => "audio/l16",
                "ulaw" => "audio/mulaw",
                "alaw" => "audio/alaw",
                _ => return Err(Error("invalid_native_audio_format")),
            };
            let media = native_provider::bundle(
                vec![native_provider::part("audio", mime, &bytes)?],
                vec![],
                Value::Null,
            )?;
            let reference =
                native_media::reference(spec, &format!("sync_{}", crate::protocol::id()))?;
            let hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            self.speech = Some((reference.clone(), digest(spec)?, media, hash));
            return Ok(reference);
        }
        if model["provider"] == "kling" {
            let data =
                self.http
                    .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
            let code = data["code"]
                .as_i64()
                .ok_or(Error("invalid_provider_response"))?;
            if code != 0 {
                need(
                    data["data"]["id"].is_null() && data["data"]["task_id"].is_null(),
                    "invalid_provider_response",
                )?;
                return Err(crate::submission::kling_error(code));
            }
            let id = if model["kind"] == "image" {
                &data["data"]["task_id"]
            } else {
                &data["data"]["id"]
            };
            return native_media::reference(spec, s(id)?);
        }
        if matches!(
            model["adapter"].as_str(),
            Some("schema-minimax-tts" | "schema-minimax-image")
        ) {
            self.speech = None;
            let data =
                self.http
                    .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
            let code = data["base_resp"]["status_code"]
                .as_i64()
                .ok_or(Error("invalid_provider_response"))?;
            if code != 0 {
                return Err(crate::submission::minimax_error(code));
            }
            let mut parts = vec![];
            let mut timestamps = Value::Null;
            if model["kind"] == "image" {
                let images = data["data"]["image_base64"]
                    .as_array()
                    .ok_or(Error("provider_no_media"))?;
                need(
                    images.len() as u64 == spec["input"]["parameters"]["n"].as_u64().unwrap(),
                    "provider_no_media",
                )?;
                for image in images {
                    let bytes = crate::protocol::decode(s(image)?)?;
                    parts.push(native_provider::part(
                        "image",
                        native_provider::mime(&bytes)?,
                        &bytes,
                    )?);
                }
            } else {
                let bytes = hex::decode(s(&data["data"]["audio"])?)
                    .map_err(|_| Error("invalid_provider_media"))?;
                let format = spec["input"]["parameters"]["audio_setting"]["format"]
                    .as_str()
                    .unwrap_or("mp3");
                let mime = match format {
                    "mp3" => "audio/mpeg",
                    "wav" => "audio/wav",
                    "flac" => "audio/flac",
                    "pcm" => "audio/l16",
                    _ => return Err(Error("invalid_native_audio_format")),
                };
                parts.push(native_provider::part("audio", mime, &bytes)?);
                if spec["input"]["parameters"]["subtitle_enable"] == true {
                    let bytes = self.download_bounded(
                        s(&model["provider"])?,
                        s(&data["data"]["subtitle_file"])?,
                        &["*.minimax.io", "*.minimaxi.com", "*.minimax.chat"],
                        None,
                        &[],
                        1024 * 1024,
                    )?;
                    let segments: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| Error("invalid_provider_media"))?;
                    timestamps = json!({"segments":segments});
                }
            }
            let media = native_provider::bundle(parts, vec![], timestamps)?;
            let reference =
                native_media::reference(spec, &format!("sync_{}", crate::protocol::id()))?;
            let hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            self.speech = Some((reference.clone(), digest(spec)?, media, hash));
            return Ok(reference);
        }
        if model["adapter"] == "schema-minimax-video" {
            let data =
                self.http
                    .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
            let code = data["base_resp"]["status_code"]
                .as_i64()
                .ok_or(Error("invalid_provider_response"))?;
            if code != 0 {
                need(data["task_id"].is_null(), "invalid_provider_response")?;
                return Err(crate::submission::minimax_error(code));
            }
            return native_media::reference(spec, s(&data["task_id"])?);
        }
        need(model["adapter"] == "schema-bfl", "unsupported_provider")?;
        let data =
            self.http
                .json(self.native_request(model, s(&model["apiPath"])?, Some(body))?)?;
        native_media::reference(spec, &crate::bfl::submission_reference(&data)?)
    }
    fn schema_read(&self, reference: &str, spec: &Value, model: &Value) -> Result<Value> {
        if model["provider"] == "runway" {
            let id = native_media::reference_id(spec, reference)?;
            let data =
                self.http
                    .json(self.native_request(model, &format!("/v1/tasks/{id}"), None)?)?;
            need(data["id"] == id, "provider_id_mismatch")?;
            return Ok(data);
        }
        if model["provider"] == "luma-agents" {
            let id = native_media::reference_id(spec, reference)?;
            let data = self.http.json(self.native_request(
                model,
                &format!("/v1/generations/{id}"),
                None,
            )?)?;
            need(
                data["id"] == id && data["model"] == model["apiModel"],
                "provider_id_mismatch",
            )?;
            return Ok(data);
        }
        if model["provider"] == "kling" {
            let id = native_media::reference_id(spec, reference)?;
            let path = if model["kind"] == "image" {
                format!("{}/{id}", s(&model["apiPath"])?)
            } else {
                format!("/tasks?task_ids={id}")
            };
            let data = self.http.json(self.native_request(model, &path, None)?)?;
            need(data["code"] == 0, "invalid_provider_response")?;
            let task = if model["kind"] == "image" {
                &data["data"]
            } else {
                let rows = data["data"]
                    .as_array()
                    .ok_or(Error("invalid_provider_response"))?;
                need(rows.len() == 1, "provider_id_mismatch")?;
                &rows[0]
            };
            need(
                if model["kind"] == "image" {
                    task["task_id"] == id
                } else {
                    task["id"] == id
                },
                "provider_id_mismatch",
            )?;
            return Ok(task.clone());
        }
        if model["adapter"] == "schema-minimax-video" {
            let id = native_media::reference_id(spec, reference)?;
            let data = self.http.json(self.native_request(
                model,
                &format!("/v2/query/video_generation/{id}"),
                None,
            )?)?;
            let task = &data["task"];
            need(task["id"] == id, "provider_id_mismatch")?;
            if !task["model"].is_null() {
                need(
                    task["model"] == model["apiModel"],
                    "provider_model_mismatch",
                )?;
            }
            return Ok(task.clone());
        }
        need(model["adapter"] == "schema-bfl", "unsupported_provider")?;
        let bound = native_media::reference_id(spec, reference)?;
        let (id, url) = crate::bfl::polling_url(bound)?;
        let host = url::Url::parse(&url)
            .map_err(|_| Error("invalid_provider_ref"))?
            .host_str()
            .unwrap()
            .to_string();
        let mut req = Request::get(url, &[&host]);
        req.headers = self.auth("bfl")?;
        req.timeout = 60;
        let data = self.http.json(req)?;
        if let Some(returned) = data.get("id") {
            need(
                returned.as_str() == Some(id.as_str()),
                "provider_id_mismatch",
            )?;
        }
        Ok(data)
    }
    pub(crate) fn schema_poll(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<String> {
        if model["ltx25"] == true && model["provider"] != "wavespeed" {
            return self.ltx25_poll(reference, spec, model);
        }
        if model["provider"] == "wavespeed" {
            return self.wavespeed_poll(reference, spec, model);
        }
        if model["adapter"] == "schema-minimax-tts"
            || model["adapter"] == "schema-minimax-image"
            || model["provider"] == "elevenlabs"
        {
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
        let d = self.schema_read(reference, spec, model)?;
        if model["provider"] == "runway" {
            return match d["status"].as_str() {
                Some("PENDING" | "THROTTLED" | "RUNNING") => Ok("IN_PROGRESS".into()),
                Some("SUCCEEDED") => Ok("COMPLETED".into()),
                Some("FAILED" | "CANCELLED") => Err(self.provider_failure(
                    Error("provider_generation_failed"),
                    s(&model["provider"])?,
                    &d,
                )),
                _ => Err(Error("invalid_provider_status")),
            };
        }
        if model["provider"] == "luma-agents" {
            return match d["state"].as_str() {
                Some("queued" | "processing") => Ok("IN_PROGRESS".into()),
                Some("completed") => Ok("COMPLETED".into()),
                Some("failed") => Err(self.provider_failure(
                    Error("provider_generation_failed"),
                    s(&model["provider"])?,
                    &d,
                )),
                _ => Err(Error("invalid_provider_status")),
            };
        }
        if model["provider"] == "kling" {
            let status = if model["kind"] == "image" {
                &d["task_status"]
            } else {
                &d["status"]
            };
            return match status.as_str() {
                Some("succeed" | "succeeded") => Ok("COMPLETED".into()),
                Some("submitted" | "processing") => Ok("IN_PROGRESS".into()),
                Some("failed") => Err(self.provider_failure(
                    Error("provider_generation_failed"),
                    s(&model["provider"])?,
                    &d,
                )),
                _ => Err(Error("invalid_provider_status")),
            };
        }
        if model["adapter"] == "schema-minimax-video" {
            return match d["status"].as_str() {
                Some("succeeded") => Ok("COMPLETED".into()),
                Some("queued" | "running") => Ok("IN_PROGRESS".into()),
                Some("failed" | "cancelled") => Err(self.provider_failure(
                    Error("provider_generation_failed"),
                    s(&model["provider"])?,
                    &d,
                )),
                _ => Err(Error("invalid_provider_status")),
            };
        }
        crate::bfl::poll_status(&d["status"])
            .map(str::to_owned)
            .map_err(|e| self.provider_failure(e, s(&model["provider"]).unwrap_or(""), &d))
    }
    pub(crate) fn schema_result(
        &self,
        reference: &str,
        spec: &Value,
        model: &Value,
    ) -> Result<Output> {
        if model["ltx25"] == true && model["provider"] != "wavespeed" {
            return self.ltx25_result(reference, spec, model);
        }
        if model["provider"] == "wavespeed" {
            return self.wavespeed_result(reference, spec, model);
        }
        if model["adapter"] == "schema-minimax-tts"
            || model["adapter"] == "schema-minimax-image"
            || model["provider"] == "elevenlabs"
        {
            self.schema_poll(reference, spec, model)?;
            let (_, _, media, hash) = self.speech.as_ref().unwrap();
            return Ok(Output {
                media: media.clone(),
                response_hash: hash.clone(),
            });
        }
        let d = self.schema_read(reference, spec, model)?;
        if model["provider"] == "runway" {
            need(d["status"] == "SUCCEEDED", "provider_not_complete")?;
            let rows = d["output"].as_array().ok_or(Error("provider_no_media"))?;
            let kind = s(&model["kind"])?;
            let mut parts = vec![];
            for row in rows {
                let bytes = self.download_bounded(
                    s(&model["provider"])?,
                    s(row)?,
                    &[
                        "*.runwayml.com",
                        "*.runwayml.cloud",
                        "dnznrvs05pmza.cloudfront.net",
                    ],
                    None,
                    &[],
                    80 * 1024 * 1024,
                )?;
                let format = spec["input"]["parameters"]["outputFormat"]
                    .as_str()
                    .unwrap_or("mp4");
                let mime = if kind == "image" {
                    native_provider::mime(&bytes)?
                } else if ["prores", "hdr_prores", "hdr_pq_12bit_master"].contains(&format) {
                    "video/quicktime"
                } else {
                    "video/mp4"
                };
                parts.push(native_provider::part(kind, mime, &bytes)?);
            }
            let media = native_provider::bundle(parts, vec![], Value::Null)?;
            let response_hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            return Ok(Output {
                media,
                response_hash,
            });
        }
        if model["provider"] == "luma-agents" {
            need(d["state"] == "completed", "provider_not_complete")?;
            let rows = d["output"].as_array().ok_or(Error("provider_no_media"))?;
            need(rows.len() == 1, "provider_no_media")?;
            let kind = s(&model["kind"])?;
            need(rows[0]["type"] == kind, "invalid_provider_media")?;
            let bytes = self.download_bounded(
                s(&model["provider"])?,
                s(&rows[0]["url"])?,
                &["*.lumalabs.ai", "*.luma.ai", "storage.cdn-luma.com"],
                None,
                &[],
                80 * 1024 * 1024,
            )?;
            let mime = if kind == "image" {
                native_provider::mime(&bytes)?
            } else {
                "video/mp4"
            };
            let media = native_provider::bundle(
                vec![native_provider::part(kind, mime, &bytes)?],
                vec![],
                Value::Null,
            )?;
            let response_hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            return Ok(Output {
                media,
                response_hash,
            });
        }
        if model["provider"] == "kling" {
            self.schema_poll(reference, spec, model)?;
            let kind = s(&model["kind"])?;
            need(
                if kind == "image" {
                    d["task_status"] == "succeed" || d["task_status"] == "succeeded"
                } else {
                    d["status"] == "succeeded"
                },
                "provider_not_complete",
            )?;
            let rows = if kind == "image" {
                &d["task_result"]["images"]
            } else {
                &d["outputs"]
            };
            let rows = rows.as_array().ok_or(Error("provider_no_media"))?;
            let mut parts = vec![];
            for row in rows {
                if kind == "video" && row["type"] != "video" {
                    continue;
                }
                let bytes = self.download_bounded(
                    s(&model["provider"])?,
                    s(&row["url"])?,
                    &["*.klingai.com", "h1.inkwai.com"],
                    None,
                    &[],
                    80 * 1024 * 1024,
                )?;
                parts.push(native_provider::part(
                    kind,
                    if kind == "image" {
                        native_provider::mime(&bytes)?
                    } else {
                        "video/mp4"
                    },
                    &bytes,
                )?);
            }
            let media = native_provider::bundle(parts, vec![], Value::Null)?;
            let response_hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            return Ok(Output {
                media,
                response_hash,
            });
        }
        if model["adapter"] == "schema-minimax-video" {
            need(d["status"] == "succeeded", "provider_not_complete")?;
            let bytes = self.download_bounded(
                s(&model["provider"])?,
                s(&d["content"]["url"])?,
                &[
                    "video-product.cdn.minimax.io",
                    "video.cdn.minimax.io",
                    "file.cdn.minimax.io",
                    "cdn.minimax.io",
                ],
                None,
                &[],
                80 * 1024 * 1024,
            )?;
            let media = native_provider::bundle(
                vec![native_provider::part("video", "video/mp4", &bytes)?],
                vec![],
                Value::Null,
            )?;
            let response_hash = digest(
                &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
            )?;
            return Ok(Output {
                media,
                response_hash,
            });
        }
        need(d["status"] == "Ready", "provider_not_complete")?;
        let delivery = crate::bfl::delivery_url(s(&d["result"]["sample"])?)?;
        let bytes = self.download_bounded(
            s(&model["provider"])?,
            delivery.as_str(),
            &[delivery.host_str().unwrap()],
            None,
            &[],
            80 * 1024 * 1024,
        )?;
        let kind = s(&model["kind"])?;
        let mime = if kind == "image" {
            native_provider::mime(&bytes)?
        } else {
            "video/mp4"
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
