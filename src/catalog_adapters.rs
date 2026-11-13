//! Data definitions for existing execution engines; this module adds no transport.
use crate::{Error, Result, models, native_media, s};
use serde_json::{Value, json};
pub fn policy(id: &str, adapter: &str) -> Option<&'static Value> {
    crate::worker_catalog::contract(id)
        .filter(|m| m["catalogAdapter"] == adapter)
        .and_then(|m| m.get("catalogPolicy"))
}
pub fn compiled(id: &str) -> Result<Value> {
    let mut m = models::MODELS
        .get(id)
        .or_else(|| models::STUDIO_MODELS.get(id))
        .ok_or(Error("unsupported_model"))?
        .clone();
    let (adapter, p) = if let Some(p) = crate::google_settings::CAPS.get(id) {
        ("google-interactions", p)
    } else if let Some(p) = crate::studio_video::POLICIES.get(id) {
        ("studio-video", p)
    } else if let Some(p) = crate::studio_media::POLICIES.get(id) {
        ("studio-media", p)
    } else if let Some(p) = crate::studio_tools::POLICIES.get(id) {
        ("studio-tool", p)
    } else {
        return Ok(m);
    };
    m["catalogAdapter"] = json!(adapter);
    m["catalogPolicy"] = p.clone();
    Ok(m)
}
pub fn baseline_hash(id: &str, m: &Value) -> Result<String> {
    let d = compiled(id)?;
    if d["catalogAdapter"]
        .as_str()
        .is_some_and(|a| a.starts_with("studio-"))
    {
        crate::worker_catalog::fingerprint(
            &json!({"catalogAdapter":d["catalogAdapter"],"catalogPolicy":d["catalogPolicy"]}),
        )
    } else if d.get("catalogAdapter").is_some() {
        crate::worker_catalog::fingerprint(&d)
    } else {
        crate::worker_catalog::fingerprint(m)
    }
}
pub fn check_example(example: &Value, m: &Value) -> Result<()> {
    let refs = example["input"]["references"]
        .as_array()
        .map(|refs| {
            refs.iter()
                .map(|r| s(&r["url"]).map(str::to_owned))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let p = &m["catalogPolicy"];
    match m["catalogAdapter"].as_str() {
        Some("google-interactions") => crate::google_settings::check_with(example, p),
        Some("studio-video") => crate::studio_video::body_with(example, p, &refs).map(|_| ()),
        Some("studio-media") => crate::studio_media::body_with(example, p, &refs).map(|_| ()),
        Some("studio-tool") => crate::studio_tools::body_with(example, p, &refs).map(|_| ()),
        None if m["nativeMedia"] == true => native_media::body_with(example, m, &refs).map(|_| ()),
        _ => Err(Error("catalog_adapter_update_required")),
    }
}
pub fn policy_compatible(t: &Value, m: &Value) -> bool {
    // Preserve executable switches. Numeric limits, lists, defaults and provider
    // model names are data. No descriptor may select a new provider or task mode.
    if let Some(t) = t.as_object() {
        return m.as_object().is_some_and(|m| {
            m.iter().all(|(k, v)| {
                t.contains_key(k)
                    || (!v.is_boolean()
                        && !matches!(
                            k.as_str(),
                            "provider" | "kind" | "mode" | "adapter" | "apiPath"
                        ))
            }) && t.iter().all(|(k, v)| {
                m.get(k).is_some_and(|n| {
                    if matches!(
                        k.as_str(),
                        "provider" | "kind" | "mode" | "adapter" | "apiPath"
                    ) || v.is_boolean()
                    {
                        v == n
                    } else {
                        policy_compatible(v, n)
                    }
                })
            })
        });
    }
    if t.is_array() {
        return m.is_array();
    }
    if t.is_number() {
        return if t.is_u64() {
            m.as_u64().is_some()
        } else {
            m.is_number()
        };
    }
    if t.is_string() {
        return m.is_string();
    }
    t.is_null() || t == m
}

pub fn policy_identity(template: &Value, model: &Value) -> bool {
    let t = &template["catalogPolicy"];
    let m = &model["catalogPolicy"];
    if m.is_null() {
        return true;
    }
    if m.get("apiModel").is_some() && m["apiModel"] != model["apiModel"] {
        return false;
    }
    if m.get("provider").is_some() && m["provider"] != model["provider"] {
        return false;
    }
    if m.get("kind").is_some() && m["kind"] != model["kind"] {
        return false;
    }
    if t["path"] == m["path"] {
        return true;
    }
    let (Some(old), Some(new), Some(old_path), Some(path)) = (
        t["apiModel"].as_str(),
        m["apiModel"].as_str(),
        t["path"].as_str(),
        m["path"].as_str(),
    ) else {
        return false;
    };
    (old_path == old && path == new)
        || (old_path == old.trim_start_matches("fal-ai/")
            && path == new.trim_start_matches("fal-ai/"))
}

pub fn definition_hash(m: &Value) -> Result<String> {
    if m["catalogAdapter"]
        .as_str()
        .is_some_and(|a| a.starts_with("studio-"))
    {
        crate::worker_catalog::fingerprint(
            &json!({"catalogAdapter":m["catalogAdapter"],"catalogPolicy":m["catalogPolicy"]}),
        )
    } else {
        crate::worker_catalog::fingerprint(m)
    }
}
