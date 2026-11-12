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
    fn set_media_policy(&mut self, _policy: &Value, _chain: &Value, _now: i64) -> Result<()> {
        Ok(())
    }
    fn models(&self) -> Vec<String>;
    fn refresh_availability(&mut self, _now: i64) -> Option<Value> {
        None
    }
    /// The coordinator's signed list of retired provider models (narrowing only).
    fn set_retirements(&mut self, _retired: &Value) {}
    fn provider_budget(&self) -> Option<Value> {
        None
    }
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
    pub(crate) media_policy: crate::media_policy::Cache,
    pub(crate) speech: Option<(String, String, Vec<u8>, String)>,
    submission_deadline: Option<i64>,
    budget: std::cell::RefCell<crate::provider_budget::Cache>,
    catalog: crate::model_availability::CatalogCache,
    pub(crate) ltx25_cache: std::cell::RefCell<Option<(String, i64, Value)>>,
    pub(crate) wavespeed_cache: std::cell::RefCell<Option<crate::wavespeed::Cached>>,
}
fn reference(v: &Value) -> Result<String> {
    need(matches("[a-zA-Z0-9_-]{1,150}", v), "invalid_provider_ref")?;
    Ok(s(v)?.into())
}
/// MiniMax's query response uses a string ID; file metadata may use int64.
/// Compare losslessly, never through a floating point conversion.
pub fn minimax_file_id(value: &Value) -> Result<String> {
    if value.is_string() {
        return reference(value);
    }
    value
        .as_u64()
        .map(|n| n.to_string())
        .ok_or(Error("invalid_provider_ref"))
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
        "pika" => "https://api.dev.pika.art",
        _ => "",
    }
}
fn bfl_ref(r: &str) -> Result<(String, String)> {
    crate::bfl::polling_url(
        r.strip_prefix("bfl_")
            .ok_or(Error("invalid_provider_ref"))?,
    )
}
impl<'a> Router<'a> {
    pub(crate) fn provider_failure(
        &self,
        error: crate::Error,
        provider: &str,
        data: &Value,
    ) -> crate::Error {
        let secrets: Vec<String> = self
            .credentials
            .as_object()
            .into_iter()
            .flat_map(|v| v.values())
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        error
            .with_refusal(refusal(provider, data))
            .with_provider_error(crate::provider_error::extract(data, &secrets))
    }
    pub(crate) fn guard_submission(&self, req: &mut Request) -> Result<()> {
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
            media_policy: Default::default(),
            submission_deadline: None,
            budget: Default::default(),
            catalog: crate::model_availability::CatalogCache::default(),
            ltx25_cache: std::cell::RefCell::default(),
            wavespeed_cache: std::cell::RefCell::default(),
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
    pub(crate) fn auth(&self, p: &str) -> Result<Vec<(String, String)>> {
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
            "pika" => vec![("X-API-Key".into(), key.into())],
            _ => vec![("authorization".into(), format!("Bearer {key}"))],
        };
        if p == "runway" {
            h.push(("X-Runway-Version".into(), "2024-11-06".into()))
        }
        Ok(h)
    }
    fn provider_json(&self, provider: &str, req: Request) -> Result<Value> {
        self.http.json(req).inspect_err(|error| {
            if provider == "openrouter" {
                let key = self.credentials["OPENROUTER_API_KEY"]
                    .as_str()
                    .unwrap_or("");
                let now = crate::now();
                if error.0 == "http_403" {
                    // A fresh budget check can stop subsequent work, but cannot
                    // prove what caused an ambiguous earlier 403. Preserve that
                    // job's non-retryable classification, including moderation.
                    self.budget.borrow_mut().recheck(self.http, key, now);
                }
                if error.0 == crate::provider_budget::EXHAUSTED {
                    self.budget.borrow_mut().reject(key, now);
                }
            }
        })
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
        let data = self.provider_json(p, req)?;
        if p == "pika" && body.is_some() {
            if data["status"] == "failed" {
                return Err(self.provider_failure(
                    crate::submission::pika_error(data["error"]["code"].as_str().unwrap_or("")),
                    p,
                    &data,
                ));
            }
            need(
                matches!(
                    data["status"].as_str(),
                    Some("queued" | "running" | "completed")
                ),
                "invalid_provider_response",
            )?;
        }
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
                return Err(self.provider_failure(
                    Error(match code {
                        10001 | 10003 => "vivix_auth_rejected",
                        10004 | 10005 => "vivix_billing_required",
                        10008 | 30008 => "vivix_capacity_limited",
                        20004 => "vivix_invalid_parameters",
                        20005 => "vivix_job_not_found",
                        30001 => "vivix_content_rejected",
                        50001 | 50003 => "vivix_unavailable",
                        _ => "provider_rejected",
                    }),
                    p,
                    &data,
                ));
            }
            need(data["data"].is_object(), "invalid_provider_response")?;
            return Ok(data["data"].clone());
        }
        if p == "minimax" && data["base_resp"]["status_code"] != 0 {
            return Err(self.provider_failure(Error("provider_rejected"), p, &data));
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
                &self.media_policy.hosts(
                    "vivix",
                    &["static.vivi-x.ai", "clipgen-us.vivi-x.ai"],
                    crate::now(),
                ),
            )?;
            urls.push(url.into());
        }
        let (path, body) = crate::studio_video::body(spec, &urls)?;
        let result = self.api(p, &path, Some(&body))?;
        let r = match p {
            "fal" => reference(&result["request_id"])?,
            "openrouter" => reference(&result["id"])?,
            "pika" => reference(&result["id"])?,
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
        let data = self.provider_json(p, req)?;
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
    fn google_read(&self, r: &str, spec: &Value, m: &Value) -> Result<Value> {
        let id = crate::google::reference_id(spec, r)?;
        let mut req = Request::get(
            format!("{}/v1beta/interactions/{id}", crate::google::ORIGIN),
            &["generativelanguage.googleapis.com"],
        );
        req.headers = self.auth("google")?;
        req.limit = crate::google::JSON_LIMIT;
        req.timeout = 60;
        let data = self.http.json(req)?;
        crate::google::interaction_id(&data, m, Some(id))?;
        // spec was validated by context; binding the model also prevents
        // reusing an interaction from a different signed model contract.
        models::check(spec)?;
        Ok(data)
    }
    fn google_references(&self, spec: &Value) -> Result<Vec<Value>> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use sha2::Digest;
        let mut out = vec![];
        let mut references: Vec<_> = spec["input"]["references"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        references.sort_by_key(|r| match r["role"].as_str() {
            Some("first") => 0,
            Some("last") => 1,
            _ => 2,
        });
        for r in references {
            let mut req = Request::get(
                s(&r["url"])?,
                &["api.everyframe.studio", "app.everyframe.studio"],
            );
            req.limit = crate::n(&r["bytes"])? as usize;
            req.timeout = 180;
            let bytes = self.http.bytes(req)?;
            need(
                bytes.len() == crate::n(&r["bytes"])? as usize && sha(&bytes) == s(&r["sha256"])?,
                "reference_integrity_mismatch",
            )?;
            let boundary = format!("everyframe-{}", id());
            let metadata =
                json!({"file":{"display_name":format!("everyframe-{}",&s(&r["sha256"])?[..16])}});
            let mut payload=format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: {}\r\n\r\n",s(&r["mime"])?).into_bytes();
            payload.extend_from_slice(&bytes);
            payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let mut req = Request::get(
                format!(
                    "{}/upload/v1beta/files?uploadType=multipart",
                    crate::google::ORIGIN
                ),
                &["generativelanguage.googleapis.com"],
            );
            req.method = "POST".into();
            req.headers = self.auth("google")?;
            req.headers.push((
                "content-type".into(),
                format!("multipart/related; boundary={boundary}"),
            ));
            req.body = Some(payload);
            req.timeout = 180;
            self.guard_submission(&mut req)?;
            let mut file = self.http.json(req)?["file"].clone();
            let name = s(&file["name"])?.to_string();
            need(
                crate::matches("files/[A-Za-z0-9_-]+", &json!(name)),
                "invalid_provider_ref",
            )?;
            let deadline = crate::now() + 60000;
            while file["state"] == "PROCESSING" {
                need(crate::now() < deadline, "provider_timeout")?;
                std::thread::sleep(std::time::Duration::from_millis(500));
                file = self.api("google", &format!("/v1beta/{name}"), None)?;
                need(file["name"] == name, "provider_id_mismatch")?;
            }
            need(file["state"] == "ACTIVE", "reference_upload_failed")?;
            if let Some(mime) = file.get("mimeType") {
                need(mime == &r["mime"], "reference_integrity_mismatch")?;
            }
            if let Some(size) = file.get("sizeBytes") {
                need(
                    size.as_str()
                        .and_then(|v| v.parse::<i64>().ok())
                        .or_else(|| size.as_i64())
                        == r["bytes"].as_i64(),
                    "reference_integrity_mismatch",
                )?;
            }
            if let Some(hash) = file.get("sha256Hash") {
                need(
                    STANDARD.decode(s(hash)?).ok() == Some(sha2::Sha256::digest(&bytes).to_vec()),
                    "reference_integrity_mismatch",
                )?;
            }

            let uri = s(&file["uri"])?;
            let parsed = safe_url(uri, &["generativelanguage.googleapis.com".into()])?;
            need(
                parsed.path() == format!("/v1beta/{name}") && parsed.query().is_none(),
                "invalid_reference_origin",
            )?;
            out.push(json!({"type":if ["first","last"].contains(&s(&r["role"])?){"image"}else{s(&r["role"])?},"mime_type":r["mime"],"uri":uri}));
        }
        Ok(out)
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
            "pika" => {
                let d = self.api(p, &format!("/v1/media/jobs/{r}"), None)?;
                need(d["id"] == r, "provider_id_mismatch")?;
                // Pika uses the selected route as model_id. Bind it whenever
                // returned, including the keyframe/reference variant.
                let (path, _) = crate::studio_video::body(
                    spec,
                    &spec["input"]["references"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|a| a["url"].as_str().unwrap().to_string())
                        .collect::<Vec<_>>(),
                )?;
                need(
                    d["model_id"].is_null()
                        || d["model_id"] == path.trim_start_matches("/v1/media/"),
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
        if p == "bfl" {
            return crate::bfl::poll_status(&d["status"]);
        }
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
            "pika" => (&["queued"], &["running"], &["completed"], &["failed"]),
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
        provider: &str,
        url: &str,
        hosts: &[&str],
        auth: Option<&str>,
        redirects: &[&str],
    ) -> Result<Vec<u8>> {
        self.download_bounded(provider, url, hosts, auth, redirects, MAX_MEDIA)
    }
    pub(crate) fn download_bounded(
        &self,
        provider: &str,
        url: &str,
        hosts: &[&str],
        auth: Option<&str>,
        redirects: &[&str],
        limit: usize,
    ) -> Result<Vec<u8>> {
        crate::destination_diagnostics::clear();
        // Policy applies only to output delivery, never API calls. Authenticated
        // initial downloads retain their compiled API origin and path guards.
        let effective = self.media_policy.hosts(provider, hosts, crate::now());
        let mut r = Request::get(url, hosts);
        if auth.is_none() {
            r.hosts = effective.clone();
        }
        safe_url(url, &r.hosts)?;
        r.limit = limit;
        r.timeout = 180;
        r.redirect_hosts = if auth.is_none() && self.media_policy.active(provider, crate::now()) {
            effective
        } else if !redirects.is_empty() {
            self.media_policy.hosts(provider, redirects, crate::now())
        } else {
            vec![]
        };
        if let Some(p) = auth {
            r.headers = self.auth(p)?
        };
        let b = self.http.bytes(r)?;
        need(!b.is_empty() && b.len() <= limit, "invalid_media_size")?;
        Ok(b)
    }
}
impl Provider for Router<'_> {
    fn set_media_policy(&mut self, policy: &Value, chain: &Value, now: i64) -> Result<()> {
        self.media_policy.install(policy, chain, now)
    }
    fn refresh_availability(&mut self, now: i64) -> Option<Value> {
        let mut rows = self.catalog.refresh(self.http, &self.credentials, now);
        if configured(&self.credentials["OPENROUTER_API_KEY"]) {
            self.budget.borrow_mut().refresh(
                self.http,
                self.credentials["OPENROUTER_API_KEY"]
                    .as_str()
                    .unwrap_or(""),
                now,
            );
            if !self.budget.borrow().allows(now) {
                for row in rows.as_array_mut().into_iter().flatten() {
                    if models::info(row["model"].as_str().unwrap_or(""))
                        .is_ok_and(|m| m["provider"] == "openrouter")
                        && !matches!(row["status"].as_str(), Some("removed" | "deprecated"))
                    {
                        row["status"] = json!("access_denied");
                    }
                }
            }
        }
        Some(rows)
    }
    fn provider_budget(&self) -> Option<Value> {
        self.budget.borrow().proof()
    }
    fn set_retirements(&mut self, retired: &Value) {
        self.catalog.set_server_retirements(retired);
    }
    fn submit_before(&mut self, spec: &Value, deadline: i64) -> Result<String> {
        let (_, provider) = self.context(spec)?;
        if provider == "openrouter" {
            need(
                self.catalog.allows(s(&spec["model"])?),
                "provider_model_unavailable",
            )?;
            need(crate::now() < deadline, "submission_deadline_elapsed")?;
            self.budget.borrow_mut().guard(
                self.http,
                s(&self.credentials["OPENROUTER_API_KEY"])?,
                crate::now(),
            )?;
        }
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
        if provider == "openrouter"
            && result
                .as_ref()
                .is_err_and(|e| e.0 == crate::provider_budget::EXHAUSTED)
        {
            self.budget
                .borrow_mut()
                .reject(s(&self.credentials["OPENROUTER_API_KEY"])?, crate::now());
        }
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
                    && (m["provider"] != "openrouter"
                        || self.budget.borrow().proof().is_none()
                        || self
                            .budget
                            .borrow()
                            .proof()
                            .is_some_and(|v| v["state"] == "ready"))
                    && (m["provider"] != "alibaba"
                        || crate::invitation::valid_workspace(
                            &self.credentials[crate::invitation::ALIBABA_WORKSPACE_KEY],
                        ))
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
        if m["nativeMedia"] == true {
            return self.native_submit(spec, m);
        }
        if p == "google" && m["interaction"] == true {
            let references = self.google_references(spec)?;
            let body = crate::google::body_with_references(spec, m, &references)?;
            let mut req = Request::get(
                format!("{}/v1beta/interactions", crate::google::ORIGIN),
                &["generativelanguage.googleapis.com"],
            )
            .json("POST", &body);
            req.headers.extend(self.auth("google")?);
            req.limit = crate::google::JSON_LIMIT;
            req.timeout = 60;
            self.guard_submission(&mut req)?;
            let data = self.http.json(req)?;
            let id = crate::google::interaction_id(&data, m, None)?;
            return crate::google::reference(spec, &id);
        }
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
                reference(&json!(format!(
                    "bfl_{}",
                    crate::bfl::submission_reference(&d)?
                )))?
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
                let url = if model == "eleven_v4" {
                    format!(
                        "{}/v1/text-to-dialogue?output_format={}",
                        prefix(&p),
                        s(&input["output_format"])?
                    )
                } else {
                    format!(
                        "{}/v1/text-to-speech/{}?output_format={}",
                        prefix(&p),
                        s(&input["voice_id"])?,
                        s(&input["output_format"])?
                    )
                };
                let body = if model == "eleven_v4" {
                    json!({"inputs":[{"text":input["prompt"],"voice_id":input["voice_id"]}],"model_id":model})
                } else {
                    json!({"text":input["prompt"],"model_id":model})
                };
                let mut r = Request::get(url, &["api.elevenlabs.io"]).json("POST", &body);
                r.headers.extend(self.auth(&p)?);
                r.limit = 10 * 1024 * 1024;
                r.timeout = 60;
                self.guard_submission(&mut r)?;
                let media = if model == "eleven_v4_turbo" {
                    let mut ws = Request::get(
                        format!(
                            "https://api.elevenlabs.io/v1/text-to-dialogue/stream-input?model_id={model}&output_format={}",
                            s(&input["output_format"])?
                        ),
                        &["api.elevenlabs.io"],
                    );
                    ws.headers = self.auth(&p)?;
                    ws.limit = r.limit;
                    ws.timeout = r.timeout;
                    self.guard_submission(&mut ws)?;
                    self.http.dialogue(
                        ws,
                        &json!([input["voice_id"]]),
                        &json!([{"text":input["prompt"],"voice_id":input["voice_id"]}]),
                    )?
                } else {
                    self.http.bytes(r)?
                };
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
        if m["nativeMedia"] == true {
            return self.native_poll(r, spec, m);
        }
        if p == "google" && m["interaction"] == true {
            let data = self.google_read(r, spec, m)?;
            let state = crate::google::state(&data)?;
            if state == "FAILED" {
                return Err(self.provider_failure(Error(failure_category(&p, &data)), &p, &data));
            }
            return Ok(state.into());
        }
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
            // from a billing or provider fault. Diagnostic text is separately redacted.
            return Err(self.provider_failure(Error(failure_category(&p, &data)), &p, &data));
        }
        Ok(state.into())
    }
    fn result(&mut self, r: &str, spec: &Value) -> Result<Output> {
        let (m, p) = self.context(spec)?;
        if m["nativeMedia"] == true {
            return self.native_result(r, spec, m);
        }
        if p == "google" && m["interaction"] == true {
            let data = self.google_read(r, spec, m)?;
            let expected = crate::google::output_model(spec, m)?;
            let blocks = crate::google::media_blocks(&data, &expected)?;
            let bundled = blocks.len() > 1 || spec["input"]["response_format"].is_array();
            let mut parts = vec![];
            let mut media = vec![];
            for block in blocks {
                let bytes = if block["data"].is_string() {
                    crate::google::inline(block)?
                } else {
                    self.download(
                        &p,
                        &crate::google::download_url(block)?,
                        &["generativelanguage.googleapis.com"],
                        Some("google"),
                        &[
                            "generativelanguage.googleapis.com",
                            "*.googleusercontent.com",
                            "storage.googleapis.com",
                        ],
                    )?
                };
                let bytes = crate::google::normalize_media(block, &expected, bytes)?;
                if bundled {
                    parts.push(json!({"type":expected["kind"],"mime":expected["mime"],"data":crate::protocol::b64(&bytes)}));
                } else {
                    media = bytes;
                }
            }
            if bundled {
                let text: Vec<_> = data["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|s| s["type"] == "model_output")
                    .filter_map(|s| s["content"].as_array())
                    .flatten()
                    .filter(|c| c["type"] == "text")
                    .filter_map(|c| c["text"].as_str())
                    .collect();
                media = json!({"kind":"google-media-bundle","version":1,"parts":parts,"text":text})
                    .to_string()
                    .into_bytes();
                need(media.len() <= MAX_MEDIA, "invalid_media_size")?;
            }
            let response_hash = crate::google::response_hash(spec, &data, &media)?;
            return Ok(Output {
                media,
                response_hash,
            });
        }
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
            if !(data["error"].is_null() && data["detail"].is_null()) {
                return Err(self.provider_failure(Error(failure_category(&p, &data)), &p, &data));
            }
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
            let media = self.download(&p, url, &["*.fal.media"], None, &[])?;
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
            "pika" => {
                need(
                    data["deleted_at"].is_null()
                        && data["error"].is_null()
                        && data["output"]["media_type"] == "video",
                    "provider_no_media",
                )?;
                self.download_bounded(
                    &p,
                    s(&data["output"]["video"]["url"])?,
                    &["cdn.pika.art"],
                    None,
                    &[],
                    128 * 1024 * 1024,
                )?
            }
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
                self.download(&p, s(&outputs[0]["url"])?, &["*.klingai.com"], None, &[])?
            }
            // Bundled fallback; fresh signed policy controls reviewed delivery hosts.
            "vivix" => self.download(
                &p,
                s(&data["video_url"])?,
                &["static.vivi-x.ai", "clipgen-us.vivi-x.ai"],
                None,
                &[],
            )?,
            "minimax" => {
                let file_id = minimax_file_id(&data["file_id"])?;
                let file = self.api(&p, &format!("/v1/files/retrieve?file_id={file_id}"), None)?;
                need(
                    minimax_file_id(&file["file"]["file_id"])? == file_id,
                    "provider_file_mismatch",
                )?;
                let media = self.download(
                    &p,
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
                &p,
                &format!("{}/api/v1/videos/{r}/content?index=0", prefix(&p)),
                &["openrouter.ai"],
                Some(&p),
                &[],
            )?,
            "bfl" => {
                let raw = s(&data["result"]["sample"])?;
                let delivery = self.media_policy.delivery_url(
                    "bfl",
                    raw,
                    crate::bfl::delivery_url(raw),
                    crate::now(),
                )?;
                self.download(
                    &p,
                    delivery.as_str(),
                    &[delivery.host_str().unwrap()],
                    None,
                    &[],
                )?
            }
            "replicate" => self.download(
                &p,
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
                    &p,
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
                    &p,
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
                &p,
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
/// coordinator accepts. Diagnostic wording is reported separately and never changes it.
pub fn failure_category(provider: &str, data: &Value) -> &'static str {
    if provider == "wavespeed" {
        return match crate::wavespeed::error(data["code"].as_i64().unwrap_or(1403)).0 {
            "wavespeed_auth_rejected" => "provider_auth_rejected",
            "wavespeed_billing_required" => "provider_billing_required",
            "wavespeed_capacity_limited" => "provider_capacity_limited",
            "wavespeed_content_rejected" => "provider_content_rejected",
            "wavespeed_invalid_parameters" => "provider_invalid_request",
            _ => "provider_generation_failed",
        };
    }
    if provider == "pika" {
        // Only documented machine codes classify Pika errors. Prompt-influenced
        // message prose must not change moderation or failover decisions.
        return match crate::submission::pika_error(data["error"]["code"].as_str().unwrap_or("")).0 {
            "pika_billing_required" => "provider_billing_required",
            "pika_capacity_limited" => "provider_capacity_limited",
            "pika_content_rejected" => "provider_content_rejected",
            "pika_invalid_parameters" => "provider_invalid_request",
            _ => "provider_generation_failed",
        };
    }
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

/// Why a provider refused a job, as one of the coordinator's fixed refusal codes
/// (real_person, copyright, reference_too_small, prompt_too_long, content_policy).
/// This code never changes the failure category, so
/// pausing and failover decide exactly as before. Matching reads the provider's
/// error fields only (never request echoes such as input or loc); an error that
/// quotes the prompt can at worst pick a different fixed customer message.
pub fn refusal(provider: &str, data: &Value) -> Option<&'static str> {
    if provider == "wavespeed" && data["code"] == 1200 {
        return Some("content_policy");
    }
    if provider == "pika" {
        // Pika: documented machine codes only, as in failure_category.
        return match data["error"]["code"].as_str() {
            Some("content_moderation") => Some("content_policy"),
            _ => None,
        };
    }
    let mut codes = Vec::new();
    let mut messages = Vec::new();
    failure_fields(data, 0, &mut codes, &mut messages);
    // OpenRouter relays the upstream provider's own error as a string here.
    for raw in [&data["error"]["metadata"]["raw"], &data["metadata"]["raw"]] {
        if let Some(text) = raw.as_str() {
            messages.push(text.chars().take(4096).collect::<String>().to_lowercase());
        }
    }
    let code = |words: &[&str]| codes.iter().any(|c| words.iter().any(|w| c.contains(w)));
    let text = messages.join(" ");
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if code(&["privacyinformation"])
        || has(&[
            "privacyinformation",
            "real person",
            "real people",
            "real human",
            "likeness",
            "celebrit",
            "public figure",
        ])
    {
        Some("real_person")
    } else if code(&["copyright"]) || has(&["copyright"]) {
        Some("copyright")
    } else if has(&["image", "video", "reference"])
        && has(&[
            "pixel count",
            "too small",
            "resolution is too low",
            "minimum resolution",
        ])
    {
        Some("reference_too_small")
    } else if code(&["string_too_long"])
        || has(&["prompt"]) && has(&["too long", "exceeds the maximum", "maximum length"])
    {
        Some("prompt_too_long")
    } else if code(&[
        "content_policy",
        "content_blocked",
        "moderation",
        "safety",
        "nsfw",
        "sensitive",
    ]) || has(&[
        "content_policy",
        "content policy",
        "content checker",
        "moderat",
        "nsfw",
        "safety system",
        "sensitive",
        "prohibited",
        "flagged",
    ]) {
        Some("content_policy")
    } else {
        None
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
/// classification work. Reportable text is separately bounded and redacted.
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
