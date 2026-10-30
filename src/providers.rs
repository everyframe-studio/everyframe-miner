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
    fn refresh_availability(&mut self, _now: i64) -> Option<Value> {
        None
    }
    /// The coordinator's signed list of retired provider models (narrowing only).
    fn set_retirements(&mut self, _retired: &Value) {}
    fn submit(&mut self, spec: &Value) -> Result<String>;
    fn submit_before(&mut self, spec: &Value, _deadline: i64) -> Result<String> {
        self.submit(spec)
    }
    fn poll(&mut self, reference: &str, spec: &Value) -> Result<String>;
    fn result(&mut self, reference: &str, spec: &Value) -> Result<Output>;
}
pub struct Router<'a> {
    pub http: &'a dyn Http,
    pub credentials: Value,
    speech: Option<(String, String, Vec<u8>, String)>,
    submission_deadline: Option<i64>,
    catalog: crate::model_availability::CatalogCache,
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
        "kling" => "https://api-singapore.klingai.com",
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
    fn guard_submission(&self, req: &mut Request) -> Result<()> {
        if let Some(deadline) = self.submission_deadline {
            need(crate::now() < deadline, "submission_deadline_elapsed")?;
            req.submit_deadline = Some(deadline);
        }
        Ok(())
    }
    pub fn new(http: &'a dyn Http, credentials: Value) -> Self {
        Self {
            http,
            credentials,
            speech: None,
            submission_deadline: None,
            catalog: crate::model_availability::CatalogCache::default(),
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
        if p == "fal"
            && body.is_none()
            && path.starts_with("/fal-ai/motion-to-code/requests/")
            && !path.ends_with("/status")
        {
            req.limit = 16 * 1024 * 1024;
        }
        if let Some(b) = body {
            req = req.json("POST", b);
            self.guard_submission(&mut req)?;
        };
        let data = self.http.json(req)?;
        if p == "kling" {
            let code = data["code"]
                .as_i64()
                .ok_or(Error("invalid_provider_response"))?;
            if code != 0 {
                return Err(crate::submission::kling_error(code));
            }
            need(
                data["data"].is_object() || data["data"].is_array(),
                "invalid_provider_response",
            )?;
            return Ok(data["data"].clone());
        }
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
            self.guard_submission(&mut req)?;
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
    fn media_submit(&mut self, spec: &Value, p: &str) -> Result<String> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let checked = crate::studio_media::references(self.http, spec)?;
        // Inline exactly the verified bytes for synchronous image APIs. Fal's
        // video queue receives immutable owned attachment URLs after hashing.
        let urls = checked
            .iter()
            .map(|(a, b)| {
                if p == "openrouter" {
                    format!(
                        "data:{};base64,{}",
                        a["mime"].as_str().unwrap(),
                        STANDARD.encode(b)
                    )
                } else {
                    a["url"].as_str().unwrap().to_string()
                }
            })
            .collect::<Vec<_>>();
        let (path, body) = crate::studio_media::body(spec, &urls)?;
        if p == "fal" {
            return reference(&self.api(p, &path, Some(&body))?["request_id"]);
        }
        need(p == "openrouter", "unsupported_provider")?;
        self.speech = None;
        let mut req = Request::get(format!("https://openrouter.ai{path}"), &["openrouter.ai"])
            .json("POST", &body);
        req.headers.extend(self.auth(p)?);
        req.timeout = 300;
        req.limit = 40 * 1024 * 1024;
        self.guard_submission(&mut req)?;
        let data = self.http.json(req)?;
        need(data.get("error").is_none(), "provider_rejected")?;
        let media = if path == "/api/v1/images" {
            need(
                data["data"].as_array().is_some_and(|a| a.len() == 1),
                "provider_no_media",
            )?;
            STANDARD
                .decode(s(&data["data"][0]["b64_json"])?)
                .map_err(|_| Error("invalid_provider_media"))?
        } else {
            let images = &data["choices"][0]["message"]["images"];
            need(
                images.as_array().is_some_and(|a| a.len() == 1),
                "provider_no_media",
            )?;
            let uri = s(&images[0]["image_url"]["url"])?;
            let (prefix, encoded) = uri.split_once(',').ok_or(Error("invalid_provider_media"))?;
            need(
                [
                    "data:image/png;base64",
                    "data:image/jpeg;base64",
                    "data:image/webp;base64",
                ]
                .contains(&prefix),
                "invalid_provider_media",
            )?;
            STANDARD
                .decode(encoded)
                .map_err(|_| Error("invalid_provider_media"))?
        };
        need(
            !media.is_empty() && media.len() <= 20 * 1024 * 1024,
            "invalid_media_size",
        )?;
        let local = format!("openrouter_sync_{}", id());
        let hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"responseId":data["id"],"localId":local}),
        )?;
        self.speech = Some((local.clone(), digest(spec)?, media, hash));
        Ok(local)
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
            "kling" => {
                let d = self.api(p, &format!("/tasks?task_ids={r}"), None)?;
                let tasks = d.as_array().ok_or(Error("invalid_provider_response"))?;
                need(tasks.len() == 1, "invalid_provider_response")?;
                need(tasks[0]["id"] == r, "provider_id_mismatch")?;
                tasks[0].clone()
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
                    d["model"].is_null() || d["model"] == model,
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
        if p == "fal" && !d["error"].is_null() {
            return Ok("FAILED");
        }
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
            "fal" => (
                &["IN_QUEUE"],
                &["IN_PROGRESS"],
                &["COMPLETED"],
                &["FAILED", "ERROR", "CANCELLED", "CANCELED"],
            ),
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
            "kling" => (&["submitted"], &["processing"], &["succeeded"], &["failed"]),
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
        r.timeout = 180;
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
    fn refresh_availability(&mut self, now: i64) -> Option<Value> {
        Some(self.catalog.refresh(self.http, &self.credentials, now))
    }
    fn set_retirements(&mut self, retired: &Value) {
        self.catalog.set_server_retirements(retired);
    }
    fn submit_before(&mut self, spec: &Value, deadline: i64) -> Result<String> {
        let (_, provider) = self.context(spec)?;
        if provider == "vivix"
            && let Ok(balance) =
                crate::balances::vivix_w_balance(self.http, s(&self.credentials["VIVIX_API_KEY"])?)
            && balance["remainingMicrounits"]
                .as_i64()
                .is_some_and(|v| v <= 0)
        {
            return Err(Error("vivix_billing_required"));
        }
        self.submission_deadline = Some(deadline);
        let result = self.submit(spec);
        self.submission_deadline = None;
        result
    }
    fn models(&self) -> Vec<String> {
        models::MODELS
            .as_object()
            .unwrap()
            .iter()
            .chain(models::STUDIO_MODELS.as_object().unwrap().iter())
            .filter(|(id, m)| {
                self.catalog.allows(id)
                    && PROVIDERS
                        .iter()
                        .any(|(p, k)| m["provider"] == *p && configured(&self.credentials[k]))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
    fn submit(&mut self, spec: &Value) -> Result<String> {
        let (m, p) = self.context(spec)?;
        need(
            self.catalog.allows(s(&spec["model"])?),
            "provider_model_unavailable",
        )?;
        if s(&spec["model"])?.starts_with("studio/tool/") {
            let urls = crate::studio_tools::references(self.http, spec)?;
            let (path, body) = crate::studio_tools::body(spec, &urls)?;
            return reference(&self.api(&p, &path, Some(&body))?["request_id"]);
        }
        if s(&spec["model"])?.starts_with("studio/media/") {
            return self.media_submit(spec, &p);
        }
        if s(&spec["model"])?.starts_with("studio/") {
            return self.studio_submit(spec, &p);
        }
        let model = m["apiModel"].as_str().unwrap_or("");
        let input = &spec["input"];
        let mut body = input.clone();
        let result = match p.as_str() {
            "kling" => {
                body.as_object_mut().unwrap().remove("prompt");
                let d = self.api(
                    &p,
                    "/text-to-video/kling-3.0",
                    Some(&json!({
                        "prompt": input["prompt"], "settings": body,
                        "options": {"watermark_info": {"enabled": false}}
                    })),
                )?;
                reference(&d["id"])?
            }
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
                self.guard_submission(&mut r)?;
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
        if r.starts_with("openrouter_sync_") {
            need(
                p == "openrouter" && m["kind"] == "image",
                "provider_ref_namespace",
            )?;
            need(
                self.speech
                    .as_ref()
                    .is_some_and(|(id, h, _, _)| id == r && digest(spec).is_ok_and(|d| d == *h)),
                "synchronous_result_unavailable",
            )?;
            return Ok("COMPLETED".into());
        }
        let r = self.native(r, &p)?;
        let data = self.read(r, spec, m, &p)?;
        let state = Self::state(&p, &data)?;
        if state == "FAILED" {
            // Report a fixed category so the coordinator can tell a content block
            // from a billing or provider fault. Provider text never leaves the TEE.
            return Err(Error(failure_category(&p, &data)));
        }
        Ok(state.into())
    }
    fn result(&mut self, r: &str, spec: &Value) -> Result<Output> {
        let (m, p) = self.context(spec)?;
        if r.starts_with("openrouter_sync_") {
            need(
                p == "openrouter" && m["kind"] == "image",
                "provider_ref_namespace",
            )?;
            let (id, h, media, hash) = self
                .speech
                .as_ref()
                .ok_or(Error("synchronous_result_unavailable"))?;
            need(id == r && *h == digest(spec)?, "provider_input_mismatch")?;
            return Ok(Output {
                media: media.clone(),
                response_hash: hash.clone(),
            });
        }
        let r = self.native(r, &p)?;
        if p == "fal" {
            let data = self.api(&p, &format!("/{}/requests/{r}", s(&m["app"])?), None)?;
            need(
                data["error"].is_null() && data["detail"].is_null(),
                failure_category(&p, &data),
            )?;
            let url = match m["kind"].as_str() {
                Some("image") => {
                    need(
                        data["images"].as_array().is_some_and(|a| a.len() == 1),
                        "provider_no_media",
                    )?;
                    s(&data["images"][0]["url"])?
                }
                Some("audio") => data["audio"]
                    .as_str()
                    .or_else(|| data["audio"]["url"].as_str())
                    .ok_or(Error("provider_no_media"))?,
                _ => data["video"]["url"]
                    .as_str()
                    .ok_or(Error("provider_no_media"))?,
            };
            let media = self.download(url, &["*.fal.media"], None, &[])?;
            let media = if m["mime"] == "application/vnd.everyframe.motion+json" {
                crate::studio_tools::motion_bundle(s(&spec["model"])?, &data, &media)?
            } else {
                media
            };
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
            "kling" => {
                let outputs = data["outputs"]
                    .as_array()
                    .ok_or(Error("provider_no_media"))?;
                need(
                    outputs.len() == 1 && outputs[0]["type"] == "video",
                    "provider_no_media",
                )?;
                // No API credentials, redirects, arbitrary origins or caller URLs.
                // Exact artifact admission and live CDN verification remain release gates.
                self.download(s(&outputs[0]["url"])?, &["*.klingai.com"], None, &[])?
            }
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
                &["*.lumalabs.ai", "*.luma.ai", "storage.cdn-luma.com"],
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

/// Classifies a provider's failure payload into one of the categories the
/// coordinator accepts. Only the category is reported, never provider text.
pub fn failure_category(provider: &str, data: &Value) -> &'static str {
    let status = data["status"].as_str().unwrap_or_default();
    if provider == "bfl" && (status == "Request Moderated" || status == "Content Moderated") {
        return "provider_content_rejected";
    }
    if provider == "vivix" {
        match data["code"]
            .as_i64()
            .or_else(|| data["error"]["code"].as_i64())
        {
            Some(30001) => return "provider_content_rejected",
            Some(10004 | 10005) => return "provider_billing_required",
            Some(20004 | 20008) => return "provider_invalid_request",
            _ => {}
        }
    }
    let mut codes = Vec::new();
    let mut messages = Vec::new();
    failure_fields(data, 0, &mut codes, &mut messages);
    // Structured types/codes take precedence over prose, which can quote user
    // input. Unknown fields (especially input, request, ctx and loc) are ignored.
    if let Some(category) = codes
        .iter()
        .find_map(|code| structured_failure_category(code))
    {
        return category;
    }
    let text = messages.join(" ");
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&[
        "content_policy",
        "content policy",
        "moderat",
        "nsfw",
        "safety",
        "sensitive",
        "prohibited",
    ]) {
        "provider_content_rejected"
    } else if has(&[
        "insufficient credits",
        "insufficient credit",
        "insufficient balance",
        "insufficient funds",
        "not enough credits",
        "out of credits",
        "exhausted balance",
        "balance exhausted",
        "payment required",
        "billing required",
    ]) {
        "provider_billing_required"
    } else if has(&[
        "value_error",
        "validation",
        "invalid",
        "literal_error",
        "missing",
        "unprocessable",
        "not supported",
        "unsupported",
    ]) {
        "provider_invalid_request"
    } else if has(&["timeout", "timed out"]) {
        "provider_timeout"
    } else {
        "provider_generation_failed"
    }
}

fn structured_failure_category(code: &str) -> Option<&'static str> {
    match code {
        "provider_content_rejected"
        | "content_policy_violation"
        | "content_policy"
        | "content_blocked"
        | "moderation_blocked"
        | "safety_violation"
        | "nsfw" => Some("provider_content_rejected"),
        "provider_billing_required"
        | "insufficient_credits"
        | "insufficient_balance"
        | "insufficient_funds"
        | "payment_required"
        | "billing_required" => Some("provider_billing_required"),
        "provider_invalid_request"
        | "invalid_request"
        | "invalid_request_error"
        | "validation_error"
        | "value_error"
        | "literal_error"
        | "missing"
        | "string_type"
        | "int_type"
        | "int_parsing"
        | "float_parsing" => Some("provider_invalid_request"),
        "provider_timeout" | "timeout" | "timed_out" => Some("provider_timeout"),
        "provider_unavailable" | "service_unavailable" => Some("provider_unavailable"),
        _ => None,
    }
}

/// Visit only error schema fields, never arbitrary provider JSON or echoed
/// request values. Bounds also prevent unusually nested errors from dominating
/// classification work. All collected text stays local; only fixed codes leave.
fn failure_fields(data: &Value, depth: usize, codes: &mut Vec<String>, messages: &mut Vec<String>) {
    if depth > 8 || codes.len() + messages.len() >= 128 {
        return;
    }
    match data {
        Value::String(text) => {
            messages.push(text.chars().take(2048).collect::<String>().to_lowercase())
        }
        Value::Array(items) => {
            for item in items.iter().take(32) {
                failure_fields(item, depth + 1, codes, messages);
            }
        }
        Value::Object(fields) => {
            for key in ["code", "type", "error_code", "failureCode"] {
                if let Some(code) = fields.get(key).and_then(Value::as_str)
                    && code.len() <= 128
                {
                    codes.push(code.to_ascii_lowercase());
                }
            }
            for key in ["message", "msg", "error_message"] {
                if let Some(text) = fields.get(key).and_then(Value::as_str) {
                    messages.push(text.chars().take(2048).collect::<String>().to_lowercase());
                }
            }
            for key in ["error", "errors", "detail", "failure"] {
                if let Some(value) = fields.get(key) {
                    failure_fields(value, depth + 1, codes, messages);
                }
            }
        }
        _ => {}
    }
}
