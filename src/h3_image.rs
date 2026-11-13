//! MiniMax H3 Image Standard/Highspeed. Exact settings, no seed or mask fiction.
use crate::{Error, Result, need, s};
use serde_json::{Value, json};

/// For the H3 Images API, 422 means every image failed content safety.
/// Keep this distinct from request validation (400) and uncertain 5xx errors.
pub fn http_error(status: u16) -> Error {
    if status == 422 {
        Error("provider_content_rejected")
    } else {
        crate::diagnostics::http_error(status)
    }
}

pub fn dimensions(p: &Value) -> Result<Option<(u64, u64)>> {
    need(
        p.get("size").is_none_or(|v| v.is_string()),
        "invalid_native_image_size",
    )?;
    let size = p["size"].as_str().unwrap_or("auto");
    if size == "auto" {
        return Ok(None);
    }
    need(
        crate::matches("[1-9][0-9]*x[1-9][0-9]*", &p["size"]),
        "invalid_native_image_size",
    )?;
    let (w, h) = size
        .split_once('x')
        .ok_or(Error("invalid_native_image_size"))?;
    let w = w
        .parse::<u64>()
        .map_err(|_| Error("invalid_native_image_size"))?;
    let h = h
        .parse::<u64>()
        .map_err(|_| Error("invalid_native_image_size"))?;
    need(
        w <= 20000000
            && h <= 20000000
            && (655360..=20000000).contains(&(w * h))
            && w.max(h) <= 3 * w.min(h),
        "invalid_native_image_size",
    )?;
    Ok(Some((w, h)))
}
pub fn check(p: &Value, m: &Value, refs: &[Value]) -> Result<()> {
    need(
        p.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                [
                    "size",
                    "area",
                    "aspect_ratio",
                    "n",
                    "output_format",
                    "disable_prompt_enhanced",
                    "output_compression",
                ]
                .contains(&k.as_str())
            })
        }),
        "invalid_native_media_fields",
    )?;
    need(
        p["n"].as_u64().is_some_and(|n| (1..=10).contains(&n)),
        "invalid_native_image_count",
    )?;
    need(
        ["jpeg", "png", "webp"].contains(&s(&p["output_format"])?)
            && p["disable_prompt_enhanced"].is_boolean(),
        "invalid_native_image_format",
    )?;
    if dimensions(p)?.is_some() {
        need(
            p.get("area").is_none() && p.get("aspect_ratio").is_none(),
            "invalid_native_image_size",
        )?;
    } else {
        need(
            p.get("area").is_none() || ["1K", "2K", "4K"].contains(&s(&p["area"])?),
            "invalid_native_image_size",
        )?;
        need(
            p.get("aspect_ratio").is_none_or(|v| v.is_string()),
            "invalid_native_image_size",
        )?;
        let ratio = p["aspect_ratio"].as_str().unwrap_or("auto");
        if ratio != "auto" {
            need(
                crate::matches("[0-9]+:[0-9]+", &json!(ratio)),
                "invalid_native_image_size",
            )?;
            let values = ratio
                .split(':')
                .map(str::parse::<u64>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| Error("invalid_native_image_size"))?;
            need(
                values.len() == 2
                    && values.iter().all(|n| (1..=300).contains(n))
                    && values[0].max(values[1]) <= 3 * values[0].min(values[1]),
                "invalid_native_image_size",
            )?;
        }
        need(
            p["disable_prompt_enhanced"] != true || ratio != "auto",
            "invalid_native_image_size",
        )?;
        need(m["provider"] != "fal", "unsupported_provider_image_size")?;
    }
    if let Some(v) = p.get("output_compression") {
        need(
            m["provider"] == "minimax"
                && v.as_u64().is_some_and(|n| n <= 100)
                && p["output_format"] != "png",
            "unsupported_provider_image_compression",
        )?;
    }
    need(
        refs.len() <= 16 && refs.iter().all(|r| r["role"] == "image"),
        "invalid_native_media_references",
    )?;
    need(
        refs.is_empty() || p["disable_prompt_enhanced"] != true,
        "unsupported_edit_prompt_control",
    )?;
    for r in refs {
        need(
            r["bytes"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 64 * 1024 * 1024),
            "invalid_reference_size",
        )?;
        let w = r["width"]
            .as_u64()
            .ok_or(Error("invalid_reference_dimensions"))?;
        let h = r["height"]
            .as_u64()
            .ok_or(Error("invalid_reference_dimensions"))?;
        need(
            (128..=8192).contains(&w) && (128..=8192).contains(&h) && w.max(h) <= 4 * w.min(h),
            "invalid_reference_dimensions",
        )?;
    }
    Ok(())
}
pub fn body(spec: &Value, m: &Value, urls: &[String]) -> Result<(String, Value)> {
    let mut p = spec["input"]["parameters"].clone();
    check(
        &p,
        m,
        spec["input"]["references"]
            .as_array()
            .ok_or(Error("invalid_native_media_references"))?,
    )?;
    need(
        urls.len() == spec["input"]["references"].as_array().unwrap().len(),
        "reference_count_mismatch",
    )?;
    p["prompt"] = spec["input"]["prompt"].clone();
    match s(&m["provider"])? {
        "fal" => {
            let (w, h) = dimensions(&p)?.ok_or(Error("unsupported_provider_image_size"))?;
            let mut b = json!({"prompt":p["prompt"],"image_size":{"width":w,"height":h},"num_images":p["n"],"output_format":p["output_format"],"sync_mode":false});
            let root = s(&m["apiModel"])?
                .strip_suffix("/text-to-image")
                .ok_or(Error("unsupported_model"))?;
            if urls.is_empty() {
                b["enable_prompt_expansion"] = json!(p["disable_prompt_enhanced"] != true);
            } else {
                b["image_urls"] = json!(urls);
            }
            Ok((
                format!(
                    "/{root}/{}",
                    if urls.is_empty() {
                        "text-to-image"
                    } else {
                        "edit"
                    }
                ),
                b,
            ))
        }
        "openrouter" => {
            let disabled = p
                .as_object_mut()
                .unwrap()
                .remove("disable_prompt_enhanced")
                .unwrap();
            p["provider"] = json!({"only":["minimax"],"allow_fallbacks":false,"require_parameters":true,"options":{"minimax":{"disable_prompt_enhanced":disabled}}});
            if p["size"] == "auto" {
                p["resolution"] = p
                    .as_object_mut()
                    .unwrap()
                    .remove("area")
                    .unwrap_or(json!("4K"));
                if p.get("aspect_ratio").is_none() {
                    p["aspect_ratio"] = json!("auto");
                }
            }
            if p["size"] == "auto" {
                p.as_object_mut().unwrap().remove("size");
            }
            p["model"] = m["apiModel"].clone();
            if !urls.is_empty() {
                p["input_references"] = json!(
                    urls.iter()
                        .map(|u| json!({"type":"image_url","image_url":{"url":u}}))
                        .collect::<Vec<_>>()
                );
            }
            Ok(("/api/v1/images".into(), p))
        }
        "minimax" => {
            if !urls.is_empty() {
                p.as_object_mut().unwrap().remove("disable_prompt_enhanced");
                p["images"] = json!(
                    urls.iter()
                        .map(|u| json!({"image_url":u}))
                        .collect::<Vec<_>>()
                );
            }
            p["model"] = m["apiModel"].clone();
            p["response_format"] = json!("url");
            p["stream"] = json!(false);
            Ok((
                if urls.is_empty() {
                    "/v1/images/generations"
                } else {
                    "/v1/images/edits"
                }
                .into(),
                p,
            ))
        }
        _ => Err(Error("unsupported_provider")),
    }
}
pub fn studio_native(spec: &Value) -> Result<Value> {
    let v = &spec["input"];
    let p = &v["parameters"];
    need(
        p["quality"] == "medium" && p["editPrecision"] == "none" && p["maskIndex"] == -1,
        "unsupported_h3_image_control",
    )?;
    let prompt = if s(&p["negative"])?.trim().is_empty() {
        s(&v["prompt"])?.trim().to_owned()
    } else {
        format!(
            "{}\n\nAvoid: {}.",
            s(&v["prompt"])?.trim(),
            s(&p["negative"])?.trim()
        )
    };
    let ratio = s(&v["aspect"])?
        .split(':')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error("invalid_media_aspect"))?;
    let area = match s(&v["resolution"])? {
        "1k" => 1048576.,
        "2k" => 4194304.,
        "4k" => 16777216.,
        _ => return Err(Error("invalid_media_resolution")),
    };
    let w = ((area * ratio[0] / ratio[1]).sqrt() / 32.).round() as u64 * 32;
    let h = ((area * ratio[1] / ratio[0]).sqrt() / 32.).round() as u64 * 32;
    let model = if s(&spec["model"])?.contains("/highspeed/") {
        "direct/minimax/h3-image-highspeed"
    } else {
        "direct/minimax/h3-image"
    };
    crate::native_media::spec(
        &json!({"model":model,"prompt":prompt,"references":v["references"],"parameters":{"size":format!("{w}x{h}"),"n":1,"output_format":"jpeg","disable_prompt_enhanced":v["promptExpansion"]=="disabled"}}),
    )
}
