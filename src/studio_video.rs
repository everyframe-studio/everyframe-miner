//! Opt-in Studio contracts. Old model IDs and signed bodies stay unchanged.
use crate::{
    Error, Result, n, need,
    network::{Http, Request, safe_url},
    protocol::{exact, sha},
    s,
};
use serde_json::{Value, json};
use std::sync::LazyLock;
pub static POLICIES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/studio-video.json"))
        .expect("reviewed Studio contracts")
});
pub fn check(spec: &Value) -> Result<()> {
    exact(spec, &["model", "input"])?;
    let p = &POLICIES[s(&spec["model"])?];
    need(p.is_object(), "unsupported_studio_model")?;
    let v = &spec["input"];
    exact(
        v,
        &[
            "prompt",
            "seed",
            "duration",
            "resolution",
            "aspect",
            "references",
            "promptExpansion",
        ],
    )?;
    let prompt = s(&v["prompt"])?;
    need(
        !prompt.trim().is_empty()
            && prompt.len() <= 150000
            && prompt.encode_utf16().count() <= n(&p["maxPrompt"])? as usize,
        "invalid_studio_prompt",
    )?;
    need(
        (0..2147483648).contains(&n(&v["seed"])?),
        "invalid_studio_seed",
    )?;
    need(
        (n(&p["min"])?..=n(&p["max"])?).contains(&n(&v["duration"])?),
        "invalid_studio_duration",
    )?;
    for (key, policy, code) in [
        ("resolution", "resolutions", "invalid_studio_resolution"),
        ("aspect", "aspects", "invalid_studio_aspect"),
    ] {
        need(
            p[policy].as_array().is_some_and(|a| a.contains(&v[key])),
            code,
        )?;
    }
    need(
        ["balanced", "disabled"].contains(&s(&v["promptExpansion"])?),
        "invalid_prompt_expansion",
    )?;
    let refs = v["references"]
        .as_array()
        .ok_or(Error("invalid_studio_references"))?;
    need(refs.len() <= 14, "invalid_studio_references")?;
    let mut counts = [0i64; 5];
    let mut durations = [0i64; 5];
    let mut bytes = 0i64;
    for a in refs {
        exact(
            a,
            &[
                "url",
                "sha256",
                "bytes",
                "mime",
                "role",
                "width",
                "height",
                "durationMs",
            ],
        )?;
        let url = s(&a["url"])?;
        need(
            url.len() <= 1000
                && crate::matches(
                    r"https://(api|app)\.everyframe\.studio/(api/)?v1/attachment/att_[A-Za-z0-9_-]+\?t=[A-Za-z0-9_-]{20,100}",
                    &a["url"],
                ),
            "invalid_reference_origin",
        )?;
        need(
            crate::matches("[a-f0-9]{64}", &a["sha256"]),
            "invalid_reference_hash",
        )?;
        let role = s(&a["role"])?;
        let index = ["image", "video", "audio", "first", "last"]
            .iter()
            .position(|r| *r == role)
            .ok_or(Error("invalid_reference_role"))?;
        counts[index] += 1;
        let kind = if index >= 3 { 0 } else { index };
        let mime = s(&a["mime"])?;
        let allowed: &[&str] = match kind {
            0 => &["image/jpeg", "image/png", "image/webp"],
            1 => &["video/mp4", "video/webm", "video/quicktime"],
            _ => &[
                "audio/mpeg",
                "audio/wav",
                "audio/mp4",
                "audio/ogg",
                "audio/webm",
            ],
        };
        need(allowed.contains(&mime), "invalid_reference_mime")?;
        let max = if p["provider"] == "vivix" {
            10
        } else {
            match kind {
                1 => 64,
                2 => 20,
                _ => 12,
            }
        } * 1024
            * 1024;
        let size = n(&a["bytes"])?;
        need(size > 0 && size <= max, "invalid_reference_size")?;
        bytes += size;
        for key in ["width", "height"] {
            need(
                (0..=16384).contains(&n(&a[key])?),
                "invalid_reference_metadata",
            )?;
        }
        let duration = n(&a["durationMs"])?;
        need(
            kind == 2 || n(&a["width"])? > 0 && n(&a["height"])? > 0,
            "invalid_reference_metadata",
        )?;
        need(
            if kind == 0 {
                duration == 0
            } else {
                duration > 0
            },
            "invalid_reference_metadata",
        )?;
        need(
            (0..=15000).contains(&duration),
            "invalid_reference_metadata",
        )?;
        durations[index] += duration;
    }
    need(bytes <= 128 * 1024 * 1024, "reference_set_too_large")?;
    need(
        counts[3] <= 1
            && counts[4] <= 1
            && counts[3] + counts[4] <= n(&p["frames"])?
            && (counts[4] == 0 || counts[3] == 1),
        "invalid_reference_frames",
    )?;
    for (i, kind) in ["image", "video", "audio"].iter().enumerate() {
        need(
            counts[i] <= n(&p["refs"][kind])?,
            "reference_kind_not_supported",
        )?;
    }
    need(
        p["mixed"] == true || counts[..3].iter().sum::<i64>() == 0 || counts[3] + counts[4] == 0,
        "reference_modes_conflict",
    )?;
    if p["provider"] == "fal" && counts[2] > 0 {
        need(counts[0] + counts[1] > 0, "audio_needs_visual_reference")?;
    }
    need(
        durations[1] <= 15000 && durations[2] <= 15000,
        "reference_duration_exceeded",
    )?;
    Ok(())
}

/// All source reads are credential-free, redirect-free, size-bounded, and
/// hash checked BEFORE a provider can accept or charge for a generation.
pub fn references(http: &dyn Http, spec: &Value) -> Result<Vec<(Value, Vec<u8>)>> {
    check(spec)?;
    let mut result = vec![];
    for a in spec["input"]["references"].as_array().unwrap() {
        let mut r = Request::get(
            s(&a["url"])?,
            &["api.everyframe.studio", "app.everyframe.studio"],
        );
        safe_url(&r.url, &r.hosts)?;
        r.limit = n(&a["bytes"])? as usize;
        r.timeout = 60;
        let bytes = http.bytes(r)?;
        need(
            bytes.len() == n(&a["bytes"])? as usize && sha(&bytes) == s(&a["sha256"])?,
            "reference_integrity_mismatch",
        )?;
        result.push((a.clone(), bytes));
    }
    Ok(result)
}

/// `urls` preserve original ordering. Vivix URLs are workspace uploads of the
/// verified bytes, other providers receive the immutable Everyframe sources.
pub fn body(spec: &Value, urls: &[String]) -> Result<(String, Value)> {
    check(spec)?;
    let p = &POLICIES[s(&spec["model"])?];
    let v = &spec["input"];
    let refs = v["references"].as_array().unwrap();
    need(refs.len() == urls.len(), "reference_count_mismatch")?;
    let mut first = None;
    let mut last = None;
    let mut images = vec![];
    let mut videos = vec![];
    let mut audio = vec![];
    for (a, url) in refs.iter().zip(urls) {
        match s(&a["role"])? {
            "first" => first = Some(url),
            "last" => last = Some(url),
            "image" => images.push(url),
            "video" => videos.push(url),
            "audio" => audio.push(url),
            _ => return Err(Error("invalid_reference_role")),
        }
    }
    let has_refs = !images.is_empty() || !videos.is_empty() || !audio.is_empty();
    let model = s(&p["apiModel"])?;
    let mut b = json!({"prompt":v["prompt"],"duration":v["duration"],"aspect_ratio":v["aspect"],"seed":v["seed"]});
    if p["provider"] == "vivix" {
        b.as_object_mut().unwrap().remove("seed");
        if let Some(url) = first {
            b["first_frame_image"] = json!(url)
        }
        if let Some(url) = last {
            b["last_frame_image"] = json!(url)
        }
        if !images.is_empty() {
            b["ref_images"] = json!(
                images
                    .iter()
                    .map(|url| json!({"url":url}))
                    .collect::<Vec<_>>()
            )
        }
        if !audio.is_empty() {
            b["ref_audios"] = json!(
                audio
                    .iter()
                    .map(|url| json!({"url":url}))
                    .collect::<Vec<_>>()
            )
        }
        return Ok((
            "/v1/videos/jobs".into(),
            json!({"model":model,"seed":v["seed"],"jobs":[b]}),
        ));
    }
    b["resolution"] = v["resolution"].clone();
    if p["provider"] == "openrouter" {
        b["model"] = json!(model);
        if has_refs {
            let mut values = vec![];
            for (kind, list) in [
                ("image_url", images),
                ("video_url", videos),
                ("audio_url", audio),
            ] {
                for url in list {
                    let mut item = json!({"type":kind});
                    item[kind] = json!({"url":url});
                    values.push(item);
                }
            }
            b["input_references"] = json!(values);
        } else if let Some(url) = first {
            let mut frames = vec![
                json!({"type":"image_url","image_url":{"url":url},"frame_type":"first_frame"}),
            ];
            if let Some(url) = last {
                frames.push(
                    json!({"type":"image_url","image_url":{"url":url},"frame_type":"last_frame"}),
                );
            }
            b["frame_images"] = json!(frames);
        }
        return Ok(("/api/v1/videos".into(), b));
    }
    let app = model
        .strip_suffix("/text-to-video")
        .ok_or(Error("unsupported_studio_endpoint"))?;
    let h3 = app.starts_with("minimax/h3-max");
    let wan = app == "alibaba/wan-3.0";
    let seedance = app == "bytedance/seedance-2.5";
    if h3 {
        b["resolution"] = json!(s(&v["resolution"])?.to_uppercase());
        b["enable_safety_checker"] = json!(true);
        b["prompt_expansion_mode"] = v["promptExpansion"].clone();
    }
    if wan {
        b["audio"] = json!(true);
    }
    if seedance {
        b["duration"] = json!(n(&v["duration"])?.to_string());
        b["generate_audio"] = json!(true);
        b["bitrate_mode"] = json!("standard");
    }
    if seedance || app.starts_with("xai/") {
        b.as_object_mut().unwrap().remove("seed");
    }
    if let Some(url) = first {
        b[if wan { "start_image_url" } else { "image_url" }] = json!(url);
        if h3 {
            b.as_object_mut().unwrap().remove("aspect_ratio");
        } else {
            b["aspect_ratio"] = json!(if wan { "adaptive" } else { "auto" });
        }
    }
    if let Some(url) = last {
        b["end_image_url"] = json!(url);
    }
    for (key, list) in [
        (
            if seedance {
                "image_urls"
            } else {
                "reference_image_urls"
            },
            images,
        ),
        (
            if seedance {
                "video_urls"
            } else {
                "reference_video_urls"
            },
            videos,
        ),
        (
            if seedance {
                "audio_urls"
            } else {
                "reference_audio_urls"
            },
            audio,
        ),
    ] {
        if !list.is_empty() {
            b[key] = json!(list);
        }
    }
    Ok((
        format!(
            "/{app}/{}",
            if has_refs {
                "reference-to-video"
            } else if first.is_some() {
                "image-to-video"
            } else {
                "text-to-video"
            }
        ),
        b,
    ))
}

pub fn model_registry() -> Value {
    let mut result = serde_json::Map::new();
    for (id, p) in POLICIES.as_object().unwrap() {
        let model = p["apiModel"].as_str().unwrap();
        let app = model.split('/').take(2).collect::<Vec<_>>().join("/");
        result.insert(id.clone(),json!({"provider":p["provider"],"apiModel":model,"app":app,"name":p["name"],"family":"Studio","kind":"video","mime":"video/mp4","params":{},"seed":true,"duration":5,"resolution":p["resolutions"][0],"aspect":"16:9","dimensions":[],"codecs":["h264","hevc"],"deadlineMs":480000,"maxPrompt":p["maxPrompt"]}));
    }
    Value::Object(result)
}
