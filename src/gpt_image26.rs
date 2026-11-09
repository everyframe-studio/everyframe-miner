//! GPT Image 2.6 contracts. Only documented provider fields reach the wire.
use crate::{Error, Result, need, s};
use serde_json::{Value, json};

pub fn check(p: &Value, m: &Value, refs: &[Value]) -> Result<()> {
    need(
        refs.iter()
            .all(|r| r["bytes"].as_u64().is_some_and(|n| n <= 50 * 1024 * 1024)),
        "invalid_reference_size",
    )?;
    let c = &m["nativeCapabilities"];
    let or = m["provider"] == "openrouter";
    let fields = [
        "size",
        "aspect_ratio",
        "quality",
        "background",
        "output_format",
        "output_compression",
        "n",
        "complexity",
        "moderation",
        "frame_count",
        "frame_duration_ms",
        "svg_model",
    ];
    need(
        p.as_object()
            .is_some_and(|o| o.keys().all(|k| fields.contains(&k.as_str()))),
        "invalid_native_media_fields",
    )?;
    let listed = |k: &str, options: &Value| options.as_array().is_some_and(|a| a.contains(&p[k]));
    need(
        listed("quality", &c["qualities"])
            && listed("output_format", &c["formats"])
            && listed("complexity", &c["complexities"]),
        "invalid_native_image_format",
    )?;
    let count = p["n"]
        .as_u64()
        .filter(|n| (1..=10).contains(n))
        .ok_or(Error("invalid_native_image_count"))?;
    need(
        ["auto", "transparent", "opaque"].contains(&s(&p["background"])?),
        "invalid_native_image_background",
    )?;
    need(
        p["background"] != "transparent" || ["png", "webp"].contains(&s(&p["output_format"])?),
        "invalid_native_image_background",
    )?;
    if let Some(v) = p.get("output_compression") {
        need(
            ["jpeg", "webp"].contains(&s(&p["output_format"])?)
                && v.as_u64().is_some_and(|n| n <= 100),
            "invalid_native_image_compression",
        )?;
    }
    if let Some(v) = p.get("moderation") {
        need(
            ["auto", "low"].contains(&s(v)?),
            "invalid_native_image_moderation",
        )?;
    }
    if or {
        need(
            p.get("size").is_none() && listed("aspect_ratio", &c["aspects"]),
            "invalid_native_image_size",
        )?;
        need(
            p.get("frame_count").is_none() && p.get("frame_duration_ms").is_none(),
            "unsupported_animation_controls",
        )?;
    } else {
        need(p.get("aspect_ratio").is_none(), "invalid_native_image_size")?;
        let size = s(&p["size"])?;
        if size != "auto" {
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
                w > 0
                    && h > 0
                    && w <= 3840
                    && h <= 3840
                    && w % 16 == 0
                    && h % 16 == 0
                    && (655360..=8294400).contains(&(w * h))
                    && w.max(h) <= 3 * w.min(h),
                "invalid_native_image_size",
            )?;
        }
    }
    let images = refs.iter().filter(|r| r["role"] == "image").count();
    let mask = refs.iter().filter(|r| r["role"] == "mask").count();
    let markup = refs.iter().filter(|r| r["role"] == "markup").count();
    need(
        images <= 16
            && mask + markup <= 1
            && refs.len() == images + mask + markup
            && (mask + markup == 0 || images > 0)
            && (!or || mask + markup == 0),
        "invalid_native_media_references",
    )?;
    for r in refs
        .iter()
        .filter(|r| r["role"] == "mask" || r["role"] == "markup")
    {
        need(r["mime"] == "image/png", "invalid_reference_type")?;
        if r["role"] == "markup" {
            let first = refs.iter().find(|r| r["role"] == "image").unwrap();
            need(
                r["width"].as_u64().is_some_and(|w| w > 0)
                    && r["height"].as_u64().is_some_and(|h| h > 0)
                    && r["width"] == first["width"]
                    && r["height"] == first["height"]
                    && r["width"]
                        .as_u64()
                        .unwrap()
                        .checked_mul(r["height"].as_u64().unwrap())
                        .is_some_and(|n| n <= 16777216)
                    && r["bytes"].as_u64().is_some_and(|n| n <= 50 * 1024 * 1024),
                "invalid_markup_reference",
            )?;
        }
    }
    if p["output_format"] == "svg" {
        need(refs.is_empty() && count == 1, "invalid_svg_request")?;
    }
    if let Some(v) = p.get("svg_model") {
        need(
            m["provider"] == "fal"
                && p["output_format"] == "svg"
                && [
                    "gpt-6-astra",
                    "gpt-5.5",
                    "gpt-5.4",
                    "gpt-5.6-terra",
                    "gpt-6-luna",
                ]
                .contains(&s(v)?),
            "invalid_svg_request",
        )?;
    }
    for (key, min, max) in [("frame_count", 2, 16), ("frame_duration_ms", 20, 2000)] {
        if let Some(v) = p.get(key) {
            need(
                p["output_format"] == "gif" && v.as_u64().is_some_and(|n| (min..=max).contains(&n)),
                "invalid_animation_request",
            )?;
        }
    }
    need(
        p["output_format"] != "gif" || mask == 0,
        "invalid_animation_request",
    )?;
    Ok(())
}

pub fn body(spec: &Value, m: &Value, urls: &[String]) -> Result<(String, Value)> {
    let x = &spec["input"];
    let refs = x["references"].as_array().unwrap();
    need(refs.len() == urls.len(), "reference_count_mismatch")?;
    let mut p = x["parameters"].clone();
    let images: Vec<_> = refs
        .iter()
        .zip(urls)
        .filter(|(r, _)| r["role"] == "image")
        .map(|(_, u)| u.clone())
        .collect();
    p["prompt"] = x["prompt"].clone();
    match s(&m["provider"])? {
        "fal" => {
            let o = p.as_object_mut().unwrap();
            let size = o.remove("size").unwrap();
            p["image_size"] = if size == "auto" {
                size
            } else {
                let (w, h) = s(&size)?.split_once('x').unwrap();
                json!({"width":w.parse::<u64>().unwrap(),"height":h.parse::<u64>().unwrap()})
            };
            p["num_images"] = p.as_object_mut().unwrap().remove("n").unwrap();
            p["reasoning_effort"] = p.as_object_mut().unwrap().remove("complexity").unwrap();
            p["enable_safety_checker"] = json!(
                p.as_object_mut()
                    .unwrap()
                    .remove("moderation")
                    .is_none_or(|v| v != "low")
            );
            for (role, key) in [("mask", "mask_url"), ("markup", "markup_image_url")] {
                if let Some((_, url)) = refs.iter().zip(urls).find(|(r, _)| r["role"] == role) {
                    p[key] = json!(url);
                }
            }
            let path = s(&m["apiModel"])?;
            if images.is_empty() {
                Ok((format!("/{path}"), p))
            } else {
                p["image_urls"] = json!(images);
                Ok((
                    format!("/{}/edit", path.strip_suffix("/text-to-image").unwrap()),
                    p,
                ))
            }
        }
        "openrouter" => {
            p["model"] = m["apiModel"].clone();
            let mut options =
                json!({"complexity":p.as_object_mut().unwrap().remove("complexity").unwrap()});
            if let Some(v) = p.as_object_mut().unwrap().remove("moderation") {
                options["moderation"] = v;
            }
            p["provider"] =
                json!({"only":["openai"],"allow_fallbacks":false,"options":{"openai":options}});
            if !images.is_empty() {
                p["input_references"] = json!(
                    images
                        .iter()
                        .map(|u| json!({"type":"image_url","image_url":{"url":u}}))
                        .collect::<Vec<_>>()
                );
            }
            Ok(("/api/v1/images".into(), p))
        }
        "openai" => {
            p["model"] = m["apiModel"].clone();
            for role in ["mask", "markup"] {
                if let Some((_, url)) = refs.iter().zip(urls).find(|(r, _)| r["role"] == role) {
                    p[role] = json!({"image_url":url});
                }
            }
            if images.is_empty() {
                Ok(("/v1/images/generations".into(), p))
            } else {
                p["images"] = json!(
                    images
                        .iter()
                        .map(|u| json!({"image_url":u}))
                        .collect::<Vec<_>>()
                );
                Ok(("/v1/images/edits".into(), p))
            }
        }
        _ => Err(Error("unsupported_provider")),
    }
}
