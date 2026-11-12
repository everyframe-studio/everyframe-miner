//! Reviewed direct-provider media requests. Provider keys and endpoint URLs
//! never come from a job; references remain content-addressed Studio assets.
use crate::{Error, Result, matches, need, protocol::exact, s};
use serde_json::{Value, json};
use std::sync::LazyLock;

pub static MODELS: LazyLock<Value> = LazyLock::new(|| {
    let mut models: Value = serde_json::from_str(include_str!("../config/native-media.json"))
        .expect("reviewed native media models");
    let additions: Value =
        serde_json::from_str(include_str!("../config/native-schema-models.json"))
            .expect("reviewed native schema models");
    models
        .as_object_mut()
        .unwrap()
        .extend(additions.as_object().unwrap().clone());
    let gpt: Value = serde_json::from_str(include_str!("../config/gpt-image-26.json"))
        .expect("reviewed GPT Image 2.6 models");
    models
        .as_object_mut()
        .unwrap()
        .extend(gpt.as_object().unwrap().clone());
    let wavespeed: Value = serde_json::from_str(include_str!("../config/wavespeed-models.json"))
        .expect("reviewed WaveSpeed media models");
    for (id, model) in wavespeed.as_object().unwrap() {
        assert!(
            models.get(id).is_none(),
            "WaveSpeed must not replace a signed contract"
        );
        models[id] = model.clone();
    }
    let asian: Value = serde_json::from_str(include_str!("../config/asian-providers.json"))
        .expect("reviewed Asian provider models");
    for (id, model) in asian.as_object().unwrap() {
        assert!(
            models.get(id).is_none(),
            "new providers must not replace existing contracts"
        );
        models[id] = model.clone();
    }
    let ltx: Value = serde_json::from_str(include_str!("../config/ltx25-models.json"))
        .expect("reviewed LTX 2.5 models");
    for (id, model) in ltx.as_object().unwrap() {
        assert!(
            models.get(id).is_none(),
            "LTX must not replace existing contracts"
        );
        models[id] = model.clone();
    }
    let h3: Value = serde_json::from_str(include_str!("../config/h3-image.json"))
        .expect("reviewed H3 Image models");
    for (id, m) in h3.as_object().unwrap() {
        assert!(models.get(id).is_none());
        models[id] = m.clone();
    }
    models
});

pub fn reference(spec: &Value, id: &str) -> Result<String> {
    need(
        matches("[A-Za-z0-9_-]{1,100}", &json!(id)),
        "invalid_provider_ref",
    )?;
    Ok(format!(
        "native_{}_{}",
        &crate::protocol::digest(spec)?[..24],
        id
    ))
}
pub fn reference_id<'a>(spec: &Value, reference: &'a str) -> Result<&'a str> {
    let prefix = format!("native_{}_", &crate::protocol::digest(spec)?[..24]);
    let id = reference
        .strip_prefix(&prefix)
        .ok_or(Error("provider_ref_namespace"))?;
    need(
        matches("[A-Za-z0-9_-]{1,100}", &json!(id)),
        "invalid_provider_ref",
    )?;
    Ok(id)
}

pub fn state(spec: &Value, id: &str, data: &Value) -> Result<&'static str> {
    check(spec)?;
    if let Some(returned) = data.get("request_id") {
        need(returned == id, "provider_id_mismatch")?;
    }
    let model = &MODELS[s(&spec["model"])?];
    if let Some(returned) = data.get("model").filter(|v| !v.is_null()) {
        need(returned == &model["apiModel"], "provider_model_mismatch")?;
    }
    match data["status"].as_str() {
        Some("pending" | "queued" | "running") => Ok("IN_PROGRESS"),
        Some("done") if data["error"].is_null() => Ok("COMPLETED"),
        Some("failed" | "expired") => Ok("FAILED"),
        _ => Err(Error("invalid_provider_status")),
    }
}

fn integer(v: &Value, min: i64, max: i64) -> bool {
    v.as_i64().is_some_and(|x| (min..=max).contains(&x))
}
fn number(v: &Value, min: f64, max: f64) -> bool {
    decimal(v).is_some_and(|x| x.is_finite() && x >= min && x <= max)
}
fn decimal(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok())
}
fn canonical_numbers(v: &mut Value) {
    match v {
        Value::Number(n) if !n.is_i64() && !n.is_u64() => {
            let number = n.as_f64().unwrap();
            *v = if number.fract() == 0.0 && number.abs() <= 9007199254740991.0 {
                json!(number as i64)
            } else {
                json!(n.to_string())
            };
        }
        Value::Array(a) => a.iter_mut().for_each(canonical_numbers),
        Value::Object(o) => o.values_mut().for_each(canonical_numbers),
        _ => (),
    }
}
fn listed(v: &Value, options: &Value) -> bool {
    options.as_array().is_some_and(|a| a.contains(v))
}
fn optional_fields(v: &Value, fields: &[&str]) -> Result<()> {
    need(
        v.as_object()
            .is_some_and(|o| o.keys().all(|k| fields.contains(&k.as_str()))),
        "invalid_native_media_fields",
    )
}

pub fn spec(value: &Value) -> Result<Value> {
    let model = s(&value["model"])?;
    let policy = &MODELS[model];
    need(policy.is_object(), "unsupported_model")?;
    optional_fields(value, &["model", "prompt", "parameters", "references"])?;
    let mut parameters = policy["params"].clone();
    if let Some(provided) = value.get("parameters") {
        for (k, v) in provided
            .as_object()
            .ok_or(Error("invalid_native_media_fields"))?
        {
            // Nested format objects accept partial overrides just like the
            // top-level options. Unknown keys are still rejected by check.
            if parameters[k].is_object() && v.is_object() {
                for (field, value) in v.as_object().unwrap() {
                    parameters[k][field] = value.clone();
                }
            } else {
                parameters[k] = v.clone();
            }
        }
    }
    if policy["h3Image"] == true
        && (value["parameters"].get("area").is_some()
            || value["parameters"].get("aspect_ratio").is_some())
        && value["parameters"].get("size").is_none()
    {
        parameters["size"] = json!("auto");
    }
    if policy["gptImage26"] == true
        && policy["provider"] != "openrouter"
        && parameters["output_format"] == "gif"
        && value["parameters"].get("size").is_none()
    {
        parameters["size"] = json!("auto");
    }
    if parameters["output_format"].is_object()
        && parameters["output_format"]["codec"] != "mp3"
        && value["parameters"]["output_format"]
            .get("bit_rate")
            .is_none()
    {
        parameters["output_format"]
            .as_object_mut()
            .unwrap()
            .remove("bit_rate");
    }
    let mut result = json!({"model":model,"input":{"prompt":value["prompt"],"parameters":parameters,
        "references":value.get("references").cloned().unwrap_or(json!([]))}});
    canonical_numbers(&mut result);
    check(&result)?;
    Ok(result)
}

pub fn check(value: &Value) -> Result<()> {
    exact(value, &["model", "input"])?;
    let policy = &MODELS[s(&value["model"])?];
    need(policy.is_object(), "unsupported_model")?;
    let input = &value["input"];
    exact(input, &["prompt", "parameters", "references"])?;
    let prompt = s(&input["prompt"])?;
    need(
        !prompt.trim().is_empty()
            && prompt.chars().count() <= policy["maxPrompt"].as_u64().unwrap() as usize,
        "invalid_native_media_prompt",
    )?;
    let p = &input["parameters"];
    let c = &policy["nativeCapabilities"];
    let references = input["references"]
        .as_array()
        .ok_or(Error("invalid_native_media_references"))?;
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut total_bytes = 0u64;
    for reference in references {
        optional_fields(
            reference,
            &[
                "url",
                "sha256",
                "bytes",
                "mime",
                "role",
                "width",
                "height",
                "durationMs",
                "frameCount",
                "timestampSeconds",
            ],
        )?;
        let role = s(&reference["role"])?;
        need(
            [
                "image", "first", "last", "keyframe", "mask", "markup", "audio", "video",
            ]
            .contains(&role),
            "invalid_reference_role",
        )?;
        need(
            matches(
                r"https://(api|app)\.everyframe\.studio/(api/)?v1/attachment/att_[A-Za-z0-9_-]+\?t=[A-Za-z0-9_-]{20,100}",
                &reference["url"],
            ) && s(&reference["url"])?.len() <= 1000
                && matches("[a-f0-9]{64}", &reference["sha256"]),
            "invalid_reference_origin",
        )?;
        let bytes = reference["bytes"]
            .as_u64()
            .ok_or(Error("invalid_reference_size"))?;
        need(
            bytes > 0
                && bytes
                    <= if policy["provider"] == "kling" {
                        300 * 1024 * 1024
                    } else {
                        128 * 1024 * 1024
                    },
            "invalid_reference_size",
        )?;
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or(Error("invalid_reference_size"))?;
        need(total_bytes <= 512 * 1024 * 1024, "invalid_reference_size")?;
        let mime = s(&reference["mime"])?;
        need(
            match role {
                "audio" => {
                    ["audio/wav", "audio/x-wav", "audio/mpeg"].contains(&mime)
                        || policy["ltx25"] == true && ["audio/mp4", "audio/ogg"].contains(&mime)
                }
                "video" => {
                    mime == "video/mp4"
                        || (policy["adapter"] == "schema-minimax-video"
                            || policy["provider"] == "kling")
                            && mime == "video/quicktime"
                }
                _ => {
                    ["image/png", "image/jpeg", "image/webp"].contains(&mime)
                        || policy["adapter"] == "schema-minimax-video"
                            && ["image/heic", "image/heif"].contains(&mime)
                }
            },
            "invalid_reference_type",
        )?;
        *counts.entry(role).or_default() += 1;
    }
    let count = |role| *counts.get(role).unwrap_or(&0);
    if policy["h3Image"] == true {
        return crate::h3_image::check(p, policy, references);
    }
    if policy["nativeSchema"].is_object() {
        if policy["adapter"] == "schema-bfl" {
            need(
                references.iter().all(|r| {
                    if policy["kind"] == "image" {
                        matches!(r["role"].as_str(), Some("image" | "mask"))
                    } else {
                        matches!(
                            r["role"].as_str(),
                            Some("image" | "first" | "last" | "keyframe" | "video")
                        )
                    }
                }),
                "invalid_native_media_references",
            )?;
        }
        let urls = references
            .iter()
            .map(|r| s(&r["url"]).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        crate::native_schema::body(value, policy, &urls)?;
        return Ok(());
    }
    if policy["asianMedia"] == true {
        return crate::asian_media::check(value, policy);
    }
    if policy["gptImage26"] == true {
        return crate::gpt_image26::check(p, policy, references);
    }
    match s(&policy["adapter"])? {
        "openai-images" => {
            optional_fields(
                p,
                &[
                    "size",
                    "quality",
                    "background",
                    "output_format",
                    "output_compression",
                    "n",
                    "model_version",
                ],
            )?;
            need(
                listed(&p["quality"], &c["qualities"])
                    && listed(&p["output_format"], &c["formats"]),
                "invalid_native_image_format",
            )?;
            need(integer(&p["n"], 1, 10), "invalid_native_image_count")?;
            need(
                ["auto", "transparent", "opaque"].contains(&s(&p["background"])?),
                "invalid_native_image_background",
            )?;
            need(
                p["background"] != "transparent" || p["output_format"] != "jpeg",
                "invalid_native_image_background",
            )?;
            if let Some(compression) = p.get("output_compression") {
                need(
                    p["output_format"] != "png" && integer(compression, 0, 100),
                    "invalid_native_image_compression",
                )?;
            }
            let size = s(&p["size"])?;
            if size != "auto" {
                let (width, height) = size
                    .split_once('x')
                    .ok_or(Error("invalid_native_image_size"))?;
                let width = width
                    .parse::<u64>()
                    .map_err(|_| Error("invalid_native_image_size"))?;
                let height = height
                    .parse::<u64>()
                    .map_err(|_| Error("invalid_native_image_size"))?;
                need(
                    width > 0
                        && height > 0
                        && width <= 3840
                        && height <= 3840
                        && width % 16 == 0
                        && height % 16 == 0
                        && (655360..=8294400).contains(&(width * height))
                        && width.max(height) <= 3 * width.min(height),
                    "invalid_native_image_size",
                )?;
            }
            if let Some(version) = p.get("model_version") {
                need(listed(version, &c["versions"]), "unsupported_model_version")?;
            }
            for reference in references
                .iter()
                .filter(|r| r["role"] == "mask" || r["role"] == "markup")
            {
                need(reference["mime"] == "image/png", "invalid_reference_type")?;
            }
            need(
                count("image") <= 16
                    && count("mask") <= 1
                    && count("markup") <= usize::from(c["markup"] == true)
                    && count("mask") + count("markup") <= 1
                    && references.len() == count("image") + count("mask") + count("markup")
                    && (count("mask") + count("markup") == 0 || count("image") > 0),
                "invalid_native_media_references",
            )?;
        }
        "xai-images" => {
            optional_fields(p, &["resolution", "aspect_ratio", "n", "quality"])?;
            need(
                listed(&p["resolution"], &c["resolutions"])
                    && listed(&p["aspect_ratio"], &c["aspects"])
                    && integer(&p["n"], 1, 10),
                "invalid_native_image_format",
            )?;
            if let Some(quality) = p.get("quality") {
                need(
                    listed(quality, &c["qualities"]),
                    "invalid_native_image_quality",
                )?;
            }
            need(
                references.len() == count("image")
                    && count("image") <= c["maxReferences"].as_u64().unwrap() as usize,
                "invalid_native_media_references",
            )?;
        }
        "xai-videos" => {
            optional_fields(
                p,
                &[
                    "duration",
                    "resolution",
                    "aspect_ratio",
                    "task",
                    "reference_voices",
                ],
            )?;
            need(
                integer(&p["duration"], 1, 15)
                    && listed(&p["resolution"], &c["resolutions"])
                    && listed(&p["aspect_ratio"], &c["aspects"])
                    && listed(&p["task"], &c["tasks"]),
                "invalid_native_video_format",
            )?;
            let voices = p
                .get("reference_voices")
                .map(|v| v.as_array().ok_or(Error("invalid_native_video_voices")))
                .transpose()?;
            let voice_count = voices.map_or(0, Vec::len);
            if let Some(voices) = voices {
                for voice in voices {
                    need(
                        matches("[A-Za-z0-9_-]{1,100}", voice),
                        "invalid_native_video_voices",
                    )?;
                }
            }
            need(
                count("image") <= c["maxReferences"].as_u64().unwrap() as usize
                    && count("first") <= 1
                    && count("last") <= usize::from(c["lastImage"] == true)
                    && count("keyframe") <= c["keyframes"].as_u64().unwrap() as usize
                    && count("audio") + voice_count
                        <= c["referenceVoices"].as_u64().unwrap() as usize
                    && count("mask") + count("markup") == 0,
                "invalid_native_media_references",
            )?;
            for r in references.iter().filter(|r| r["role"] == "keyframe") {
                need(
                    number(
                        &r["timestampSeconds"],
                        f64::EPSILON,
                        p["duration"].as_f64().unwrap() - f64::EPSILON,
                    ),
                    "invalid_native_keyframe",
                )?;
            }
            let times: Vec<_> = references
                .iter()
                .filter(|r| r["role"] == "keyframe")
                .map(|r| decimal(&r["timestampSeconds"]).unwrap())
                .collect();
            need(
                times.windows(2).all(|w| w[0] < w[1]),
                "invalid_native_keyframe",
            )?;
            if p["task"] == "generate" {
                need(count("video") == 0, "invalid_native_media_references")?;
                if count("image") + count("audio") + voice_count > 0 {
                    need(
                        p["resolution"] == c["referenceResolution"]
                            && p["duration"].as_u64().unwrap()
                                <= c["referenceDurationMax"].as_u64().unwrap(),
                        "invalid_native_reference_format",
                    )?;
                }
            } else {
                need(
                    count("video") == 1 && references.len() == 1 && voice_count == 0,
                    "invalid_native_media_references",
                )?;
                let r = &references[0];
                need(
                    integer(
                        &r["durationMs"],
                        1,
                        if p["task"] == "edit" { 8700 } else { 15000 },
                    ),
                    "invalid_native_video_source",
                )?;
                if p["task"] == "extend" {
                    need(
                        integer(&p["duration"], 2, 10),
                        "invalid_native_video_duration",
                    )?;
                }
            }
        }
        "openai-audio" => {
            optional_fields(
                p,
                &[
                    "audio",
                    "max_completion_tokens",
                    "temperature",
                    "top_p",
                    "frequency_penalty",
                    "presence_penalty",
                ],
            )?;
            exact(&p["audio"], &["voice", "format"])?;
            let voice = &p["audio"]["voice"];
            if voice.is_object() {
                exact(voice, &["id"])?;
                need(
                    matches("[A-Za-z0-9_-]{1,100}", &voice["id"]),
                    "invalid_native_audio_voice",
                )?;
            } else {
                need(listed(voice, &c["voices"]), "invalid_native_audio_voice")?;
            }
            need(
                listed(&p["audio"]["format"], &c["formats"])
                    && integer(&p["max_completion_tokens"], 1, 16384),
                "invalid_native_audio_format",
            )?;
            for (field, min, max) in [
                ("temperature", 0., 2.),
                ("top_p", 0., 1.),
                ("frequency_penalty", -2., 2.),
                ("presence_penalty", -2., 2.),
            ] {
                if let Some(v) = p.get(field) {
                    need(number(v, min, max), "invalid_native_audio_parameter")?;
                }
            }
            need(
                references.len() == count("audio"),
                "invalid_native_media_references",
            )?;
        }
        "xai-tts" => {
            optional_fields(
                p,
                &[
                    "voice_id",
                    "language",
                    "output_format",
                    "speed",
                    "optimize_streaming_latency",
                    "text_normalization",
                    "with_timestamps",
                    "replace",
                ],
            )?;
            need(
                matches("[A-Za-z0-9_-]{1,100}", &p["voice_id"])
                    && matches("(auto|[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*)", &p["language"]),
                "invalid_native_audio_voice",
            )?;
            optional_fields(&p["output_format"], &["codec", "sample_rate", "bit_rate"])?;
            need(
                listed(&p["output_format"]["codec"], &c["codecs"])
                    && listed(&p["output_format"]["sample_rate"], &c["sampleRates"])
                    && number(&p["speed"], 0.7, 1.5),
                "invalid_native_audio_format",
            )?;
            if let Some(rate) = p["output_format"].get("bit_rate") {
                need(
                    p["output_format"]["codec"] == "mp3" && listed(rate, &c["bitRates"]),
                    "invalid_native_audio_format",
                )?;
            }
            if let Some(latency) = p.get("optimize_streaming_latency") {
                need(integer(latency, 0, 2), "invalid_native_audio_parameter")?;
            }
            for field in ["text_normalization", "with_timestamps"] {
                if let Some(v) = p.get(field) {
                    need(v.is_boolean(), "invalid_native_audio_parameter")?;
                }
            }
            if let Some(replacements) = p.get("replace") {
                let map = replacements
                    .as_object()
                    .ok_or(Error("invalid_native_audio_replacements"))?;
                need(map.len() <= 200, "invalid_native_audio_replacements")?;
                for (from, to) in map {
                    need(
                        !from.is_empty()
                            && from.chars().count() <= 100
                            && from
                                .chars()
                                .all(|c| c.is_alphanumeric() || c == '\'' || c == ' ')
                            && s(to)?.chars().count() <= 128,
                        "invalid_native_audio_replacements",
                    )?;
                }
                // Replacements can expand the provider input beyond its limit.
                let expanded = map
                    .iter()
                    .try_fold(prompt.to_string(), |text, (from, to)| {
                        let text = text.replace(from, s(to)?);
                        need(
                            text.chars().count() <= 240000,
                            "invalid_native_audio_replacements",
                        )?;
                        Ok(text)
                    })?;
                need(
                    expanded.chars().count() <= 240000,
                    "invalid_native_audio_replacements",
                )?;
            }
            need(references.is_empty(), "invalid_native_media_references")?;
        }
        _ => return Err(Error("unsupported_provider")),
    }
    Ok(())
}

pub fn body(spec: &Value, urls: &[String]) -> Result<(String, Value)> {
    check(spec)?;
    let m = &MODELS[s(&spec["model"])?];
    if m["asianMedia"] == true {
        return Ok((
            s(&m["apiPath"])?.into(),
            crate::asian_media::body(spec, m, urls)?,
        ));
    }
    if m["h3Image"] == true {
        return crate::h3_image::body(spec, m, urls);
    }
    if m["nativeSchema"].is_object() {
        return Ok((
            s(&m["apiPath"])?.to_string(),
            crate::native_schema::body(spec, m, urls)?,
        ));
    }
    if m["gptImage26"] == true {
        return crate::gpt_image26::body(spec, m, urls);
    }
    let x = &spec["input"];
    let refs = x["references"].as_array().unwrap();
    need(refs.len() == urls.len(), "reference_count_mismatch")?;
    let mut body = x["parameters"].clone();
    for field in [
        "speed",
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
    ] {
        if let Some(v) = body.get(field).filter(|v| v.is_string()) {
            body[field] = json!(decimal(v).ok_or(Error("invalid_native_audio_parameter"))?);
        }
    }
    body["model"] = m["apiModel"].clone();
    body["prompt"] = x["prompt"].clone();
    match s(&m["adapter"])? {
        "openai-images" => {
            if let Some(version) = body.as_object_mut().unwrap().remove("model_version") {
                body["model"] = version;
            }
            let images: Vec<_> = refs
                .iter()
                .zip(urls)
                .filter(|(r, _)| r["role"] == "image")
                .map(|(_, url)| json!({"image_url":url}))
                .collect();
            for role in ["mask", "markup"] {
                if let Some((_, url)) = refs.iter().zip(urls).find(|(r, _)| r["role"] == role) {
                    body[role] = json!({"image_url":url});
                }
            }
            if images.is_empty() {
                Ok(("/v1/images/generations".into(), body))
            } else {
                body["images"] = json!(images);
                Ok(("/v1/images/edits".into(), body))
            }
        }
        "xai-images" => {
            body["deferred"] = json!(true);
            body["response_format"] = json!("url");
            if refs.is_empty() {
                Ok(("/v1/images/generations".into(), body))
            } else {
                body["images"] = json!(
                    urls.iter()
                        .map(|url| json!({"url":url}))
                        .collect::<Vec<_>>()
                );
                Ok(("/v1/images/edits".into(), body))
            }
        }
        "xai-videos" => {
            let task = body.as_object_mut().unwrap().remove("task").unwrap();
            let voices = body
                .as_object_mut()
                .unwrap()
                .remove("reference_voices")
                .unwrap_or(json!([]));
            if task != "generate" {
                body["video"] = json!({"url":urls[0]});
                body.as_object_mut().unwrap().remove("aspect_ratio");
                body.as_object_mut().unwrap().remove("resolution");
                if task == "edit" {
                    body.as_object_mut().unwrap().remove("duration");
                }
                return Ok((
                    if task == "edit" {
                        "/v1/videos/edits"
                    } else {
                        "/v1/videos/extensions"
                    }
                    .into(),
                    body,
                ));
            }
            let mut images = Vec::new();
            let mut audios = Vec::new();
            let mut frames = Vec::new();
            for (r, url) in refs.iter().zip(urls) {
                match s(&r["role"])? {
                    "image" => images.push(json!({"url":url})),
                    "audio" => audios.push(json!({"url":url})),
                    "first" => body["image"] = json!({"url":url}),
                    "last" => body["last_frame"] = json!({"url":url}),
                    "keyframe" => frames.push(json!({"image":{"url":url},"timestamp_s":decimal(&r["timestampSeconds"]).ok_or(Error("invalid_native_keyframe"))?})),
                    _ => return Err(Error("invalid_reference_role")),
                }
            }
            audios.extend(
                voices
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|voice| json!({"voice_id":voice})),
            );
            if !images.is_empty() {
                body["reference_images"] = json!(images);
            }
            if !audios.is_empty() {
                body["reference_audios"] = json!(audios);
            }
            if !frames.is_empty() {
                body["keyframes"] = json!(frames);
            }
            Ok(("/v1/videos/generations".into(), body))
        }
        "openai-audio" => {
            body.as_object_mut().unwrap().remove("prompt");
            body["modalities"] = json!(["text", "audio"]);
            let mut content = vec![json!({"type":"text","text":x["prompt"]})];
            for (r, url) in refs.iter().zip(urls) {
                let (_, data) = url.split_once(',').ok_or(Error("invalid_reference_type"))?;
                content.push(json!({"type":"input_audio","input_audio":{"data":data,"format":if r["mime"] == "audio/mpeg" {"mp3"} else {"wav"}}}));
            }
            body["messages"] = json!([{"role":"user","content":content}]);
            Ok(("/v1/chat/completions".into(), body))
        }
        "xai-tts" => {
            body.as_object_mut().unwrap().remove("model");
            body.as_object_mut().unwrap().remove("prompt");
            body["text"] = x["prompt"].clone();
            Ok(("/v1/tts".into(), body))
        }
        _ => Err(Error("unsupported_provider")),
    }
}
