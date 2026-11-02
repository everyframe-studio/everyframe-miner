//! Provider-supported Google settings, separate from transport and credentials.
use crate::{Error, Result, n, need, protocol::exact, s};
use serde_json::{Value, json};
use std::sync::LazyLock;
pub static CAPS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/google-capabilities.json")).unwrap()
});
fn fields(v: &Value, keys: &[&str]) -> Result<()> {
    need(
        v.as_object()
            .is_some_and(|o| o.keys().all(|k| keys.contains(&k.as_str()))),
        "invalid_google_fields",
    )
}
fn int(v: &Value, min: i64, max: i64) -> bool {
    v.as_i64().is_some_and(|n| n >= min && n <= max)
}
fn string(v: &Value, max: usize) -> bool {
    v.as_str()
        .is_some_and(|v| !v.is_empty() && v.encode_utf16().count() <= max)
}
fn one(v: &Value, items: &Value) -> bool {
    items.as_array().is_some_and(|a| a.contains(v))
}
pub fn formats(input: &Value) -> Vec<&Value> {
    if let Some(a) = input["response_format"].as_array() {
        a.iter().collect()
    } else {
        vec![&input["response_format"]]
    }
}
pub fn format<'a>(input: &'a Value, kind: &str) -> Result<&'a Value> {
    formats(input)
        .into_iter()
        .find(|f| f["type"] == kind)
        .ok_or(Error("invalid_google_format"))
}
pub fn check(v: &Value) -> Result<()> {
    exact(v, &["model", "input"])?;
    let c = &CAPS[s(&v["model"])?];
    need(c.is_object(), "unsupported_model")?;
    let x = &v["input"];
    fields(
        x,
        &[
            "prompt",
            "response_format",
            "generation_config",
            "store",
            "background",
            "references",
            "turns",
            "tools",
            "previous_interaction_id",
            "system_instruction",
        ],
    )?;
    need(
        x["store"] == true && x["background"] == true,
        "google_recovery_required",
    )?;
    need(
        string(&x["prompt"], n(&c["inputTokens"])? as usize * 8),
        "invalid_google_prompt",
    )?;
    if let Some(instruction) = x.get("system_instruction") {
        need(
            string(instruction, n(&c["inputTokens"])? as usize * 8),
            "invalid_google_instruction",
        )?;
    }
    let fs = formats(x);
    need(
        !fs.is_empty()
            && fs.len() <= 2
            && fs.iter().filter(|f| f["type"] == c["kind"]).count() == 1,
        "invalid_google_format",
    )?;
    if fs.len() == 2 {
        need(fs[0]["type"] != fs[1]["type"], "invalid_google_format")?;
    }
    for f in fs {
        if f["type"] == "text" {
            fields(f, &["type", "mime_type", "schema"])?;
            if let Some(mime) = f.get("mime_type") {
                need(
                    one(mime, &json!(["text/plain", "application/json"])),
                    "invalid_google_mime",
                )?;
            }
            if let Some(schema) = f.get("schema") {
                need(
                    f["mime_type"] == "application/json" && schema.is_object(),
                    "invalid_google_schema",
                )?;
            }
            continue;
        }
        need(f["type"] == c["kind"], "invalid_google_format")?;
        let kind = s(&c["kind"])?;
        fields(
            f,
            match kind {
                "video" => &["type", "delivery", "duration", "resolution", "aspect_ratio"],
                "image" => &[
                    "type",
                    "delivery",
                    "mime_type",
                    "image_size",
                    "aspect_ratio",
                ],
                _ => &["type", "delivery", "mime_type", "sample_rate", "bit_rate"],
            },
        )?;
        if let Some(d) = f.get("delivery") {
            need(one(d, &json!(["inline", "uri"])), "invalid_google_delivery")?;
        }
        if kind == "video" {
            if let Some(r) = f.get("resolution") {
                need(one(r, &c["resolutions"]), "invalid_google_format")?;
            }
            if let Some(a) = f.get("aspect_ratio") {
                need(one(a, &c["aspects"]), "invalid_google_format")?;
            }
            if let Some(d) = f.get("duration") {
                let a = s(d)?;
                let n = a.strip_suffix('s').and_then(|v| v.parse::<f64>().ok());
                need(
                    crate::matches(r"\d+(\.\d{1,9})?s", d)
                        && n.is_some_and(|n| (3.0..=10.0).contains(&n)),
                    "invalid_google_duration",
                )?;
            }
        } else if kind == "image" {
            if let Some(r) = f.get("image_size") {
                need(one(r, &c["resolutions"]), "invalid_google_format")?;
            }
            if let Some(a) = f.get("aspect_ratio") {
                need(one(a, &c["aspects"]), "invalid_google_format")?;
            }
            if let Some(m) = f.get("mime_type") {
                need(m == "image/jpeg", "invalid_google_mime")?;
            }
        } else {
            if let Some(m) = f.get("mime_type") {
                need(one(m, &c["formats"]), "invalid_google_mime")?;
            }
            if let Some(r) = f.get("sample_rate") {
                need(int(r, 8000, 48000), "invalid_google_sample_rate")?;
            }
            if let Some(r) = f.get("bit_rate") {
                need(
                    int(r, 8000, 512000)
                        && one(&f["mime_type"], &json!(["audio/mp3", "audio/ogg_opus"])),
                    "invalid_google_bit_rate",
                )?;
            }
        }
    }
    let empty = json!({});
    let g = x.get("generation_config").unwrap_or(&empty);
    fields(
        g,
        &[
            "seed",
            "stop_sequences",
            "temperature",
            "top_p",
            "thinking_level",
            "thinking_summaries",
            "max_output_tokens",
            "video_config",
            "speech_config",
            "structured_speech_config",
            "tool_choice",
        ],
    )?;
    if let Some(seed) = g.get("seed") {
        need(int(seed, -2147483648, 2147483647), "invalid_google_seed")?;
    }
    for (key, min, max) in [("temperature", 0.0, 2.0), ("top_p", 0.0, 1.0)] {
        if let Some(v) = g.get(key) {
            need(
                crate::matches(r"-?\d+(\.\d+)?", v)
                    && s(v)?.parse::<f64>().is_ok_and(|n| n >= min && n <= max),
                "invalid_google_sampling",
            )?;
        }
    }
    if let Some(v) = g.get("max_output_tokens") {
        need(
            int(v, 1, n(&c["outputTokens"])?),
            "invalid_google_output_tokens",
        )?;
    }
    if let Some(v) = g.get("thinking_level") {
        need(
            one(v, &json!(["minimal", "low", "medium", "high"])),
            "invalid_google_thinking",
        )?;
    }
    if let Some(v) = g.get("thinking_summaries") {
        need(one(v, &json!(["auto", "none"])), "invalid_google_thinking")?;
    }
    if let Some(v) = g.get("stop_sequences") {
        need(
            v.as_array().is_some_and(|a| {
                a.iter()
                    .all(|x| string(x, n(&c["inputTokens"]).unwrap_or(0) as usize * 8))
            }),
            "invalid_google_stop_sequences",
        )?;
    }
    if let Some(v) = g.get("tool_choice") {
        if v.is_string() {
            need(
                one(v, &json!(["auto", "any", "none", "validated"])),
                "invalid_google_tool_choice",
            )?;
        } else {
            fields(v, &["allowed_tools"])?;
            if let Some(a) = v.get("allowed_tools") {
                fields(a, &["mode", "tools"])?;
                if let Some(mode) = a.get("mode") {
                    need(
                        one(mode, &json!(["auto", "any", "none", "validated"])),
                        "invalid_google_tool_choice",
                    )?;
                }
                if let Some(tools) = a.get("tools") {
                    need(
                        tools
                            .as_array()
                            .is_some_and(|xs| xs.iter().all(|v| v == "google_search")),
                        "invalid_google_tool_choice",
                    )?;
                }
            }
        }
    }
    if let Some(v) = g.get("video_config") {
        fields(v, &["task"])?;
        need(
            c["kind"] == "video" && one(&v["task"], &c["tasks"]),
            "invalid_google_task",
        )?;
    }
    need(
        g.get("speech_config").is_none() || g.get("structured_speech_config").is_none(),
        "invalid_google_speakers",
    )?;
    let mut names = vec![];
    if let Some(v) = g
        .get("speech_config")
        .or_else(|| g.get("structured_speech_config"))
    {
        need(c["speech"] == true, "invalid_google_speakers")?;
        let voices = if v.is_array() {
            v
        } else {
            fields(v, &["speakers", "mode"])?;
            if let Some(mode) = v.get("mode") {
                need(mode == "conversational", "invalid_google_speakers")?;
            }
            &v["speakers"]
        };
        let voices = voices.as_array().ok_or(Error("invalid_google_speakers"))?;
        need(
            !voices.is_empty() && voices.len() <= 2,
            "invalid_google_speakers",
        )?;
        for v in voices {
            fields(v, &["voice", "speaker", "language"])?;
            need(
                string(&v["voice"], n(&c["inputTokens"])? as usize * 8),
                "invalid_google_voice",
            )?;
            if let Some(v) = v.get("speaker") {
                need(string(v, 100), "invalid_google_voice")?;
                names.push(s(v)?.to_string());
            }
            if let Some(v) = v.get("language") {
                need(string(v, 100), "invalid_google_voice")?;
            }
        }
        if voices.len() == 2 {
            need(
                names.len() == 2
                    && names[0] != names[1]
                    && voices
                        .iter()
                        .all(|v| !crate::matches("voice(key)?_.*", &v["voice"])),
                "invalid_google_speakers",
            )?;
        }
    }
    if let Some(turns) = x.get("turns") {
        need(c["speech"] == true, "invalid_google_turns")?;
        let turns = turns.as_array().ok_or(Error("invalid_google_turns"))?;
        need(!turns.is_empty(), "invalid_google_turns")?;
        for t in turns {
            fields(t, &["text", "speaker", "style"])?;
            need(
                string(&t["text"], n(&c["inputTokens"])? as usize * 8),
                "invalid_google_turns",
            )?;
            if let Some(v) = t.get("speaker") {
                need(names.contains(&s(v)?.to_string()), "invalid_google_turns")?;
            }
            if let Some(v) = t.get("style") {
                need(
                    string(v, n(&c["inputTokens"])? as usize * 8),
                    "invalid_google_turns",
                )?;
            }
        }
        need(
            turns
                .iter()
                .map(|t| s(&t["text"]).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                == s(&x["prompt"])?,
            "google_turn_text_mismatch",
        )?;
    }
    if let Some(tools) = x.get("tools") {
        need(c["search"] == true, "invalid_google_tools")?;
        let tools = tools.as_array().ok_or(Error("invalid_google_tools"))?;
        need(tools.len() == 1, "invalid_google_tools")?;
        for t in tools {
            fields(t, &["type", "search_types"])?;
            need(t["type"] == "google_search", "invalid_google_tools")?;
            if let Some(v) = t.get("search_types") {
                need(
                    v.as_array().is_some_and(|a| {
                        !a.is_empty()
                            && a.iter()
                                .all(|v| one(v, &json!(["web_search", "image_search"])))
                    }),
                    "invalid_google_tools",
                )?;
            }
        }
    }
    if let Some(p) = x.get("previous_interaction_id") {
        need(
            crate::matches("v1_[A-Za-z0-9_-]{1,97}", p),
            "invalid_google_previous_interaction",
        )?;
    }
    references(x, c)?;
    Ok(())
}
fn references(x: &Value, c: &Value) -> Result<()> {
    let empty = vec![];
    let refs = x
        .get("references")
        .map(|v| v.as_array().ok_or(Error("invalid_google_references")))
        .transpose()?
        .unwrap_or(&empty);
    let (mut images, mut videos, mut first, mut last) = (0, 0, 0, 0);
    let task = x["generation_config"]["video_config"]["task"]
        .as_str()
        .unwrap_or("");
    for r in refs {
        exact(
            r,
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
            ],
        )?;
        need(
            crate::matches(
                r"https://(api|app)\.everyframe\.studio/(api/)?v1/attachment/att_[A-Za-z0-9_-]+\?t=[A-Za-z0-9_-]{20,100}",
                &r["url"],
            ),
            "invalid_reference_origin",
        )?;
        need(
            crate::matches("[a-f0-9]{64}", &r["sha256"]) && int(&r["bytes"], 1, 2147483648),
            "invalid_reference_manifest",
        )?;
        need(
            ["width", "height", "durationMs", "frameCount"]
                .iter()
                .all(|k| int(&r[*k], 0, 2147483647)),
            "invalid_reference_metadata",
        )?;
        match s(&r["role"])? {
            "image" | "first" | "last" => {
                need(
                    one(
                        &r["mime"],
                        &json!([
                            "image/png",
                            "image/jpeg",
                            "image/webp",
                            "image/heic",
                            "image/heif"
                        ]),
                    ) && n(&r["width"])? > 0
                        && n(&r["height"])? > 0,
                    "invalid_reference_mime",
                )?;
                need(
                    c["kind"] == "video" || r["role"] == "image",
                    "invalid_reference_role",
                )?;
                images += 1;
                if r["role"] == "first" {
                    first += 1;
                }
                if r["role"] == "last" {
                    last += 1;
                }
            }
            "video" => {
                need(
                    n(&c["videoReferences"])? > 0
                        && s(&r["mime"])?.starts_with("video/")
                        && n(&r["durationMs"])? > 0
                        && n(&r["frameCount"])? > 0,
                    "invalid_reference_mime",
                )?;
                videos += 1;
                if c["kind"] == "video" {
                    need(
                        n(&r["durationMs"])?
                            <= if ["edit", "extend"].contains(&task) {
                                10000
                            } else {
                                3000
                            },
                        "google_reference_duration",
                    )?;
                }
            }
            "document" => need(
                c["documentReferences"] == true && r["mime"] == "application/pdf",
                "invalid_reference_mime",
            )?,
            _ => return Err(Error("invalid_reference_role")),
        }
    }
    need(
        images <= n(&c["imageReferences"])?
            && videos <= n(&c["videoReferences"])?
            && first <= 1
            && last <= 1
            && (last == 0 || first == 1),
        "invalid_google_references",
    )?;
    match task {
        "text_to_video" => need(refs.is_empty(), "google_task_input_mismatch")?,
        "image_to_video" => need(
            (1..=2).contains(&images) && videos == 0,
            "google_task_input_mismatch",
        )?,
        "edit" | "extend" => need(
            videos == 1 || x["previous_interaction_id"].is_string(),
            "google_task_input_mismatch",
        )?,
        _ => {}
    };
    Ok(())
}
pub fn spec(v: &Value, m: &Value) -> Result<Value> {
    fields(
        v,
        &[
            "model",
            "prompt",
            "options",
            "references",
            "turns",
            "duration",
            "resolution",
            "aspect",
            "seed",
        ],
    )?;
    let c = &CAPS[s(&v["model"])?];
    let mut x = m["params"].clone();
    x["prompt"] = if let Some(p) = v.get("prompt") {
        p.clone()
    } else {
        json!(
            v["turns"]
                .as_array()
                .ok_or(Error("invalid_google_prompt"))?
                .iter()
                .map(|t| s(&t["text"]))
                .collect::<Result<Vec<_>>>()?
                .join("\n")
        )
    };
    if let Some(o) = v.get("options") {
        fields(
            o,
            &[
                "response_format",
                "generation_config",
                "tools",
                "previous_interaction_id",
                "system_instruction",
            ],
        )?;
        for k in ["response_format", "generation_config"] {
            if let Some(value) = o.get(k) {
                if value.is_array() {
                    x[k] = value.clone();
                } else {
                    let obj = value.as_object().ok_or(Error("invalid_google_fields"))?;
                    if !x[k].is_object() {
                        x[k] = json!({});
                    }
                    x[k].as_object_mut().unwrap().extend(obj.clone());
                }
            }
        }
        if o["generation_config"]
            .get("structured_speech_config")
            .is_some()
        {
            x["generation_config"]
                .as_object_mut()
                .unwrap()
                .remove("speech_config");
        }
        if o["generation_config"].get("speech_config").is_some() {
            x["generation_config"]
                .as_object_mut()
                .unwrap()
                .remove("structured_speech_config");
        }
        for k in ["tools", "previous_interaction_id", "system_instruction"] {
            if let Some(value) = o.get(k) {
                x[k] = value.clone();
            }
        }
    }
    let f = if x["response_format"].is_array() {
        x["response_format"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["type"] == c["kind"])
            .ok_or(Error("invalid_google_format"))?
    } else {
        &mut x["response_format"]
    };
    if let Some(d) = v.get("duration") {
        f["duration"] = json!(format!("{d}s"));
    }
    if let Some(r) = v.get("resolution") {
        f[if c["kind"] == "image" {
            "image_size"
        } else {
            "resolution"
        }] = if c["kind"] == "image" {
            json!(s(r)?.to_uppercase())
        } else {
            r.clone()
        };
    }
    if let Some(a) = v.get("aspect") {
        if a == "auto" {
            f.as_object_mut()
                .ok_or(Error("invalid_google_format"))?
                .remove("aspect_ratio");
        } else {
            f["aspect_ratio"] = a.clone();
        }
    }
    let requested = if let Some(a) = v["options"]["response_format"].as_array() {
        a.iter().find(|f| f["type"] == c["kind"])
    } else {
        v["options"].get("response_format")
    };
    if v["references"].as_array().is_some_and(|r| !r.is_empty())
        && v.get("aspect").is_none()
        && requested.and_then(|f| f.get("aspect_ratio")).is_none()
    {
        f.as_object_mut()
            .ok_or(Error("invalid_google_format"))?
            .remove("aspect_ratio");
    }
    if let Some(seed) = v.get("seed") {
        if !x["generation_config"].is_object() {
            x["generation_config"] = json!({});
        }
        x["generation_config"]["seed"] = seed.clone();
    }
    for k in ["references", "turns"] {
        if let Some(value) = v.get(k)
            && (k != "references" || value.as_array().is_none_or(|a| !a.is_empty()))
        {
            x[k] = value.clone();
        }
    }
    for k in ["temperature", "top_p"] {
        if let Some(value) = x["generation_config"].get(k).cloned() {
            x["generation_config"][k] = json!(if value.is_string() {
                s(&value)?.to_string()
            } else {
                value.to_string()
            });
        }
    }
    let result = json!({"model":v["model"],"input":x});
    check(&result)?;
    Ok(result)
}
