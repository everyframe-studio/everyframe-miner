use crate::{
    Error, Result,
    invitation::{PROVIDERS, configured},
    matches,
    models::{self, MAX_MEDIA},
    need,
    network::{Http, Request, safe_url},
    protocol::{digest, id, sha},
    s,
};
use serde_json::{Value, json};
pub struct Output {
    pub media: Vec<u8>,
    pub response_hash: String,
}
pub trait Provider {
    fn models(&self) -> Vec<String>;
    fn submit(&mut self, spec: &Value) -> Result<String>;
    fn poll(&mut self, reference: &str, spec: &Value) -> Result<String>;
    fn result(&mut self, reference: &str, spec: &Value) -> Result<Output>;
}
pub struct Router<'a> {
    pub http: &'a dyn Http,
    pub credentials: Value,
    speech: Option<(String, String, Vec<u8>, String)>,
}
fn reference(v: &Value) -> Result<String> {
    need(matches("[a-zA-Z0-9_-]{1,150}", v), "invalid_provider_ref")?;
    Ok(s(v)?.into())
}
fn prefix(p: &str) -> &str {
    match p {
        "fal" => "https://queue.fal.run",
        "minimax" => "https://api.minimax.io",
        "openrouter" => "https://openrouter.ai",
        "bfl" => "https://api.bfl.ai",
        "replicate" => "https://api.replicate.com",
        "google" => "https://generativelanguage.googleapis.com",
        "runway" => "https://api.dev.runwayml.com",
        "luma" => "https://api.lumalabs.ai",
        "elevenlabs" => "https://api.elevenlabs.io",
        "vivix" => "https://api.vivix.ai",
        _ => "",
    }
}
fn bfl_host(region: &str) -> Result<&'static str> {
    match region {
        "global" => Ok("api.bfl.ai"),
        "us1" => Ok("api.us1.bfl.ai"),
        "eu1" => Ok("api.eu1.bfl.ai"),
        _ => Err(Error("invalid_provider_ref")),
    }
}
fn bfl_ref(r: &str) -> Result<(String, String)> {
    let parts = r.splitn(3, '_').collect::<Vec<_>>();
    need(
        parts.len() == 3 && parts[0] == "bfl",
        "invalid_provider_ref",
    )?;
    let host = bfl_host(parts[1])?;
    let id = reference(&json!(parts[2]))?;
    Ok((id.clone(), format!("https://{host}/v1/get_result?id={id}")))
}
impl<'a> Router<'a> {
    pub fn new(http: &'a dyn Http, credentials: Value) -> Self {
        Self {
            http,
            credentials,
            speech: None,
        }
    }
    fn context(&self, spec: &Value) -> Result<(&'static Value, String)> {
        models::check(spec)?;
        let m = models::info(s(&spec["model"])?)?;
        let p = s(&m["provider"])?;
        let key = PROVIDERS
            .iter()
            .find(|(name, _)| *name == p)
            .ok_or(Error("unsupported_provider"))?
            .1;
        need(configured(&self.credentials[key]), "provider_key_required")?;
        Ok((m, p.into()))
    }
    fn auth(&self, p: &str) -> Result<Vec<(String, String)>> {
        let key = PROVIDERS
            .iter()
            .find(|(name, _)| *name == p)
            .ok_or(Error("unsupported_provider"))?
            .1;
        let key = s(&self.credentials[key])?;
        let mut h = match p {
            "fal" => vec![("authorization".into(), format!("Key {key}"))],
            "bfl" => vec![("x-key".into(), key.into())],
            "google" => vec![("x-goog-api-key".into(), key.into())],
            "elevenlabs" => vec![("xi-api-key".into(), key.into())],
            _ => vec![("authorization".into(), format!("Bearer {key}"))],
        };
        if p == "runway" {
            h.push(("X-Runway-Version".into(), "2024-11-06".into()))
        }
        Ok(h)
    }
    fn api(&self, p: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let origin = prefix(p);
        need(!origin.is_empty(), "unsupported_provider")?;
        let host = origin.trim_start_matches("https://");
        let mut req = Request::get(format!("{origin}{path}"), &[host]);
        req.headers = self.auth(p)?;
        req.timeout = 30;
        if let Some(b) = body {
            req = req.json("POST", b)
        };
        let data = self.http.json(req)?;
        if p == "vivix" {
            // Never expose upstream messages (which may echo credentials/prompts),
            // and never retry a POST whose acceptance is uncertain.
            let code = data["code"]
                .as_i64()
                .ok_or(Error("invalid_provider_response"))?;
            if code != 0 {
                return Err(Error(match code {
                    10001 | 10003 => "vivix_auth_rejected",
                    10004 | 10005 => "vivix_billing_required",
                    10008 | 30008 => "vivix_capacity_limited",
                    20004 => "vivix_invalid_parameters",
                    20005 => "vivix_job_not_found",
                    30001 => "vivix_content_rejected",
                    50001 | 50003 => "vivix_unavailable",
                    _ => "provider_rejected",
                }));
            }
            need(data["data"].is_object(), "invalid_provider_response")?;
            return Ok(data["data"].clone());
        }
        if p == "minimax" {
            need(data["base_resp"]["status_code"] == 0, "provider_rejected")?
        };
        Ok(data)
    }
    fn native<'b>(&self, r: &'b str, p: &str) -> Result<&'b str> {
        reference(&json!(r))?;
        if p == "fal" {
            Ok(r)
        } else {
            let v = r
                .strip_prefix(&format!("{p}_"))
                .ok_or(Error("provider_ref_namespace"))?;
            reference(&json!(v))?;
            Ok(v)
        }
    }
    fn studio_submit(&self, spec: &Value, p: &str) -> Result<String> {
        let checked = crate::studio_video::references(self.http, spec)?;
        let mut urls = vec![];
        for (a, bytes) in checked {
            if p != "vivix" {
                urls.push(s(&a["url"])?.to_string());
                continue;
            }
            let mime = s(&a["mime"])?;
            let kind = if mime.starts_with("image/") {
                "images"
            } else {
                "audios"
            };
            let ext = match mime {
                "image/jpeg" => "jpg",
                "image/png" => "png",
                "image/webp" => "webp",
                "audio/mpeg" => "mp3",
                "audio/wav" => "wav",
                "audio/mp4" => "m4a",
                "audio/ogg" => "ogg",
                "audio/webm" => "webm",
                _ => return Err(Error("invalid_reference_mime")),
            };
            let boundary = format!("everyframe-{}", id());
            let mut data=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"reference.{ext}\"\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
            data.extend_from_slice(&bytes);
            data.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let mut req = Request::get(
                format!("https://api.vivix.ai/v1/files/{kind}"),
                &["api.vivix.ai"],
            );
            req.method = "POST".into();
            req.headers = self.auth("vivix")?;
            req.headers.push((
                "content-type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            ));
            req.body = Some(data);
            req.timeout = 60;
            let result = self.http.json(req)?;
            need(result["code"] == 0, "reference_upload_failed")?;
            let url = s(&result["data"]["url"])?;
            safe_url(
                url,
                &["static.vivi-x.ai".into(), "clipgen-us.vivi-x.ai".into()],
            )?;
            urls.push(url.into());
        }
        let (path, body) = crate::studio_video::body(spec, &urls)?;
        let result = self.api(p, &path, Some(&body))?;
        let r = match p {
            "fal" => reference(&result["request_id"])?,
            "openrouter" => reference(&result["id"])?,
            "vivix" => {
                need(
                    result["results"].as_array().is_some_and(|a| a.len() == 1),
                    "invalid_provider_response",
                )?;
                reference(&result["results"][0]["job_id"])?
            }
            _ => return Err(Error("unsupported_provider")),
        };
        Ok(if p == "fal" { r } else { format!("{p}_{r}") })
    }
    fn read(&self, r: &str, spec: &Value, m: &Value, p: &str) -> Result<Value> {
        let model = m["apiModel"].as_str().unwrap_or("");
        let data = match p {
            "vivix" => {
                let d = self.api(p, &format!("/v1/videos/jobs/{r}"), None)?;
                need(d["job_id"] == r, "provider_id_mismatch")?;
                // Echo fields are optional while queued. Bind them when supplied;
                // the encoded output is still independently checked by validators.
                need(
                    (d.get("duration").is_none() || d["duration"] == spec["input"]["duration"])
                        && (d.get("aspect_ratio").is_none()
                            || d["aspect_ratio"]
                                == if s(&spec["model"])?.starts_with("studio/") {
                                    spec["input"]["aspect"].clone()
                                } else {
                                    spec["input"]["aspect_ratio"].clone()
                                })
                        && (d.get("model").is_none() || d["model"] == model),
                    "provider_input_mismatch",
                )?;
                d
            }
            "fal" => self.api(p, &format!("/{}/requests/{r}/status", s(&m["app"])?), None)?,
            "minimax" => {
                let d = self.api(p, &format!("/v1/query/video_generation?task_id={r}"), None)?;
                need(d["task_id"] == r, "provider_id_mismatch")?;
                d
            }
            "openrouter" => {
                let d = self.api(p, &format!("/api/v1/videos/{r}"), None)?;
                need(d["id"] == r, "provider_id_mismatch")?;
                need(
                    d.get("model").is_none() || d["model"] == model,
                    "provider_model_mismatch",
                )?;
                d
            }
            "bfl" => {
                let (id, url) = bfl_ref(r)?;
                let host = url::Url::parse(&url)
                    .unwrap()
                    .host_str()
                    .unwrap()
                    .to_string();
                let mut req = Request::get(url, &[&host]);
                req.headers = self.auth(p)?;
                let d = self.http.json(req)?;
                need(
                    d.get("id").is_none() || d["id"] == id,
                    "provider_id_mismatch",
                )?;
                d
            }
            "replicate" => {
                let d = self.api(p, &format!("/v1/predictions/{r}"), None)?;
                need(d["id"] == r, "provider_id_mismatch")?;
                need(
                    d["model"] == model && digest(&d["input"])? == digest(&spec["input"])?,
                    "provider_input_mismatch",
                )?;
                d
            }
            "google" => {
                let name = format!("models/{model}/operations/{r}");
                let d = self.api(p, &format!("/v1beta/{name}"), None)?;
                need(d["name"] == name, "provider_id_mismatch")?;
                d
            }
            "runway" | "luma" => {
                let path = if p == "runway" {
                    format!("/v1/tasks/{r}")
                } else {
                    format!("/dream-machine/v1/generations/{r}")
                };
                let d = self.api(p, &path, None)?;
                need(d["id"] == r, "provider_id_mismatch")?;
                d
            }
            "elevenlabs" => {
                let speech = self
                    .speech
                    .as_ref()
                    .ok_or(Error("synchronous_result_unavailable"))?;
                need(
                    speech.0 == r && speech.1 == digest(spec)?,
                    "synchronous_result_unavailable",
                )?;
                json!({})
            }
            _ => return Err(Error("unsupported_provider")),
        };
        Ok(data)
    }
    fn state(p: &str, d: &Value) -> Result<&'static str> {
        if p == "elevenlabs" {
            return Ok("COMPLETED");
        }
        if p == "google" {
            if !d["error"].is_null() {
                return Ok("FAILED");
            };
            need(
                d.get("done").is_none() || d["done"].is_boolean(),
                "invalid_provider_status",
            )?;
            return Ok(if d["done"] == true {
                "COMPLETED"
            } else {
                "IN_PROGRESS"
            });
        };
        let st = s(if p == "luma" {
            &d["state"]
        } else {
            &d["status"]
        })?;
        let (queue, progress, complete, failed): (&[&str], &[&str], &[&str], &[&str]) = match p {
            "fal" => (&["IN_QUEUE"], &["IN_PROGRESS"], &["COMPLETED"], &[]),
            "minimax" => (
                &["Preparing", "Queueing"],
                &["Processing"],
                &["Success"],
                &["Fail"],
            ),
            "openrouter" => (
                &["pending"],
                &["in_progress"],
                &["completed"],
                &["failed", "cancelled", "expired"],
            ),
            "bfl" => (
                &[],
                &["Pending"],
                &["Ready"],
                &[
                    "Error",
                    "Failed",
                    "Request Moderated",
                    "Content Moderated",
                    "Task not found",
                ],
            ),
            "replicate" => (
                &["starting"],
                &["processing"],
                &["succeeded"],
                &["failed", "canceled"],
            ),
            "runway" => (
                &["PENDING", "THROTTLED"],
                &["RUNNING"],
                &["SUCCEEDED"],
                &["FAILED", "CANCELLED"],
            ),
            "luma" => (&["queued"], &["dreaming"], &["completed"], &["failed"]),
            "vivix" => (&["queued"], &["processing"], &["succeeded"], &["failed"]),
            _ => return Err(Error("unsupported_provider")),
        };
        if queue.contains(&st) {
            Ok("IN_QUEUE")
        } else if progress.contains(&st) {
            Ok("IN_PROGRESS")
        } else if complete.contains(&st) {
            Ok("COMPLETED")
        } else if failed.contains(&st) {
            Ok("FAILED")
        } else {
            Err(Error("invalid_provider_status"))
        }
    }
    fn download(
        &self,
        url: &str,
        hosts: &[&str],
        auth: Option<&str>,
        redirects: &[&str],
    ) -> Result<Vec<u8>> {
        let mut r = Request::get(url, hosts);
        safe_url(url, &r.hosts)?;
        r.limit = MAX_MEDIA;
        r.timeout = 30;
        r.redirect_hosts = redirects.iter().map(|s| s.to_string()).collect();
        if let Some(p) = auth {
            r.headers = self.auth(p)?
        };
        let b = self.http.bytes(r)?;
        need(!b.is_empty() && b.len() <= MAX_MEDIA, "invalid_media_size")?;
        Ok(b)
    }
}
impl Provider for Router<'_> {
    fn models(&self) -> Vec<String> {
        models::MODELS
            .as_object()
            .unwrap()
            .iter()
            .chain(models::STUDIO_MODELS.as_object().unwrap().iter())
            .filter(|(_, m)| {
                PROVIDERS
                    .iter()
                    .any(|(p, k)| m["provider"] == *p && configured(&self.credentials[k]))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
    fn submit(&mut self, spec: &Value) -> Result<String> {
        let (m, p) = self.context(spec)?;
        if s(&spec["model"])?.starts_with("studio/") {
            return self.studio_submit(spec, &p);
        }
        let model = m["apiModel"].as_str().unwrap_or("");
        let input = &spec["input"];
        let mut body = input.clone();
        let result = match p.as_str() {
            "vivix" => {
                // Seed belongs to the envelope, not jobs[0]. Unknown fields,
                // including resolution, are rejected by the provider.
                body.as_object_mut().unwrap().remove("seed");
                let d = self.api(
                    &p,
                    "/v1/videos/jobs",
                    Some(&json!({"model":model,"seed":input["seed"],"jobs":[body]})),
                )?;
                let jobs = d["results"]
                    .as_array()
                    .ok_or(Error("invalid_provider_response"))?;
                need(jobs.len() == 1, "invalid_provider_response")?;
                reference(&jobs[0]["job_id"])?
            }
            "fal" => reference(
                &self.api(&p, &format!("/{}", s(&spec["model"])?), Some(input))?["request_id"],
            )?,
            "minimax" | "openrouter" | "luma" => {
                body["model"] = json!(model);
                let (path, key) = match p.as_str() {
                    "minimax" => ("/v1/video_generation", "task_id"),
                    "openrouter" => ("/api/v1/videos", "id"),
                    _ => ("/dream-machine/v1/generations", "id"),
                };
                reference(&self.api(&p, path, Some(&body))?[key])?
            }
            "bfl" => {
                let d = self.api(&p, &format!("/v1/{model}"), Some(input))?;
                let id = reference(&d["id"])?;
                let u = safe_url(
                    s(&d["polling_url"])?,
                    &[
                        "api.bfl.ai".into(),
                        "api.us1.bfl.ai".into(),
                        "api.eu1.bfl.ai".into(),
                    ],
                )?;
                need(
                    u.path() == "/v1/get_result"
                        && u.query_pairs().count() == 1
                        && u.query_pairs().any(|(k, v)| k == "id" && v == id),
                    "invalid_polling_url",
                )?;
                let region = match u.host_str() {
                    Some("api.bfl.ai") => "global",
                    Some("api.us1.bfl.ai") => "us1",
                    _ => "eu1",
                };
                reference(&json!(format!("bfl_{region}_{id}")))?
            }
            "replicate" => reference(
                &self.api(
                    &p,
                    &format!("/v1/models/{model}/predictions"),
                    Some(&json!({"input":input})),
                )?["id"],
            )?,
            "google" => {
                body.as_object_mut().unwrap().remove("prompt");
                let d = self.api(
                    &p,
                    &format!("/v1beta/models/{model}:predictLongRunning"),
                    Some(&json!({"instances":[{"prompt":input["prompt"]}],"parameters":body})),
                )?;
                let name = s(&d["name"])?;
                reference(&json!(
                    name.strip_prefix(&format!("models/{model}/operations/"))
                        .ok_or(Error("invalid_provider_ref"))?
                ))?
            }
            "runway" => {
                body.as_object_mut().unwrap().remove("prompt");
                body["promptText"] = input["prompt"].clone();
                body["model"] = json!(model);
                reference(&self.api(&p, "/v1/text_to_video", Some(&body))?["id"])?
            }
            "elevenlabs" => {
                self.speech = None;
                let url = format!(
                    "{}/v1/text-to-speech/{}?output_format={}",
                    prefix(&p),
                    s(&input["voice_id"])?,
                    s(&input["output_format"])?
                );
                let mut r = Request::get(url, &["api.elevenlabs.io"])
                    .json("POST", &json!({"text":input["prompt"],"model_id":model}));
                r.headers.extend(self.auth(&p)?);
                r.limit = 10 * 1024 * 1024;
                r.timeout = 60;
                let media = self.http.bytes(r)?;
                need(
                    !media.is_empty() && media.len() <= 10 * 1024 * 1024,
                    "invalid_media_size",
                )?;
                let local = format!("sync_{}", id());
                let input_hash = digest(spec)?;
                let hash = digest(
                    &json!({"localResponseId":local,"inputHash":input_hash,"outputHash":sha(&media),"provider":p}),
                )?;
                self.speech = Some((local.clone(), input_hash, media, hash));
                local
            }
            _ => return Err(Error("unsupported_provider")),
        };
        reference(&json!(if p == "fal" {
            result
        } else {
            format!("{p}_{result}")
        }))
    }
    fn poll(&mut self, r: &str, spec: &Value) -> Result<String> {
        let (m, p) = self.context(spec)?;
        let r = self.native(r, &p)?;
        Ok(Self::state(&p, &self.read(r, spec, m, &p)?)?.into())
    }
    fn result(&mut self, r: &str, spec: &Value) -> Result<Output> {
        let (m, p) = self.context(spec)?;
        let r = self.native(r, &p)?;
        if p == "fal" {
            let data = self.api(&p, &format!("/{}/requests/{r}", s(&m["app"])?), None)?;
            let media = self.download(s(&data["video"]["url"])?, &["*.fal.media"], None, &[])?;
            return Ok(Output {
                media,
                response_hash: sha(data.to_string()),
            });
        };
        let data = self.read(r, spec, m, &p)?;
        need(
            Self::state(&p, &data)? == "COMPLETED",
            "provider_not_complete",
        )?;
        let mut metadata = data.clone();
        let media = match p.as_str() {
            // Exact CDN hosts: clipgen-us was returned by the authenticated
            // W1 API in the 2026-10-12 smoke test. No wildcard or redirects.
            "vivix" => self.download(
                s(&data["video_url"])?,
                &["static.vivi-x.ai", "clipgen-us.vivi-x.ai"],
                None,
                &[],
            )?,
            "minimax" => {
                let file_id = reference(&data["file_id"])?;
                let file = self.api(&p, &format!("/v1/files/retrieve?file_id={file_id}"), None)?;
                need(
                    file["file"]["file_id"] == data["file_id"],
                    "provider_file_mismatch",
                )?;
                let media = self.download(
                    s(&file["file"]["download_url"])?,
                    &[
                        "file.cdn.minimax.io",
                        "video.cdn.minimax.io",
                        "cdn.minimax.io",
                    ],
                    None,
                    &[],
                )?;
                metadata = json!({"task":data,"file":file});
                media
            }
            "openrouter" => self.download(
                &format!("{}/api/v1/videos/{r}/content?index=0", prefix(&p)),
                &["openrouter.ai"],
                Some(&p),
                &[],
            )?,
            "bfl" => self.download(
                s(&data["result"]["sample"])?,
                &[
                    "delivery-us1.bfl.ai",
                    "delivery-eu1.bfl.ai",
                    "delivery.bfl.ai",
                ],
                None,
                &[],
            )?,
            "replicate" => self.download(
                s(&data["output"])?,
                &["replicate.delivery", "*.replicate.delivery"],
                None,
                &[],
            )?,
            "google" => {
                let samples = data["response"]["generateVideoResponse"]["generatedSamples"]
                    .as_array()
                    .ok_or(Error("provider_no_media"))?;
                need(samples.len() == 1, "provider_no_media")?;
                let url = s(&samples[0]["video"]["uri"])?;
                let u = safe_url(url, &["generativelanguage.googleapis.com".into()])?;
                need(
                    regex::Regex::new(r"^/v1beta/files/[a-zA-Z0-9_-]+:download$")
                        .unwrap()
                        .is_match(u.path())
                        && u.query() == Some("alt=media"),
                    "unapproved_download_path",
                )?;
                self.download(
                    url,
                    &["generativelanguage.googleapis.com"],
                    Some(&p),
                    &[
                        "generativelanguage.googleapis.com",
                        "*.googleusercontent.com",
                        "storage.googleapis.com",
                    ],
                )?
            }
            "runway" => {
                let out = data["output"]
                    .as_array()
                    .ok_or(Error("provider_no_media"))?;
                need(out.len() == 1, "provider_no_media")?;
                self.download(
                    s(&out[0])?,
                    &[
                        "*.runwayml.com",
                        "*.runwayml.cloud",
                        "dnznrvs05pmza.cloudfront.net",
                    ],
                    None,
                    &[],
                )?
            }
            "luma" => self.download(
                s(&data["assets"]["video"])?,
                &["*.lumalabs.ai", "*.luma.ai"],
                None,
                &[],
            )?,
            "elevenlabs" => {
                let speech = self
                    .speech
                    .as_ref()
                    .ok_or(Error("synchronous_result_unavailable"))?;
                return Ok(Output {
                    media: speech.2.clone(),
                    response_hash: speech.3.clone(),
                });
            }
            _ => return Err(Error("unsupported_provider")),
        };
        Ok(Output {
            media,
            response_hash: sha(metadata.to_string()),
        })
    }
}
