//! Endpoint-specific Seedance 2.0 limits, shared by direct and Studio requests.
use crate::{Result, n, need, s};
use serde_json::Value;

pub fn references(refs: &[Value], provider: &str) -> Result<()> {
    need(refs.len() <= 12, "invalid_native_media_references")?;
    let count = |role: &str| refs.iter().filter(|r| r["role"] == role).count();
    need(
        count("image") <= 9 && count("video") <= 3 && count("audio") <= 3,
        "invalid_native_media_references",
    )?;
    need(
        count("audio") == 0 || count("image") + count("video") > 0,
        "audio_needs_visual_reference",
    )?;
    let mut video_bytes = 0;
    for role in ["video", "audio"] {
        let mut total = 0;
        for r in refs.iter().filter(|r| r["role"] == role) {
            let duration = n(&r["durationMs"])?;
            need(
                (2000..=15000).contains(&duration),
                "invalid_reference_duration",
            )?;
            total += duration;
        }
        need(total <= 15000, "invalid_reference_duration")?;
    }
    for r in refs {
        let role = s(&r["role"])?;
        let mime = s(&r["mime"])?;
        let bytes = n(&r["bytes"])?;
        if role == "audio" {
            need(
                ["audio/mpeg", "audio/wav"].contains(&mime),
                "invalid_reference_mime",
            )?;
            need(bytes <= 15 * 1024 * 1024, "invalid_reference_size")?;
        } else {
            let (w, h) = (n(&r["width"])?, n(&r["height"])?);
            need(
                (300..=6000).contains(&w)
                    && (300..=6000).contains(&h)
                    && w * 5 >= h * 2
                    && h * 5 >= w * 2,
                "invalid_reference_dimensions",
            )?;
            if role == "video" {
                need(
                    ["video/mp4", "video/quicktime"].contains(&mime),
                    "invalid_reference_mime",
                )?;
                let maximum = if provider == "fal" { 927408 } else { 8295044 };
                need(
                    (407696..=maximum).contains(&(w * h)),
                    "invalid_reference_dimensions",
                )?;
                video_bytes += bytes;
                need(bytes <= 200 * 1024 * 1024, "invalid_reference_size")?;
            } else {
                need(bytes < 30 * 1024 * 1024, "invalid_reference_size")?;
            }
        }
    }
    need(
        provider != "fal" || video_bytes < 50 * 1024 * 1024,
        "invalid_reference_size",
    )?;
    Ok(())
}
