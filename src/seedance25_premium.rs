//! Seedance 2.5 Premium is a separate, allowlisted BytePlus contract.
//! Normal renders use 4K; the provider's separate Draft workflow is not a
//! lower-resolution substitute for a purchased final render.
use crate::{Result, need, s};
use serde_json::Value;

pub fn check(spec: &Value) -> Result<()> {
    let p = &spec["input"]["parameters"];
    let refs = spec["input"]["references"].as_array().unwrap();
    let count = |role: &str| refs.iter().filter(|r| r["role"] == role).count();
    let frames = count("first") + count("last");
    need(
        refs.len() <= 50 && count("image") <= 30 && count("video") <= 10 && count("audio") <= 10,
        "invalid_native_media_references",
    )?;
    need(
        count("first") <= 1
            && count("last") <= 1
            && (count("last") == 0 || count("first") == 1)
            && (frames == 0 || frames == refs.len())
            && refs.iter().all(|r| {
                ["first", "last", "image", "video", "audio"]
                    .contains(&r["role"].as_str().unwrap_or(""))
            }),
        "invalid_reference_modes",
    )?;
    let task = s(&p["omni_reference_task_type"])?;
    let video_task = ["edit", "extend"].contains(&task);
    need(
        !video_task || count("video") > 0,
        "reference_video_required",
    )?;
    need(
        !(frames > 0 || video_task) || p["aspect_ratio"] == "adaptive",
        "provider_derived_aspect_required",
    )?;
    need(
        task != "edit" || p["duration"] == -1,
        "provider_derived_duration_required",
    )?;
    // With video + auto, prompt intent may select editing. Require derived
    // settings explicitly, so a fixed duration/aspect is never silently lost.
    need(
        task != "auto"
            || count("video") == 0
            || (p["duration"] == -1 && p["aspect_ratio"] == "adaptive"),
        "provider_derived_settings_required",
    )?;
    for role in ["video", "audio"] {
        let mut total = 0u64;
        for r in refs.iter().filter(|r| r["role"] == role) {
            let d = r["durationMs"].as_u64().unwrap_or(0);
            need(
                (if task == "edit" && role == "video" {
                    4000
                } else {
                    2000
                }..=30000)
                    .contains(&d),
                "invalid_reference_duration",
            )?;
            total += d;
        }
        need(total <= 30000, "invalid_reference_duration")?;
    }
    for r in refs {
        let role = s(&r["role"])?;
        let bytes = r["bytes"].as_u64().unwrap_or(0);
        if role == "audio" {
            need(bytes <= 15 * 1024 * 1024, "invalid_reference_size")?;
        } else {
            let (w, h) = (
                r["width"].as_u64().unwrap_or(0),
                r["height"].as_u64().unwrap_or(0),
            );
            need(
                (300..=6000).contains(&w)
                    && (300..=6000).contains(&h)
                    && w * 5 >= h * 2
                    && h * 5 >= w * 2,
                "invalid_reference_dimensions",
            )?;
            if role == "video" {
                need(
                    (407696..=8295044).contains(&(w * h)),
                    "invalid_reference_dimensions",
                )?;
                need(bytes <= 200 * 1024 * 1024, "invalid_reference_size")?;
            } else {
                need(bytes < 30 * 1024 * 1024, "invalid_reference_size")?;
            }
        }
    }
    Ok(())
}
