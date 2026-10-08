use crate::{
    Error, Result, n, need,
    protocol::{digest, exact},
    s,
};
use serde_json::{Value, json};
use std::sync::LazyLock;
pub const DEFAULT_MODEL: &str = "minimax/h3-max-turbo/text-to-video";
pub const MAX_MEDIA: usize = 50 * 1024 * 1024;
pub static MODELS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/models.json"))
        .expect("embedded reviewed model registry")
});
pub fn info(id: &str) -> Result<&'static Value> {
    MODELS.get(id).ok_or(Error("unsupported_model"))
}
// Explicit integer durations; Fal schemas checked 2026-10-08. Five-second
// canonical bodies remain unchanged for existing orders and signed fixtures.
pub fn duration_range(model: &str) -> Option<(i64, i64)> {
    match model {
        "minimax/h3-max-turbo/text-to-video" | "minimax/h3-max/text-to-video" => Some((1, 15)),
        "bytedance/seedance-2.5/text-to-video" => Some((4, 30)),
        _ => None,
    }
}
pub fn spec(v: &Value) -> Result<Value> {
    let model = v.get("model").map(s).transpose()?.unwrap_or(DEFAULT_MODEL);
    let m = info(model)?;
    let mut keys = vec!["prompt"];
    if v.get("model").is_some() {
        keys.push("model")
    };
    let seed = m["seed"] == true;
    if seed {
        keys.push("seed")
    };
    if v.get("duration").is_some() {
        keys.push("duration");
        let duration = n(&v["duration"])?;
        let (min, max) = duration_range(model).ok_or(Error("invalid_duration"))?;
        need((min..=max).contains(&duration), "invalid_duration")?;
    }
    exact(v, &keys)?;
    let prompt = s(&v["prompt"])?;
    need(
        !prompt.trim().is_empty() && prompt.len() <= 8000,
        "invalid_prompt",
    )?;
    need(
        prompt.encode_utf16().count() <= n(&m["maxPrompt"])? as usize,
        "prompt_too_long",
    )?;
    let mut input = json!({"prompt":prompt});
    if seed {
        let seed = n(&v["seed"])?;
        need((0..2147483648).contains(&seed), "invalid_seed")?;
        input["seed"] = json!(seed)
    };
    for (k, v) in m["params"].as_object().ok_or(Error("invalid_model"))? {
        input[k] = v.clone()
    }
    if let Some(duration) = v.get("duration") {
        input["duration"] = if m["params"]["duration"].is_string() {
            json!(n(duration)?.to_string())
        } else {
            duration.clone()
        };
    }
    Ok(json!({"model":model,"input":input}))
}
pub fn check(v: &Value) -> Result<()> {
    let model = s(&v["model"])?;
    let mut input = json!({"model":model,"prompt":v["input"]["prompt"]});
    if info(model)?["seed"] == true {
        input["seed"] = v["input"]["seed"].clone()
    };
    if duration_range(model).is_some() {
        input["duration"] = if info(model)?["params"]["duration"].is_string() {
            json!(
                s(&v["input"]["duration"])?
                    .parse::<i64>()
                    .map_err(|_| Error("invalid_duration"))?
            )
        } else {
            v["input"]["duration"].clone()
        };
    }
    need(digest(v)? == digest(&spec(&input)?)?, "unsupported_spec")
}
pub fn max_job_ms() -> i64 {
    MODELS
        .as_object()
        .unwrap()
        .values()
        .filter_map(|m| m["deadlineMs"].as_i64())
        .max()
        .unwrap_or(480000)
}
