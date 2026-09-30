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
    Ok(json!({"model":model,"input":input}))
}
pub fn check(v: &Value) -> Result<()> {
    let model = s(&v["model"])?;
    let mut input = json!({"model":model,"prompt":v["input"]["prompt"]});
    if info(model)?["seed"] == true {
        input["seed"] = v["input"]["seed"].clone()
    };
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
