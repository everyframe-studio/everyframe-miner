//! Reviewed Alibaba Model Studio and ByteDance Ark contracts. Region, model,
//! transport and output policy are release data, never supplied by a job.
use crate::{Error, Result, native_schema, need, s};
use serde_json::{Value, json};

pub fn check(spec: &Value, model: &Value) -> Result<()> {
    let p = &spec["input"]["parameters"];
    native_schema::validate(&model["parameterSchema"], p)?;
    let refs = spec["input"]["references"]
        .as_array()
        .ok_or(Error("invalid_native_media_references"))?;
    let cap = &model["nativeCapabilities"];
    if model["seedance20"] == true {
        crate::seedance20::references(refs, s(&model["provider"])?)?;
    }
    let count = |role: &str| refs.iter().filter(|r| r["role"] == role).count();
    if model["kind"] == "image" {
        need(
            refs.iter().all(|r| r["role"] == "image")
                && refs.len() <= cap["maxImages"].as_u64().unwrap() as usize,
            "invalid_native_media_references",
        )?;
        let (w, h) = s(&p["size"])?
            .split_once('x')
            .ok_or(Error("invalid_native_image_size"))?;
        let w: u64 = w.parse().map_err(|_| Error("invalid_native_image_size"))?;
        let h: u64 = h.parse().map_err(|_| Error("invalid_native_image_size"))?;
        need(
            w <= 4096
                && h <= 4096
                && w.is_multiple_of(16)
                && h.is_multiple_of(16)
                && w * h >= cap["minPixels"].as_u64().unwrap()
                && w * h <= cap["maxPixels"].as_u64().unwrap()
                && w.max(h) <= 3 * w.min(h),
            "invalid_native_image_size",
        )?;
    } else {
        need(
            refs.iter().all(|r| {
                ["first", "last", "image", "video", "audio"]
                    .iter()
                    .any(|k| r["role"] == *k)
            }),
            "invalid_reference_role",
        )?;
        let frames = count("first") + count("last");
        need(
            count("first") <= 1
                && count("last") <= 1
                && (count("last") == 0 || count("first") == 1)
                && (frames == 0 || frames == refs.len()),
            "invalid_reference_modes",
        )?;
        for (role, limit) in [
            ("image", "maxImages"),
            ("video", "maxVideos"),
            ("audio", "maxAudios"),
        ] {
            need(
                count(role) <= cap[limit].as_u64().unwrap() as usize,
                "invalid_native_media_references",
            )?;
        }
        let maximum = cap["maxReferenceDurationMs"].as_u64().unwrap();
        let minimum = if model["provider"] == "alibaba" {
            1000
        } else {
            2000
        };
        for role in ["video", "audio"] {
            let mut total = 0u64;
            for r in refs.iter().filter(|r| r["role"] == role) {
                let d = r["durationMs"]
                    .as_u64()
                    .ok_or(Error("invalid_reference_duration"))?;
                need(
                    (minimum..=maximum).contains(&d),
                    "invalid_reference_duration",
                )?;
                total += d;
            }
            need(total <= maximum, "invalid_reference_duration")?;
            if role == "video" && model["provider"] == "alibaba" && total > 0 {
                need(
                    total + p["duration"].as_u64().unwrap() * 1000 <= 30000,
                    "invalid_reference_duration",
                )?;
            }
        }
    }
    for r in refs {
        let audio = r["role"] == "audio";
        if !audio {
            let w = r["width"]
                .as_u64()
                .ok_or(Error("invalid_reference_dimensions"))?;
            let h = r["height"]
                .as_u64()
                .ok_or(Error("invalid_reference_dimensions"))?;
            need(
                w >= 300 && h >= 300 && w <= 6000 && h <= 6000 && w * 5 >= h * 2 && h * 5 >= w * 2,
                "invalid_reference_dimensions",
            )?;
        }
        let limit = if r["role"] == "video" {
            128
        } else if audio {
            20
        } else if model["provider"] == "alibaba" {
            10
        } else {
            30
        };
        need(
            r["bytes"]
                .as_u64()
                .is_some_and(|n| n <= limit * 1024 * 1024),
            "invalid_reference_size",
        )?;
    }
    Ok(())
}

pub fn body(spec: &Value, model: &Value, urls: &[String]) -> Result<Value> {
    check(spec, model)?;
    let refs = spec["input"]["references"].as_array().unwrap();
    need(refs.len() == urls.len(), "reference_count_mismatch")?;
    let p = &spec["input"]["parameters"];
    let prompt = &spec["input"]["prompt"];
    if model["kind"] == "image" {
        let mut b = p.clone();
        b["model"] = model["apiModel"].clone();
        b["prompt"] = prompt.clone();
        b["response_format"] = json!(if model["provider"] == "alibaba" {
            "url"
        } else {
            "b64_json"
        });
        if model["provider"] != "alibaba" {
            b.as_object_mut().unwrap().remove("n");
        }
        if !urls.is_empty() {
            b["image"] = if urls.len() == 1 {
                json!(urls[0])
            } else {
                json!(urls)
            };
        }
        return Ok(b);
    }
    let role = |r: &Value| match r["role"].as_str().unwrap() {
        "first" => "first_frame",
        "last" => "last_frame",
        "image" => "reference_image",
        "video" => "reference_video",
        _ => "reference_audio",
    };
    if model["provider"] == "alibaba" {
        let mut input = json!({"prompt":prompt});
        if !urls.is_empty() {
            input["media"] = json!(
                refs.iter()
                    .zip(urls)
                    .map(|(r, u)| json!({"type":role(r),"url":u}))
                    .collect::<Vec<_>>()
            );
        }
        let mut params = json!({"duration":p["duration"],"resolution":s(&p["resolution"])?.to_ascii_uppercase(),"ratio":p["aspect_ratio"],"audio":p["generate_audio"],"prompt_extend":false});
        if let Some(seed) = p.get("seed") {
            params["seed"] = seed.clone();
        }
        Ok(json!({"model":model["apiModel"],"input":input,"parameters":params}))
    } else {
        let mut content = vec![json!({"type":"text","text":prompt})];
        for (r, u) in refs.iter().zip(urls) {
            let kind = match r["role"].as_str().unwrap() {
                "video" => "video_url",
                "audio" => "audio_url",
                _ => "image_url",
            };
            let mut item = json!({"type":kind,"role":role(r)});
            item[kind] = json!({"url":u});
            content.push(item);
        }
        let mut b = json!({"model":model["apiModel"],"content":content,"duration":p["duration"],"resolution":p["resolution"],"ratio":p["aspect_ratio"],"generate_audio":p["generate_audio"],"watermark":false});
        if let Some(seed) = p.get("seed") {
            b["seed"] = seed.clone();
        }
        Ok(b)
    }
}
