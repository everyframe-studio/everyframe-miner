//! v0.4.0 media contracts. No arbitrary endpoint, URL, or provider options.
use crate::{
    Error, Result, n, need,
    network::{Http, Request},
    protocol::{exact, sha},
    s,
};
use serde_json::{Value, json};
use std::sync::LazyLock;
pub static POLICIES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/studio-media.json"))
        .expect("reviewed media policies")
});
pub fn check(spec: &Value) -> Result<()> {
    exact(spec, &["model", "input"])?;
    let p = &POLICIES[s(&spec["model"])?];
    need(p.is_object(), "unsupported_media_model")?;
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
            "parameters",
        ],
    )?;
    let prompt = s(&v["prompt"])?;
    need(
        !prompt.trim().is_empty()
            && prompt.len() <= 150000
            && prompt.encode_utf16().count() <= 50000,
        "invalid_media_prompt",
    )?;
    need(
        (0..2147483648).contains(&n(&v["seed"])?),
        "invalid_media_seed",
    )?;
    let kind = s(&p["kind"])?;
    let max = match kind {
        "video" => 15,
        "image" => 1,
        _ => n(&p["maxDurationMs"])? / 1000,
    };
    need(
        (1..=max).contains(&n(&v["duration"])?),
        "invalid_media_duration",
    )?;
    need(
        if kind == "video" {
            ["480p", "768p", "1080p"].contains(&s(&v["resolution"])?)
        } else {
            v["resolution"] == if kind == "image" { "1k" } else { "audio" }
        },
        "invalid_media_resolution",
    )?;
    need(
        if kind == "audio" {
            v["aspect"] == "none"
        } else {
            ["21:9", "16:9", "4:3", "1:1", "3:4", "9:16"].contains(&s(&v["aspect"])?)
                || (kind == "image" && v["aspect"] == "4:5")
        },
        "invalid_media_aspect",
    )?;
    need(
        ["balanced", "disabled"].contains(&s(&v["promptExpansion"])?),
        "invalid_prompt_expansion",
    )?;
    let fields = p["parameters"]
        .as_object()
        .ok_or(Error("invalid_media_policy"))?;
    let params = v["parameters"]
        .as_object()
        .ok_or(Error("invalid_media_fields"))?;
    need(
        params.keys().all(|k| fields.contains_key(k)),
        "invalid_media_fields",
    )?;
    for (k, f) in fields {
        if f["optional"] == true && !params.contains_key(k) {
            continue;
        }
        let a = params.get(k).ok_or(Error("invalid_media_fields"))?;
        let ok = if let Some(e) = f["enum"].as_array() {
            e.contains(a)
        } else {
            match f["type"].as_str() {
                Some("boolean") => a.is_boolean(),
                Some("integer") => a.as_i64().is_some_and(|x| {
                    x >= f["min"].as_i64().unwrap_or(0) && x <= f["max"].as_i64().unwrap_or(0)
                }),
                Some("string") => a.as_str().is_some_and(|x| {
                    x.encode_utf16().count() >= f["min"].as_u64().unwrap_or(0) as usize
                        && x.encode_utf16().count() <= f["max"].as_u64().unwrap_or(0) as usize
                }),
                _ => false,
            }
        };
        need(ok, "invalid_media_parameter")?;
    }
    let refs = v["references"]
        .as_array()
        .ok_or(Error("invalid_media_references"))?;
    need(
        refs.len() <= n(&p["maxReferences"])? as usize,
        "invalid_media_references",
    )?;
    let mut counts = [0i64; 5];
    let mut durations = [0i64; 5];
    let mut total = 0;
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
                "frameCount",
            ],
        )?;
        need(
            s(&a["url"])?.len() <= 1000
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
        let i = ["image", "video", "audio", "first", "last"]
            .iter()
            .position(|r| Some(*r) == a["role"].as_str())
            .ok_or(Error("invalid_reference_role"))?;
        counts[i] += 1;
        let k = if i >= 3 { 0 } else { i };
        let allowed: &[&str] = match k {
            0 => &["image/png", "image/jpeg", "image/webp"],
            1 => &["video/mp4", "video/webm", "video/quicktime"],
            _ => &[
                "audio/mpeg",
                "audio/wav",
                "audio/mp4",
                "audio/ogg",
                "audio/webm",
            ],
        };
        need(
            allowed.contains(&s(&a["mime"])?) && (kind == "video" || k == 0),
            "invalid_reference_mime",
        )?;
        let bytes = n(&a["bytes"])?;
        need(
            bytes > 0
                && bytes
                    <= match k {
                        1 => 64,
                        2 => 20,
                        _ => 12,
                    } * 1024
                        * 1024,
            "invalid_reference_size",
        )?;
        total += bytes;
        let w = n(&a["width"])?;
        let h = n(&a["height"])?;
        let d = n(&a["durationMs"])?;
        let frames = n(&a["frameCount"])?;
        need(
            (0..=16384).contains(&w)
                && (0..=16384).contains(&h)
                && (0..=15000).contains(&d)
                && (0..=3600).contains(&frames),
            "invalid_reference_metadata",
        )?;
        need(k == 2 || w > 0 && h > 0, "invalid_reference_metadata")?;
        need(
            match k {
                0 => d == 0 && frames == 0,
                2 => d >= 2000 && frames == 0,
                _ => d >= 2000,
            },
            "invalid_reference_metadata",
        )?;
        if kind == "video" && k != 2 {
            need(
                w * 5 >= h * 2 && w * 2 <= h * 5,
                "reference_aspect_not_supported",
            )?;
        }
        durations[i] += d;
    }
    need(total <= 128 * 1024 * 1024, "reference_set_too_large")?;
    if kind == "video" {
        need(
            counts[3] <= 1
                && counts[4] <= 1
                && (counts[4] == 0 || counts[3] == 1)
                && counts[0] <= 9
                && counts[1] <= 3
                && counts[2] <= 3,
            "invalid_reference_frames",
        )?;
        need(
            (1..=12).contains(&(counts[0] + counts[1] + counts[2])),
            "reference_required",
        )?;
        need(
            durations[1] <= 15000 && durations[2] <= 15000,
            "reference_duration_exceeded",
        )?;
    } else if kind == "image" {
        need(
            refs.iter().all(|a| a["role"] == "image"),
            "invalid_reference_role",
        )?;
        let mask = n(&v["parameters"]["maskIndex"])?;
        let precise = v["parameters"]["editPrecision"] == "high";
        need(
            mask == -1 || mask > 0 && (mask as usize) < refs.len() && precise,
            "invalid_image_mask",
        )?;
        need(
            !precise || (p["apiModel"] == "fal-ai/ideogram/v4.5" && !refs.is_empty()),
            "invalid_image_precision",
        )?;
        need(
            p["apiModel"] == "fal-ai/ideogram/v4.5" || v["parameters"]["quality"] == "medium",
            "invalid_image_quality",
        )?;
    } else {
        for key in ["duration", "seconds_total", "duration_seconds"] {
            if let Some(d) = params.get(key) {
                need(d == &v["duration"], "audio_duration_mismatch")?;
            }
        }
        need(
            params.get("text").or_else(|| params.get("prompt")) == Some(&v["prompt"]),
            "audio_prompt_mismatch",
        )?;
    }
    Ok(())
}
/// Never submit paid work before all reference bytes match the signed manifest.
pub fn references(http: &dyn Http, spec: &Value) -> Result<Vec<(Value, Vec<u8>)>> {
    check(spec)?;
    let mut out = vec![];
    for a in spec["input"]["references"].as_array().unwrap() {
        need(
            a["sha256"] != "0".repeat(64) && (a["role"] != "video" || n(&a["frameCount"])? > 0),
            "reference_verification_required",
        )?;
        let mut r = Request::get(
            s(&a["url"])?,
            &["api.everyframe.studio", "app.everyframe.studio"],
        );
        r.limit = n(&a["bytes"])? as usize;
        r.timeout = 60;
        let bytes = http.bytes(r)?;
        need(
            bytes.len() == n(&a["bytes"])? as usize && sha(&bytes) == s(&a["sha256"])?,
            "reference_integrity_mismatch",
        )?;
        out.push((a.clone(), bytes));
    }
    Ok(out)
}
pub fn body(spec: &Value, urls: &[String]) -> Result<(String, Value)> {
    check(spec)?;
    let p = &POLICIES[s(&spec["model"])?];
    let v = &spec["input"];
    let par = &v["parameters"];
    need(
        urls.len() == v["references"].as_array().unwrap().len(),
        "reference_count_mismatch",
    )?;
    if p["kind"] == "audio" {
        return Ok((format!("/{}", s(&p["path"])?), par.clone()));
    }
    if p["kind"] == "video" {
        let mut b = json!({"prompt":v["prompt"],"seed":v["seed"],"duration":v["duration"],"resolution":s(&v["resolution"])?.to_uppercase(),"aspect_ratio":v["aspect"],"enable_safety_checker":true,"prompt_expansion_mode":v["promptExpansion"]});
        for (a, url) in v["references"].as_array().unwrap().iter().zip(urls) {
            let key = match s(&a["role"])? {
                "first" => "image_url",
                "last" => "end_image_url",
                "image" => "reference_image_urls",
                "video" => "reference_video_urls",
                _ => "reference_audio_urls",
            };
            if ["image_url", "end_image_url"].contains(&key) {
                b[key] = json!(url);
                if key == "image_url" {
                    b["aspect_ratio"] = json!("adaptive");
                }
            } else {
                if b.get(key).is_none() {
                    b[key] = json!([]);
                }
                b[key].as_array_mut().unwrap().push(json!(url));
            }
        }
        return Ok(("/minimax/h3-max/reference-to-video".into(), b));
    }
    let aspect = s(&v["aspect"])?;
    let model = s(&p["apiModel"])?;
    let mask = n(&par["maskIndex"])?;
    let images: Vec<_> = urls
        .iter()
        .enumerate()
        .filter(|(i, _)| *i as i64 != mask)
        .map(|(_, u)| u)
        .collect();
    let negative = s(&par["negative"])?;
    if p["provider"] == "openrouter" {
        let hint = match aspect {
            "16:9" => "Widescreen 16:9 landscape orientation.",
            "9:16" => {
                "Vertical 9:16 portrait orientation, tall aspect ratio, framed for a phone screen."
            }
            "1:1" => "Square 1:1 aspect ratio.",
            "4:5" => "Portrait 4:5 aspect ratio.",
            "21:9" => "Ultra-wide 21:9 landscape orientation.",
            _ => "Widescreen 16:9 landscape orientation.",
        };
        let mut parts = vec![];
        if !images.is_empty() {
            parts.push("Use the attached reference images in their numbered order:".to_string());
        }
        parts.push(s(&v["prompt"])?.to_string());
        parts.push(hint.into());
        if !negative.is_empty() {
            parts.push(format!("Avoid: {negative}."));
        }
        let prompt = parts.join(" ");
        if model == "openai/gpt-image-2.5-sunburst" {
            let mut b = json!({"model":model,"prompt":prompt,"n":1,"aspect_ratio":if aspect=="4:5"{"3:4"}else{aspect}});
            if !images.is_empty() {
                b["input_references"] = json!(
                    images
                        .iter()
                        .map(|u| json!({"type":"image_url","image_url":{"url":u}}))
                        .collect::<Vec<_>>()
                );
            }
            return Ok(("/api/v1/images".into(), b));
        }
        let content = if images.is_empty() {
            json!(prompt)
        } else {
            let mut a = images
                .iter()
                .map(|u| json!({"type":"image_url","image_url":{"url":u}}))
                .collect::<Vec<_>>();
            a.push(json!({"type":"text","text":prompt}));
            json!(a)
        };
        return Ok((
            "/api/v1/chat/completions".into(),
            json!({"model":model,"modalities":["image","text"],"messages":[{"role":"user","content":content}]}),
        ));
    }
    let mut prompt = s(&v["prompt"])?.trim().to_string();
    if !negative.is_empty() {
        prompt.push_str(&format!("\n\nAvoid: {}.", negative.trim()));
    }
    let mut b = json!({"prompt":prompt});
    let path = s(&p["path"])?;
    let edit = !images.is_empty();
    if model == "fal-ai/ideogram/v4.5" {
        b["quality"] = par["quality"].clone();
        b["num_images"] = json!(1);
        b["seed"] = v["seed"].clone();
        if edit {
            b["image_url"] = json!(images[0]);
            if images.len() > 1 {
                b["reference_image_urls"] = json!(images[1..]);
            }
            b["image_size"] = json!(if par["editPrecision"] == "high" {
                "auto"
            } else {
                match aspect {
                    "16:9" | "21:9" => "landscape_16_9",
                    "9:16" => "portrait_16_9",
                    "4:5" => "portrait_4_3",
                    _ => "square_hd",
                }
            });
            if par["editPrecision"] == "high" {
                b["edit_precision"] = json!("high");
            }
            if mask >= 0 {
                b["mask_url"] = json!(urls[mask as usize]);
            }
        } else {
            b["image_size"] = match aspect {
                "16:9" => json!("landscape_16_9"),
                "9:16" => json!("portrait_16_9"),
                "4:5" => json!({"width":896,"height":1120}),
                "21:9" => json!({"width":3072,"height":1280}),
                _ => json!("square_hd"),
            };
            b["enable_prompt_expansion"] = json!(true);
        }
    } else {
        b["output_format"] = json!("jpeg");
        b["resolution"] = json!(if model == "google/nano-banana-2.1" {
            "1K"
        } else {
            "1k"
        });
        if model != "blackforestlabs/flux-3" {
            b["num_images"] = json!(1);
        }
        if model == "google/nano-banana-2.1" {
            b["seed"] = v["seed"].clone();
        }
        b["aspect_ratio"] = json!(if model == "xai/grok-imagine-image" {
            match aspect {
                "4:5" => "3:4",
                "21:9" => "20:9",
                _ => aspect,
            }
        } else {
            aspect
        });
        if edit {
            b["image_urls"] = json!(images);
        }
    }
    let suffix = if model == "blackforestlabs/flux-3" {
        if edit {
            "/edit-image"
        } else {
            "/text-to-image"
        }
    } else if edit {
        "/edit"
    } else {
        ""
    };
    Ok((format!("/{path}{suffix}"), b))
}
pub fn model_registry() -> Value {
    let mut result = serde_json::Map::new();
    for (id, p) in POLICIES.as_object().unwrap() {
        let kind = p["kind"].as_str().unwrap();
        let app = p["path"]
            .as_str()
            .unwrap()
            .split('/')
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
        result.insert(id.clone(),json!({"provider":p["provider"],"apiModel":p["apiModel"],"app":app,"name":p["name"],"family":if kind=="video"{"H3"}else{"Studio Media"},"kind":kind,
   "mime":match kind{"video"=>"video/mp4","image"=>"image/png",_=>"audio/mpeg"},"params":{},"seed":true,"duration":if kind=="video"{5}else{1},"resolution":if kind=="video"{"480p"}else if kind=="image"{"1k"}else{"audio"},"aspect":if kind=="audio"{"none"}else{"16:9"},"dimensions":[],"codecs":["h264","hevc"],"deadlineMs":p["deadlineMs"],"maxPrompt":50000}));
    }
    Value::Object(result)
}
