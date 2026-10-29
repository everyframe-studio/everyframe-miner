//! Versioned Director contracts with exact parameters and immutable owned inputs.
use crate::{
    Error, Result, n, need,
    network::{Http, Request},
    protocol::{exact, sha},
    s,
};
use serde_json::{Value, json};
use std::sync::LazyLock;
pub static POLICIES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/studio-tools.json"))
        .expect("reviewed Director contracts")
});
fn int(v: &Value, min: i64, max: i64) -> bool {
    v.as_i64().is_some_and(|n| n >= min && n <= max)
}
pub fn check(spec: &Value) -> Result<()> {
    exact(spec, &["model", "input"])?;
    let p = &POLICIES[s(&spec["model"])?];
    need(p.is_object(), "unsupported_tool_model")?;
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
        prompt.len() <= 150000 && prompt.encode_utf16().count() <= n(&p["maxPrompt"])? as usize,
        "invalid_tool_prompt",
    )?;
    need(int(&v["seed"], 0, 2147483647), "invalid_tool_seed")?;
    need(
        int(&v["duration"], n(&p["min"])?, n(&p["max"])?),
        "invalid_tool_duration",
    )?;
    need(
        p["resolutions"]
            .as_array()
            .unwrap()
            .contains(&v["resolution"])
            && p["aspects"].as_array().unwrap().contains(&v["aspect"]),
        "invalid_tool_format",
    )?;
    need(
        ["balanced", "disabled"].contains(&s(&v["promptExpansion"])?),
        "invalid_prompt_expansion",
    )?;
    let fields = p["parameters"].as_object().unwrap();
    let keys = fields.keys().map(String::as_str).collect::<Vec<_>>();
    exact(&v["parameters"], &keys)?;
    for (k, f) in fields {
        let a = &v["parameters"][k];
        let ok = if let Some(e) = f["enum"].as_array() {
            e.contains(a)
        } else {
            match f["type"].as_str() {
                Some("boolean") => a.is_boolean(),
                Some("integer") => int(a, n(&f["min"])?, n(&f["max"])?),
                Some("strings") => a.as_array().is_some_and(|a| {
                    a.len() <= f["maxItems"].as_u64().unwrap() as usize
                        && a.iter().all(|s| {
                            s.as_str().is_some_and(|s| {
                                !s.is_empty()
                                    && s.encode_utf16().count()
                                        <= f["maxLength"].as_u64().unwrap() as usize
                                    && !s.chars().any(|c| c < ' ')
                            })
                        })
                }),
                Some("integers") => a.as_array().is_some_and(|a| {
                    a.len() <= f["maxItems"].as_u64().unwrap() as usize
                        && a.iter()
                            .all(|n| int(n, f["min"].as_i64().unwrap(), f["max"].as_i64().unwrap()))
                }),
                _ => false,
            }
        };
        need(ok, "invalid_tool_parameter")?;
    }
    let refs = v["references"]
        .as_array()
        .ok_or(Error("invalid_tool_references"))?;
    need(refs.len() <= 14, "invalid_tool_references")?;
    let mut counts = std::collections::BTreeMap::new();
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
                "fpsMilli",
                "hasAudio",
            ],
        )?;
        let role = s(&a["role"])?;
        need(
            p["references"].get(role).is_some(),
            "invalid_reference_role",
        )?;
        *counts.entry(role).or_insert(0) += 1;
        let lora = role == "lora";
        let pattern = if lora {
            r"https://(api|app)\.everyframe\.studio/(api/)?v1/tool-asset/ast_[A-Za-z0-9_-]+\?t=[A-Za-z0-9_-]{20,100}"
        } else {
            r"https://(api|app)\.everyframe\.studio/(api/)?v1/attachment/att_[A-Za-z0-9_-]+\?t=[A-Za-z0-9_-]{20,100}"
        };
        need(
            s(&a["url"])?.len() <= 1000 && crate::matches(pattern, &a["url"]),
            "invalid_reference_origin",
        )?;
        need(
            crate::matches("[a-f0-9]{64}", &a["sha256"]) && a["sha256"] != "0".repeat(64),
            "reference_verification_required",
        )?;
        let max = match role {
            "lora" => 128,
            "video" => 50,
            "audio" => 15,
            _ => 12,
        } * 1024
            * 1024;
        need(int(&a["bytes"], 1, max), "invalid_reference_size")?;
        total += n(&a["bytes"])?;
        need(
            int(&a["width"], 0, 4096)
                && int(&a["height"], 0, 4096)
                && int(&a["durationMs"], 0, 600000)
                && int(&a["frameCount"], 0, 36030)
                && int(&a["fpsMilli"], 0, 60050)
                && a["hasAudio"].is_boolean(),
            "invalid_reference_metadata",
        )?;
        let (w, h, d, f, fps) = (
            n(&a["width"])?,
            n(&a["height"])?,
            n(&a["durationMs"])?,
            n(&a["frameCount"])?,
            n(&a["fpsMilli"])?,
        );
        let mime = s(&a["mime"])?;
        match role {
            "image" => need(
                ["image/png", "image/jpeg", "image/webp"].contains(&mime)
                    && w >= 256
                    && h >= 256
                    && d == 0
                    && f == 0
                    && fps == 0
                    && a["hasAudio"] == false,
                "invalid_image_reference",
            )?,
            "audio" => need(
                [
                    "audio/mpeg",
                    "audio/wav",
                    "audio/mp4",
                    "audio/ogg",
                    "audio/webm",
                ]
                .contains(&mime)
                    && w == 0
                    && h == 0
                    && (1000..=30000).contains(&d)
                    && f == 0
                    && fps == 0
                    && a["hasAudio"] == true,
                "invalid_audio_reference",
            )?,
            "video" => need(
                ["video/mp4", "video/webm", "video/quicktime"].contains(&mime)
                    && w >= 256
                    && h >= 256
                    && d >= 1000
                    && f > 0
                    && fps > 0,
                "invalid_video_reference",
            )?,
            "lora" => need(
                mime == "application/x-safetensors"
                    && w == 0
                    && h == 0
                    && d == 0
                    && f == 0
                    && fps == 0
                    && a["hasAudio"] == false,
                "invalid_lora_reference",
            )?,
            _ => return Err(Error("invalid_reference_role")),
        }
    }
    need(total <= 448 * 1024 * 1024, "reference_set_too_large")?;
    for (role, range) in p["references"].as_object().unwrap() {
        let count = *counts.get(role.as_str()).unwrap_or(&0);
        need(
            count >= n(&range[0])? && count <= n(&range[1])?,
            "reference_required_or_limit",
        )?;
    }
    let mode = s(&p["mode"])?;
    let source = refs.iter().find(|a| a["role"] == "video");
    if let Some(a) = source {
        let d = n(&a["durationMs"])?;
        let w = n(&a["width"])?;
        let h = n(&a["height"])?;
        need(
            (d + 999) / 1000 == n(&v["duration"])? && d <= n(&p["max"])? * 1000,
            "source_duration_mismatch",
        )?;
        if mode == "edit" {
            need(
                d >= 3000 && w >= 700 && h >= 700 && int(&a["fpsMilli"], 23900, 60050),
                "invalid_kling_source",
            )?;
        }
        if mode == "recast" {
            need(d >= 5000, "invalid_recast_source")?;
        }
        if mode == "relight" {
            need(d >= 2334, "invalid_relight_source")?;
        }
        if mode.starts_with("dub") {
            need(a["hasAudio"] == true, "speech_audio_required")?;
        } else {
            need(
                w * 5 >= h * 2 && w * 2 <= h * 5,
                "reference_aspect_not_supported",
            )?;
        }
    }
    if mode == "edit" {
        need(
            refs.iter().filter(|a| a["role"] == "image").all(|a| {
                let w = a["width"].as_i64().unwrap();
                let h = a["height"].as_i64().unwrap();
                w >= 300 && h >= 300 && w * 5 >= h * 2 && w * 2 <= h * 5
            }),
            "invalid_kling_image",
        )?;
    }
    if mode == "voice" || mode == "edit" || mode.starts_with("lora") {
        need(!prompt.trim().is_empty(), "tool_prompt_required")?;
    }
    if mode.starts_with("lora") {
        need(
            v["parameters"]["loraScales"].as_array().unwrap().len()
                == *counts.get("lora").unwrap_or(&0) as usize,
            "lora_scale_count_mismatch",
        )?;
    }
    if ["gsap", "glsl"].contains(&mode) {
        let video = *counts.get("video").unwrap_or(&0);
        let image = *counts.get("image").unwrap_or(&0);
        need(video == 0 || image == 0, "reference_modes_conflict")?;
        need(
            !prompt.trim().is_empty() || video > 0 || image > 0,
            "tool_prompt_or_reference_required",
        )?;
        need(
            refs.iter()
                .filter(|a| a["role"] == "audio")
                .all(|a| a["durationMs"].as_i64().unwrap() <= 15000),
            "reference_duration_exceeded",
        )?;
    }
    Ok(())
}
pub fn verify_safetensors(bytes: &[u8]) -> Result<()> {
    need(bytes.len() >= 10, "invalid_lora_file")?;
    let header = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    need(
        (2..=1048576).contains(&header) && header as usize + 8 < bytes.len(),
        "invalid_lora_file",
    )?;
    let value: Value = serde_json::from_slice(&bytes[8..8 + header as usize])
        .map_err(|_| Error("invalid_lora_file"))?;
    let object = value.as_object().ok_or(Error("invalid_lora_file"))?;
    let size = bytes.len() - 8 - header as usize;
    let mut ranges = vec![];
    for (k, v) in object {
        if k == "__metadata__" {
            need(
                v.as_object()
                    .is_some_and(|m| m.values().all(Value::is_string)),
                "invalid_lora_file",
            )?;
            continue;
        }
        need(k.len() <= 1024, "invalid_lora_file")?;
        exact(v, &["dtype", "shape", "data_offsets"])?;
        let unit = match s(&v["dtype"])? {
            "F64" | "I64" | "U64" => 8,
            "F32" | "I32" | "U32" => 4,
            "F16" | "BF16" | "I16" | "U16" => 2,
            "I8" | "U8" | "BOOL" | "F8_E4M3" | "F8_E5M2" => 1,
            _ => return Err(Error("invalid_lora_file")),
        };
        let shape = v["shape"].as_array().ok_or(Error("invalid_lora_file"))?;
        need(shape.len() <= 16, "invalid_lora_file")?;
        let mut count = 1u64;
        for d in shape {
            count = count
                .checked_mul(d.as_u64().ok_or(Error("invalid_lora_file"))?)
                .ok_or(Error("invalid_lora_file"))?;
        }
        let offsets = v["data_offsets"]
            .as_array()
            .ok_or(Error("invalid_lora_file"))?;
        need(offsets.len() == 2, "invalid_lora_file")?;
        let start = offsets[0].as_u64().ok_or(Error("invalid_lora_file"))?;
        let end = offsets[1].as_u64().ok_or(Error("invalid_lora_file"))?;
        need(
            end >= start && end <= size as u64 && count.checked_mul(unit) == Some(end - start),
            "invalid_lora_file",
        )?;
        ranges.push((start, end));
    }
    need(!ranges.is_empty(), "invalid_lora_file")?;
    ranges.sort_unstable();
    let mut cursor = 0;
    for (start, end) in ranges {
        need(start == cursor, "invalid_lora_file")?;
        cursor = end;
    }
    need(cursor == size as u64, "invalid_lora_file")
}
pub fn references(http: &dyn Http, spec: &Value) -> Result<Vec<String>> {
    check(spec)?;
    let mut urls = vec![];
    for a in spec["input"]["references"].as_array().unwrap() {
        let mut r = Request::get(
            s(&a["url"])?,
            &["api.everyframe.studio", "app.everyframe.studio"],
        );
        r.limit = n(&a["bytes"])? as usize;
        r.timeout = 120;
        let bytes = http.bytes(r)?;
        need(
            bytes.len() == n(&a["bytes"])? as usize && sha(&bytes) == s(&a["sha256"])?,
            "reference_integrity_mismatch",
        )?;
        if a["role"] == "lora" {
            verify_safetensors(&bytes)?;
        }
        urls.push(s(&a["url"])?.to_string());
    }
    Ok(urls)
}
pub fn body(spec: &Value, urls: &[String]) -> Result<(String, Value)> {
    check(spec)?;
    let v = &spec["input"];
    let p = &POLICIES[s(&spec["model"])?];
    let refs = v["references"].as_array().unwrap();
    need(refs.len() == urls.len(), "reference_count_mismatch")?;
    let group = |role: &str| {
        refs.iter()
            .zip(urls)
            .filter(|(a, _)| a["role"] == role)
            .map(|(_, url)| url.clone())
            .collect::<Vec<_>>()
    };
    let videos = group("video");
    let images = group("image");
    let audio = group("audio");
    let par = &v["parameters"];
    let mode = s(&p["mode"])?;
    let mut b = match mode {
        "edit" => {
            json!({"prompt":v["prompt"],"video_url":videos[0],"resolution":v["resolution"],"duration":v["duration"]})
        }
        "recast" => {
            json!({"video_url":videos[0],"reference_image_urls":images,"resolution":s(&v["resolution"])?.to_uppercase(),"seed":v["seed"],"enhance_realism":par["enhanceRealism"],"enable_safety_checker":true})
        }
        "relight" => {
            json!({"video_url":videos[0],"reference_image_url":images[0],"resolution":s(&v["resolution"])?.to_uppercase(),"aspect_ratio":"adaptive","seed":v["seed"],"enable_safety_checker":true})
        }
        "dub" | "dub-pro" => {
            json!({"video_url":videos[0],"target_language":par["targetLanguage"],"source_language":par["sourceLanguage"],"keyterms":par["keyterms"]})
        }
        "voice" => {
            json!({"text":v["prompt"],"voice":par["voice"],"expressiveness_boost":n(&par["expressiveness"])? as f64/100.0,"enable_safety_checker":true})
        }
        "lora" | "lora-edit" => {
            let (w, h) = match s(&v["aspect"])? {
                "16:9" => (1376, 768),
                "9:16" => (768, 1376),
                "4:3" => (1152, 864),
                "3:4" => (864, 1152),
                _ => (1024, 1024),
            };
            json!({"prompt":v["prompt"],"seed":v["seed"],"image_size":{"width":w,"height":h},"num_images":1,"output_format":"png","enable_safety_checker":true,"enable_prompt_expansion":v["promptExpansion"]=="balanced","sync_mode":false,"loras":group("lora").iter().enumerate().map(|(i,url)|json!({"path":url,"scale":par["loraScales"][i].as_i64().unwrap() as f64/100.0})).collect::<Vec<_>>()})
        }
        "gsap" | "glsl" => {
            json!({"prompt":v["prompt"],"duration":v["duration"],"aspect_ratio":v["aspect"],"refine_rounds":par["refineRounds"],"enable_safety_checker":true})
        }
        _ => return Err(Error("unsupported_tool_model")),
    };
    if mode == "edit" && !images.is_empty() {
        b["image_urls"] = json!(images);
    }
    if mode == "recast" && !s(&v["prompt"])?.trim().is_empty() {
        b["prompt"] = v["prompt"].clone();
    }
    if mode == "dub-pro" {
        b["voice_cloning_strength"] = par["voiceStrength"].clone();
    }
    if mode == "voice" {
        if par["language"] != "auto" {
            b["language"] = par["language"].clone();
        }
        if let Some(url) = audio.first() {
            b["reference_audio_url"] = json!(url);
        }
    }
    if mode == "lora-edit" {
        b["image_urls"] = json!(images);
    }
    if ["gsap", "glsl"].contains(&mode) {
        for (key, urls) in [
            ("video_url", &videos),
            ("image_url", &images),
            ("audio_url", &audio),
        ] {
            if let Some(url) = urls.first() {
                b[key] = json!(url);
            }
        }
    }
    Ok((format!("/{}", s(&p["apiModel"])?), b))
}
pub fn model_registry() -> Value {
    let mut models = serde_json::Map::new();
    for (id, p) in POLICIES.as_object().unwrap() {
        let kind = p["kind"].as_str().unwrap();
        let api = p["apiModel"].as_str().unwrap();
        let mime = p["outputMime"].as_str().unwrap_or(match kind {
            "image" => "image/png",
            "audio" => "audio/wav",
            _ => "video/mp4",
        });
        models.insert(id.clone(),json!({"provider":"fal","apiModel":api,"app":api.split('/').take(2).collect::<Vec<_>>().join("/"),"name":p["name"],"vendor":"fal","family":"Director tools","kind":kind,"mime":mime,"params":{},"seed":true,"duration":p["min"],"resolution":p["resolutions"][0],"aspect":p["aspects"][0],"dimensions":[],"codecs":["h264","hevc"],"deadlineMs":p["deadlineMs"],"feeMultiplier":1,"audio":kind!="image","maxPrompt":p["maxPrompt"],"mediaPolicy":2,"toolPolicy":1,"maxDurationMs":if kind=="audio"{600000}else{p["max"].as_i64().unwrap()*1000},"maxOutputBytes":p["maxOutputBytes"],"schemaUrl":format!("https://fal.ai/models/{api}/api"),"schemaCheckedAt":"2026-10-29"}));
    }
    Value::Object(models)
}

/// Preserve executable source as inert data, bound to the same receipt as the preview.
pub fn motion_bundle(model: &str, data: &Value, video: &[u8]) -> Result<Vec<u8>> {
    use base64::Engine;
    let mode = s(&POLICIES[model]["mode"])?;
    need(["gsap", "glsl"].contains(&mode), "invalid_motion_format")?;
    need(
        video.len() >= 24 && video.len() <= 40 * 1024 * 1024 && &video[4..8] == b"ftyp",
        "invalid_motion_preview",
    )?;
    for k in ["code", "html"] {
        let text = s(&data[k])?;
        need(
            !text.is_empty() && text.len() <= 1024 * 1024 && !text.contains('\0'),
            "invalid_motion_source",
        )?;
    }
    serde_json::to_vec(&json!({"version":1,"format":mode,"code":data["code"],"html":data["html"],"video_base64":base64::engine::general_purpose::STANDARD.encode(video)})).map_err(|_|Error("invalid_motion_bundle"))
}
