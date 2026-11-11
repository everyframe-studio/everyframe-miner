//! Google Interactions API for the reviewed direct-media contracts.
//! A durable interaction ID permits GET-only recovery; a POST is never replayed.
use crate::{Error, Result, models::MAX_MEDIA, need, protocol, s};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub const ORIGIN: &str = "https://generativelanguage.googleapis.com";
pub const JSON_LIMIT: usize = MAX_MEDIA * 4 / 3 + 1024 * 1024;

pub fn reference(spec: &Value, id: &str) -> Result<String> {
    need(
        crate::matches("[A-Za-z0-9_-]{1,100}", &json!(id)),
        "invalid_provider_ref",
    )?;
    Ok(format!(
        "google_ix_{}_{}",
        &protocol::digest(spec)?[..32],
        id
    ))
}
pub fn reference_id<'a>(spec: &Value, reference: &'a str) -> Result<&'a str> {
    let prefix = format!("google_ix_{}_", &protocol::digest(spec)?[..32]);
    let id = reference
        .strip_prefix(&prefix)
        .ok_or(Error("provider_ref_namespace"))?;
    need(
        crate::matches("[A-Za-z0-9_-]{1,100}", &json!(id)),
        "invalid_provider_ref",
    )?;
    Ok(id)
}

pub fn body(spec: &Value, model: &Value) -> Result<Value> {
    body_with_references(spec, model, &[])
}
pub fn body_with_references(spec: &Value, model: &Value, refs: &[Value]) -> Result<Value> {
    crate::models::check(spec)?;
    need(
        refs.len() == spec["input"]["references"].as_array().map_or(0, Vec::len),
        "reference_count_mismatch",
    )?;
    need(
        model["interaction"] == true && model["provider"] == "google",
        "unsupported_model",
    )?;
    let mut body = spec["input"].clone();
    // Google image and audio generation return inline bytes by default,
    // but reject an explicit delivery="inline" on these routes. Normalize only the outgoing request: changing
    // the signed input would invalidate existing job hashes and recovery IDs.
    // Keep explicit URI requests intact rather than silently changing delivery.
    fn media_delivery(format: &mut Value) {
        if matches!(format["type"].as_str(), Some("image" | "audio"))
            && format["delivery"] == "inline"
        {
            format.as_object_mut().unwrap().remove("delivery");
        }
    }
    if let Some(formats) = body["response_format"].as_array_mut() {
        for format in formats {
            media_delivery(format);
        }
    } else {
        media_delivery(&mut body["response_format"]);
    }
    let prompt = body
        .as_object_mut()
        .ok_or(Error("unsupported_spec"))?
        .remove("prompt")
        .ok_or(Error("invalid_prompt"))?;
    body["model"] = model["apiModel"].clone();
    body.as_object_mut().unwrap().remove("references");
    let turns = body.as_object_mut().unwrap().remove("turns");
    let mut content = refs.to_vec();
    if let Some(turns) = turns {
        for t in turns.as_array().ok_or(Error("invalid_google_turns"))? {
            let mut part = json!({"type":"text","text":t["text"]});
            let mut annotation = json!({"type":"speech_metadata"});
            for k in ["speaker", "style"] {
                if let Some(v) = t.get(k) {
                    annotation[k] = v.clone();
                }
            }
            if annotation.as_object().unwrap().len() > 1 {
                part["annotations"] = json!([annotation]);
            }
            content.push(part);
        }
    } else {
        content.push(json!({"type":"text","text":prompt}));
    }
    for key in ["temperature", "top_p"] {
        if let Some(value) = body["generation_config"].get(key).cloned() {
            body["generation_config"][key] = json!(
                s(&value)?
                    .parse::<f64>()
                    .map_err(|_| Error("invalid_google_sampling"))?
            );
        }
    }
    body["input"] = json!([{"type":"user_input","content":content}]);
    Ok(body)
}

pub fn output_model(spec: &Value, model: &Value) -> Result<Value> {
    let mut m = model.clone();
    let f = crate::google_settings::format(&spec["input"], s(&m["kind"])?)?;
    if m["kind"] == "audio" {
        let mime = f["mime_type"]
            .as_str()
            .unwrap_or_else(|| model["mime"].as_str().unwrap_or("audio/wav"));
        m["mime"] = json!(if mime == "audio/mp3" {
            "audio/mpeg"
        } else {
            mime
        });
        if let Some(rate) = f.get("sample_rate") {
            m["sampleRate"] = rate.clone();
        }
    }
    Ok(m)
}

pub fn interaction_id(data: &Value, model: &Value, expected: Option<&str>) -> Result<String> {
    let id = s(&data["id"])?;
    need(
        crate::matches("[A-Za-z0-9_-]{1,100}", &data["id"]),
        "invalid_provider_ref",
    )?;
    need(expected.is_none_or(|v| v == id), "provider_id_mismatch")?;
    need(
        data["model"] == model["apiModel"],
        "provider_model_mismatch",
    )?;
    state(data)?;
    Ok(id.into())
}

pub fn state(data: &Value) -> Result<&'static str> {
    match data["status"].as_str() {
        Some("queued") => Ok("IN_QUEUE"),
        Some("in_progress") => Ok("IN_PROGRESS"),
        Some("completed") if data["error"].is_null() => Ok("COMPLETED"),
        Some(
            "failed" | "cancelled" | "incomplete" | "requires_action" | "requires_continuation",
        ) => Ok("FAILED"),
        _ => Err(Error("invalid_provider_status")),
    }
}

/// Only accept one final media block, never thought/user-input media.
pub fn media<'a>(data: &'a Value, model: &Value) -> Result<&'a Value> {
    let blocks = media_blocks(data, model)?;
    need(blocks.len() == 1, "provider_no_media")?;
    Ok(blocks[0])
}
pub fn media_blocks<'a>(data: &'a Value, model: &Value) -> Result<Vec<&'a Value>> {
    need(state(data)? == "COMPLETED", "provider_not_complete")?;
    let kind = s(&model["kind"])?;
    let steps = data["steps"].as_array().ok_or(Error("provider_no_media"))?;
    let blocks: Vec<_> = steps
        .iter()
        .filter(|v| v["type"] == "model_output")
        .filter_map(|v| v["content"].as_array())
        .flatten()
        .filter(|v| v["type"] == kind)
        .collect();
    need(!blocks.is_empty(), "provider_no_media")?;
    for block in &blocks {
        let mime = s(&block["mime_type"])?
            .split(';')
            .next()
            .unwrap()
            .to_ascii_lowercase();
        need(
            mime == s(&model["mime"])?
                || (model["mime"] == "audio/mpeg" && mime == "audio/mp3")
                || (model["mime"] == "audio/wav" && mime == "audio/l16"),
            "invalid_provider_media",
        )?;
        need(
            block["data"].is_string() != block["uri"].is_string(),
            "invalid_provider_media",
        )?;
    }
    Ok(blocks)
}

pub fn inline(block: &Value) -> Result<Vec<u8>> {
    let data = s(&block["data"])?;
    need(
        !data.is_empty() && data.len() <= MAX_MEDIA * 4 / 3 + 4,
        "invalid_media_size",
    )?;
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| Error("invalid_provider_media"))?;
    need(
        !bytes.is_empty() && bytes.len() <= MAX_MEDIA,
        "invalid_media_size",
    )?;
    Ok(bytes)
}

pub fn download_url(block: &Value) -> Result<String> {
    let raw = s(&block["uri"])?;
    let value = if crate::matches("files/[A-Za-z0-9_-]+", &json!(raw)) {
        format!("{ORIGIN}/v1beta/{raw}")
    } else {
        raw.to_string()
    };
    let mut url = crate::network::safe_url(&value, &["generativelanguage.googleapis.com".into()])?;
    if crate::matches("/v1beta/files/[A-Za-z0-9_-]+", &json!(url.path())) && url.query().is_none() {
        url.set_path(&format!("{}:download", url.path()));
        url.set_query(Some("alt=media"));
    }
    need(
        crate::matches(r"/v1beta/files/[A-Za-z0-9_-]+:download", &json!(url.path()))
            && url.query() == Some("alt=media"),
        "unapproved_download_path",
    )?;
    Ok(url.to_string())
}

pub fn response_hash(spec: &Value, data: &Value, media: &[u8]) -> Result<String> {
    protocol::digest(&json!({"provider":"google","interactionId":data["id"],
        "inputHash":protocol::digest(spec)?,"responseHash":protocol::sha(data.to_string()),"outputHash":protocol::sha(media)}))
}

/// The background TTS endpoint can return PCM16 even when WAV was requested.
/// Wrap the provider's actual sample bytes; never relabel raw PCM as a WAV.
pub fn normalize_media(block: &Value, model: &Value, bytes: Vec<u8>) -> Result<Vec<u8>> {
    let mime = s(&block["mime_type"])?.split(';').next().unwrap();
    if model["mime"] == "audio/wav" && mime == "audio/l16" {
        let rate = block["sample_rate"]
            .as_u64()
            .or_else(|| model["sampleRate"].as_u64())
            .unwrap_or(24000);
        let expected = model["sampleRate"].as_u64().unwrap_or(rate);
        need(rate == expected, "invalid_provider_media")?;
        let channels = block["channels"].as_u64().unwrap_or(1);
        return crate::wav::pcm16_wav(bytes, rate, channels);
    }
    Ok(bytes)
}
