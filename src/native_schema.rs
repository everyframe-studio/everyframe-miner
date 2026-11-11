//! Small, bounded validator for reviewed provider request schemas. Schemas are
//! embedded release assets: jobs cannot supply schemas, URLs or transport code.
use crate::{Error, Result, need};
use serde_json::{Value, json};

fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
fn node(schema: &Value, v: &Value, depth: usize, budget: &mut usize) -> bool {
    if depth > 32 || *budget == 0 {
        return false;
    }
    *budget -= 1;
    if schema == &Value::Bool(true) {
        return true;
    }
    if !schema.is_object() {
        return false;
    }
    // All refs must have been resolved when the release registry was built.
    if schema.get("$ref").is_some() {
        return false;
    }
    if let Some(branches) = schema["allOf"].as_array()
        && !branches.iter().all(|s| node(s, v, depth + 1, budget))
    {
        return false;
    }
    for (kind, exactly_one) in [("anyOf", false), ("oneOf", true)] {
        if let Some(branches) = schema[kind].as_array() {
            let count = branches
                .iter()
                .filter(|s| node(s, v, depth + 1, budget))
                .count();
            if count == 0 || exactly_one && count != 1 {
                return false;
            }
        }
    }
    if let Some(value) = schema.get("const")
        && value != v
    {
        return false;
    }
    if let Some(values) = schema["enum"].as_array()
        && !values.contains(v)
    {
        return false;
    }
    let kind = schema["type"].as_str();
    if let Some(kinds) = schema["type"].as_array()
        && !kinds.iter().any(|k| {
            let mut s = schema.clone();
            s["type"] = k.clone();
            node(&s, v, depth + 1, budget)
        })
    {
        return false;
    }
    match kind {
        Some("null") if !v.is_null() => return false,
        Some("boolean") if !v.is_boolean() => return false,
        Some("number" | "integer") => {
            let Some(n) = number(v) else {
                return false;
            };
            if kind == Some("integer") && (!v.is_i64() && !v.is_u64()) {
                return false;
            }
            for (key, test) in [
                ("minimum", 0),
                ("maximum", 1),
                ("exclusiveMinimum", 2),
                ("exclusiveMaximum", 3),
            ] {
                if let Some(limit) = number(&schema[key])
                    && match test {
                        0 => n < limit,
                        1 => n > limit,
                        2 => n <= limit,
                        _ => n >= limit,
                    }
                {
                    return false;
                }
            }
            if let Some(step) = number(&schema["multipleOf"])
                && (step <= 0. || ((n / step) - (n / step).round()).abs() > 1e-8)
            {
                return false;
            }
        }
        Some("string") => {
            let Some(s) = v.as_str() else {
                return false;
            };
            let length = if schema["x-lengthUtf16"] == true {
                s.encode_utf16().count()
            } else {
                s.chars().count()
            };
            if schema["minLength"]
                .as_u64()
                .is_some_and(|n| length < n as usize)
                || schema["maxLength"]
                    .as_u64()
                    .is_some_and(|n| length > n as usize)
            {
                return false;
            }
            if let Some(pattern) = schema["pattern"].as_str() {
                let pattern = pattern.replace(r"\/", "/");
                if !regex::Regex::new(&pattern).is_ok_and(|r| r.is_match(s)) {
                    return false;
                }
            }
        }
        Some("array") => {
            let Some(items) = v.as_array() else {
                return false;
            };
            if items.len() > 1000
                || schema["minItems"]
                    .as_u64()
                    .is_some_and(|n| items.len() < n as usize)
                || schema["maxItems"]
                    .as_u64()
                    .is_some_and(|n| items.len() > n as usize)
            {
                return false;
            }
            let prefix = schema["prefixItems"].as_array();
            for (i, value) in items.iter().enumerate() {
                if let Some(rule) = prefix
                    .and_then(|p| p.get(i))
                    .or_else(|| schema.get("items"))
                    && !node(rule, value, depth + 1, budget)
                {
                    return false;
                }
            }
            if schema["uniqueItems"] == true {
                for i in 0..items.len() {
                    if items[i + 1..].contains(&items[i]) {
                        return false;
                    }
                }
            }
        }
        Some("object") => {
            let Some(obj) = v.as_object() else {
                return false;
            };
            if obj.len() > 1000 {
                return false;
            }
            if schema["required"].as_array().is_some_and(|keys| {
                keys.iter()
                    .any(|k| !obj.contains_key(k.as_str().unwrap_or("")))
            }) {
                return false;
            }
            for (key, value) in obj {
                if let Some(prop) = schema["properties"].get(key) {
                    if !node(prop, value, depth + 1, budget) {
                        return false;
                    }
                } else if let Some(extra) =
                    schema.get("additionalProperties").filter(|s| s.is_object())
                {
                    if !node(extra, value, depth + 1, budget) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        None | Some("null" | "boolean") => (),
        _ => return false,
    }
    true
}
pub fn validate(schema: &Value, value: &Value) -> Result<()> {
    need(node(schema, value, 0, &mut 100000), "invalid_native_schema")
}
/// Resolve only explicit reference slots. Ordinary prompt text is never parsed
/// as a URL. All other URL-bearing values must use a hashed reference manifest.
pub fn resolve(
    value: &Value,
    refs: &[String],
    prompt_field: &str,
    path: &str,
    used: &mut Vec<usize>,
) -> Result<Value> {
    match value {
        Value::String(s) if !prompt_field.split('|').any(|p| path_matches(p, path)) => {
            if let Some(index) = s.strip_prefix("@reference:") {
                let index: usize = index
                    .parse()
                    .map_err(|_| Error("invalid_native_reference_slot"))?;
                let url = refs
                    .get(index)
                    .ok_or(Error("invalid_native_reference_slot"))?;
                used.push(index);
                return Ok(json!(url));
            }
            need(
                !s.starts_with("https:")
                    && !s.starts_with("http:")
                    && !s.starts_with("data:")
                    && !s.starts_with("runway:")
                    && !s.starts_with("gs:"),
                "unverified_native_reference",
            )?;
            Ok(value.clone())
        }
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| resolve(v, refs, prompt_field, &format!("{path}/{i}"), used))
                .collect::<Result<_>>()?,
        )),
        Value::Object(obj) => Ok(Value::Object(
            obj.iter()
                .map(|(k, v)| {
                    Ok((
                        k.clone(),
                        resolve(v, refs, prompt_field, &format!("{path}/{k}"), used)?,
                    ))
                })
                .collect::<Result<_>>()?,
        )),
        _ => Ok(value.clone()),
    }
}

fn insert(root: &mut Value, pointer: &str, value: Value) -> Result<()> {
    let (parent, key) = pointer
        .rsplit_once('/')
        .ok_or(Error("invalid_native_schema"))?;
    let parent = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent)
            .ok_or(Error("invalid_native_schema"))?
    };
    if let Some(map) = parent.as_object_mut() {
        need(
            map.get(key).is_none_or(|v| v == &value),
            "native_prompt_mismatch",
        )?;
        map.insert(key.to_string(), value);
    } else if let Some(array) = parent.as_array_mut() {
        let index = key
            .parse::<usize>()
            .map_err(|_| Error("invalid_native_schema"))?;
        need(index < array.len(), "invalid_native_schema")?;
        array[index] = value;
    } else {
        return Err(Error("invalid_native_schema"));
    }
    Ok(())
}
fn materialize(schema: &Value, value: &Value) -> Value {
    if schema["type"] == "number" && value.is_string() {
        return json!(number(value).unwrap());
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(options) = schema[key].as_array()
            && let Some(s) = options.iter().find(|s| node(s, value, 0, &mut 100000))
        {
            return materialize(s, value);
        }
    }
    if let Some(obj) = value.as_object() {
        return Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), materialize(&schema["properties"][k], v)))
                .collect(),
        );
    }
    if let Some(items) = value.as_array() {
        return Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    materialize(schema["prefixItems"].get(i).unwrap_or(&schema["items"]), v)
                })
                .collect(),
        );
    }
    value.clone()
}
pub fn body(spec: &Value, model: &Value, urls: &[String]) -> Result<Value> {
    let mut raw = spec["input"]["parameters"].clone();
    let prompt = model["apiPrompt"]
        .as_str()
        .ok_or(Error("invalid_native_schema"))?;
    if !prompt.is_empty()
        && !(model["adapter"] == "schema-elevenlabs-music"
            && raw["request"]["composition_plan"].is_object())
    {
        insert(&mut raw, prompt, spec["input"]["prompt"].clone())?;
    }
    if let Some(field) = model["apiModelField"].as_str() {
        insert(&mut raw, field, model["apiModel"].clone())?;
    }
    if model["ltx25"] == true {
        crate::ltx25::check(&raw, spec, model)?;
    }
    minimax_policy(&raw, spec, model)?;
    extra_policy(&raw, spec, model)?;
    luma_policy(&raw, spec, model)?;
    runway_policy(&raw, spec, model)?;
    let mut used = Vec::new();
    let text_fields = format!(
        "{prompt}|/negative_prompt|/contents/*/text|/request/inputs/*/text|/request/previous_text|/request/next_text|/request/future_text|/request/lyrics_text|/request/composition_plan/**|/request/music_prompt/**"
    );
    let resolved = resolve(&raw, urls, &text_fields, "", &mut used)?;
    need(
        used.len() == urls.len()
            && (0..urls.len()).all(|i| used.iter().filter(|v| **v == i).count() == 1),
        "unbound_native_reference",
    )?;
    validate(&model["nativeSchema"], &resolved)?;
    let body = materialize(&model["nativeSchema"], &resolved);
    check_references(&body, "", &model["referencePaths"], urls)?;
    Ok(body)
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let p: Vec<_> = pattern.split('/').collect();
    let v: Vec<_> = path.split('/').collect();
    for (i, part) in p.iter().enumerate() {
        if *part == "**" {
            return true;
        }
        let Some(value) = v.get(i) else {
            return false;
        };
        if *part != "*" && part != value {
            return false;
        }
    }
    p.len() == v.len()
}
fn check_references(value: &Value, path: &str, patterns: &Value, urls: &[String]) -> Result<()> {
    if let Some(text) = value.as_str()
        && patterns.as_array().is_some_and(|p| {
            p.iter()
                .filter_map(Value::as_str)
                .any(|p| path_matches(p, path))
        })
    {
        need(
            urls.iter().any(|u| u == text),
            "unverified_native_reference",
        )?;
    }
    if let Some(items) = value.as_array() {
        for (i, v) in items.iter().enumerate() {
            check_references(v, &format!("{path}/{i}"), patterns, urls)?;
        }
    }
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            check_references(v, &format!("{path}/{k}"), patterns, urls)?;
        }
    }
    Ok(())
}

fn minimax_policy(raw: &Value, spec: &Value, model: &Value) -> Result<()> {
    if model["adapter"] != "schema-minimax-video" {
        return Ok(());
    }
    let content = raw["content"]
        .as_array()
        .ok_or(Error("invalid_native_media_fields"))?;
    let refs = spec["input"]["references"].as_array().unwrap();
    let mut first = 0;
    let mut last = 0;
    let mut images = 0;
    let mut videos = 0;
    let mut audios = 0;
    let mut video_ms = 0;
    let mut audio_ms = 0;
    let mut texts = 0;
    for item in content {
        let kind = item["type"]
            .as_str()
            .ok_or(Error("invalid_reference_type"))?;
        if kind == "text" {
            texts += 1;
            need(
                item.as_object().is_some_and(|o| o.len() == 2)
                    && item["text"] == spec["input"]["prompt"],
                "native_prompt_mismatch",
            )?;
            continue;
        }
        need(
            ["image_url", "video_url", "audio_url"].contains(&kind),
            "invalid_reference_type",
        )?;
        let role = item["role"].as_str().unwrap_or(if kind == "image_url" {
            "first_frame"
        } else {
            ""
        });
        need(
            item.as_object().is_some_and(|o| {
                o.keys()
                    .all(|k| ["type", kind, "role"].contains(&k.as_str()))
            }),
            "invalid_native_media_fields",
        )?;
        let index: usize = item[kind]["url"]
            .as_str()
            .and_then(|v| v.strip_prefix("@reference:"))
            .ok_or(Error("unverified_native_reference"))?
            .parse()
            .map_err(|_| Error("invalid_native_reference_slot"))?;
        let r = refs
            .get(index)
            .ok_or(Error("invalid_native_reference_slot"))?;
        let (expected, max_bytes) = match (kind, role) {
            ("image_url", "first_frame") => {
                first += 1;
                ("first", 30 * 1024 * 1024)
            }
            ("image_url", "last_frame") => {
                last += 1;
                ("last", 30 * 1024 * 1024)
            }
            ("image_url", "reference_image") => {
                images += 1;
                ("image", 30 * 1024 * 1024)
            }
            ("video_url", "reference_video") => {
                videos += 1;
                let ms = r["durationMs"]
                    .as_i64()
                    .ok_or(Error("invalid_reference_duration"))?;
                need((2000..=15000).contains(&ms), "invalid_reference_duration")?;
                video_ms += ms;
                ("video", 50 * 1024 * 1024)
            }
            ("audio_url", "reference_audio") => {
                audios += 1;
                let ms = r["durationMs"]
                    .as_i64()
                    .ok_or(Error("invalid_reference_duration"))?;
                need((2000..=15000).contains(&ms), "invalid_reference_duration")?;
                audio_ms += ms;
                ("audio", 15 * 1024 * 1024)
            }
            _ => return Err(Error("invalid_reference_role")),
        };
        need(
            r["role"] == expected && r["bytes"].as_i64().is_some_and(|b| b > 0 && b <= max_bytes),
            "invalid_native_media_references",
        )?;
        if kind != "audio_url" {
            let w = r["width"].as_i64().unwrap_or(0);
            let h = r["height"].as_i64().unwrap_or(0);
            need(
                (256..=5760).contains(&w)
                    && (256..=5760).contains(&h)
                    && w * 10 >= h * 4
                    && w * 2 <= h * 5,
                "invalid_reference_dimensions",
            )?;
        }
    }
    need(
        texts == 1
            && first <= 1
            && last <= 1
            && images <= 9
            && videos <= 3
            && audios <= 3
            && video_ms <= 15000
            && audio_ms <= 15000
            && (!(first + last > 0) || images + videos + audios == 0),
        "invalid_native_media_references",
    )?;
    need(
        !(first + last + images + videos + audios == 0 || model["apiModel"] == "MiniMax-H3-Lite")
            || raw["ratio"] != "adaptive",
        "invalid_native_video_format",
    )?;
    need(
        raw.get("extra").is_none() || model["apiModel"] == "MiniMax-H3-Max",
        "invalid_native_media_fields",
    )?;
    Ok(())
}

fn extra_policy(raw: &Value, spec: &Value, model: &Value) -> Result<()> {
    if model["provider"] == "wavespeed" {
        return crate::wavespeed::reference_policy(raw, spec, model);
    }
    let refs = spec["input"]["references"].as_array().unwrap();
    if model["adapter"] == "schema-minimax-image" {
        need(
            raw.get("width").is_some() == raw.get("height").is_some(),
            "invalid_native_image_size",
        )?;
        need(
            refs.len() <= 1 && refs.iter().all(|r| r["role"] == "image"),
            "invalid_native_media_references",
        )?;
    }
    if model["adapter"] == "schema-minimax-tts" {
        need(refs.is_empty(), "invalid_native_media_references")?;
    }
    if model["provider"] == "elevenlabs" {
        need(refs.is_empty(), "invalid_native_media_references")?;
        if matches!(
            model["adapter"].as_str(),
            Some("schema-elevenlabs-dialogue" | "schema-elevenlabs-stream")
        ) {
            let inputs = raw["request"]["inputs"]
                .as_array()
                .ok_or(Error("invalid_native_media_fields"))?;
            let voices: std::collections::BTreeSet<_> = inputs
                .iter()
                .filter_map(|i| i["voice_id"].as_str())
                .collect();
            let chars: usize = inputs
                .iter()
                .filter_map(|i| i["text"].as_str())
                .map(|v| v.chars().count())
                .sum();
            need(
                !inputs.is_empty()
                    && voices.len()
                        <= (if model["apiModel"] == "eleven_v4_turbo" {
                            1
                        } else {
                            10
                        })
                    && chars <= model["maxPrompt"].as_u64().unwrap() as usize,
                "invalid_native_media_prompt",
            )?;
        }
        if model["adapter"] == "schema-elevenlabs-music"
            && raw["request"]["composition_plan"].is_object()
        {
            need(
                raw["request"]["prompt"].is_null(),
                "invalid_native_media_fields",
            )?;
        }
    }
    if model["provider"] != "kling" {
        return Ok(());
    }
    let prompt = spec["input"]["prompt"].as_str().unwrap();
    let english = prompt.chars().filter(|c| c.is_ascii()).count();
    let nonenglish = prompt.chars().count() - english;
    let is4 = model["apiModel"] == "kling-4.0";
    let max = if is4 {
        26000
    } else if model["apiModel"] == "kling-3.0-omni" {
        3072
    } else {
        2500
    };
    need(
        if is4 {
            english * 11000 + nonenglish * 26000 <= 26000 * 11000
        } else {
            prompt.chars().count() <= max
        },
        "invalid_native_media_prompt",
    )?;
    let mut images = 0;
    let mut videos = 0;
    let mut video_ms = 0;
    let mut first = 0;
    let mut last = 0;
    let mut base = 0;
    let mut voices = 0;
    if let Some(items) = raw["contents"].as_array() {
        need(
            items.iter().filter(|i| i["type"] == "prompt").count() == 1,
            "native_prompt_mismatch",
        )?;
        for item in items {
            let t = item["type"]
                .as_str()
                .ok_or(Error("invalid_reference_type"))?;
            if t == "prompt" {
                need(
                    item["text"] == spec["input"]["prompt"],
                    "native_prompt_mismatch",
                )?;
                continue;
            }
            if t == "voice" {
                voices += 1;
                continue;
            }
            let index = item["url"]
                .as_str()
                .and_then(|s| s.strip_prefix("@reference:"))
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or(Error("unverified_native_reference"))?;
            let r = refs
                .get(index)
                .ok_or(Error("invalid_native_reference_slot"))?;
            let role = match t {
                "first_frame" => {
                    first += 1;
                    "first"
                }
                "last_frame" => {
                    last += 1;
                    "last"
                }
                "middle_frame" => "keyframe",
                "refer_image" => "image",
                "feature_video" => "video",
                "base_video" => {
                    base += 1;
                    "video"
                }
                _ => return Err(Error("invalid_reference_role")),
            };
            need(r["role"] == role, "invalid_reference_role")?;
        }
    }
    for r in refs {
        let w = r["width"]
            .as_u64()
            .ok_or(Error("invalid_reference_dimensions"))?;
        let h = r["height"]
            .as_u64()
            .ok_or(Error("invalid_reference_dimensions"))?;
        need(
            w >= 300 && h >= 300 && w * 5 >= h * 2 && h * 5 >= w * 2,
            "invalid_reference_dimensions",
        )?;
        if r["role"] == "video" {
            videos += 1;
            let ms = r["durationMs"]
                .as_u64()
                .ok_or(Error("invalid_reference_duration"))?;
            video_ms += ms;
            need(
                ms >= 3000
                    && ms
                        <= if is4 {
                            30000
                        } else if model["apiModel"] == "kling-o1" {
                            10000
                        } else {
                            15500
                        },
                "invalid_reference_duration",
            )?;
            need(
                w >= 700 && h >= 700 && w <= 4553 && h <= 4553 && w * h <= 8294400,
                "invalid_reference_dimensions",
            )?;
        } else {
            images += 1;
            need(
                ["image/png", "image/jpeg"].contains(&r["mime"].as_str().unwrap_or("")),
                "invalid_reference_type",
            )?;
        }
        let max = if r["role"] == "video" {
            if is4 { 300 } else { 200 }
        } else if model["kind"] == "image" {
            10
        } else if is4 {
            30
        } else {
            50
        };
        need(
            r["bytes"].as_u64().is_some_and(|n| n <= max * 1024 * 1024),
            "invalid_reference_size",
        )?;
    }
    need(
        first <= 1 && last <= 1 && base <= 1 && voices <= if is4 { 3 } else { 2 },
        "invalid_native_media_references",
    )?;
    if model["kind"] == "image" {
        need(
            images <= if raw.get("image").is_some() { 1 } else { 10 } && videos == 0,
            "invalid_native_media_references",
        )?;
    } else {
        need(
            images
                <= if is4 {
                    10
                } else if videos > 0 {
                    4
                } else {
                    7
                }
                && videos <= if is4 { 5 } else { 1 },
            "invalid_native_media_references",
        )?;
        if is4 {
            need(
                video_ms <= 30000 && images + videos + voices <= 15,
                "invalid_native_media_references",
            )?;
        }
        if videos > 0 && !is4 {
            need(
                raw["settings"]["audio"] != "native",
                "invalid_native_video_format",
            )?;
        }
        if model["apiModel"] == "kling-2.6" && raw["settings"]["audio"] == "native" {
            need(
                raw["settings"]["resolution"] == "1080p",
                "invalid_native_video_format",
            )?;
        }
        if model["apiPath"]
            .as_str()
            .unwrap()
            .starts_with("/image-to-video/")
        {
            need(first == 1, "invalid_native_media_references")?;
        }
    }
    Ok(())
}

fn luma_policy(raw: &Value, spec: &Value, model: &Value) -> Result<()> {
    if model["provider"] != "luma-agents" {
        return Ok(());
    }
    let refs = spec["input"]["references"].as_array().unwrap();
    if model["kind"] == "image" {
        need(
            refs.iter().all(|r| r["role"] == "image") && refs.len() <= 9,
            "invalid_native_media_references",
        )?;
        need(
            (raw["type"] == "image_edit") == raw.get("source").is_some(),
            "invalid_native_media_references",
        )?;
        need(
            raw["image_ref"].as_array().map_or(0, Vec::len)
                <= if raw["type"] == "image_edit" { 8 } else { 9 },
            "invalid_native_media_references",
        )?;
        return Ok(());
    }
    let v = &raw["video"];
    let edit = raw["type"] == "video_edit";
    let reframe = raw["type"] == "video_reframe";
    let duration = if v["duration"] == "10s" { 10 } else { 5 };
    need(
        (edit || reframe) == raw.get("source").is_some(),
        "invalid_native_media_references",
    )?;
    if edit || reframe {
        let index = raw["source"]["url"]
            .as_str()
            .and_then(|x| x.strip_prefix("@reference:"))
            .and_then(|x| x.parse::<usize>().ok())
            .ok_or(Error("unverified_native_reference"))?;
        let r = refs
            .get(index)
            .ok_or(Error("invalid_native_reference_slot"))?;
        need(
            r["role"] == "video"
                && raw["source"]["media_type"] == r["mime"]
                && r["durationMs"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= 18000),
            "invalid_reference_duration",
        )?;
    }
    need(
        edit || v.get("edit").is_none(),
        "invalid_native_video_format",
    )?;
    if reframe {
        need(
            v.get("start_frame").is_none()
                && v.get("end_frame").is_none()
                && v.get("keyframes").is_none()
                && v["loop"] != true
                && v["hdr"] != true,
            "invalid_native_video_format",
        )?;
    }
    if !edit && !reframe {
        if v["hdr"] == true {
            need(
                ["720p", "1080p"].contains(&v["resolution"].as_str().unwrap_or("720p"))
                    && duration == 5
                    && v["loop"] != true,
                "invalid_native_video_format",
            )?;
        }
        if v["loop"] == true {
            need(
                duration == 5 && v.get("end_frame").is_none() && v.get("keyframes").is_none(),
                "invalid_native_video_format",
            )?;
        }
        if duration == 10 {
            need(
                v.get("start_frame").is_none() && v.get("end_frame").is_none(),
                "invalid_native_video_format",
            )?;
        }
    }
    for target in [v, &v["edit"]] {
        if let Some(frames) = target["keyframes"].as_array() {
            let indexes = target["keyframe_indexes"]
                .as_array()
                .ok_or(Error("invalid_native_keyframe"))?;
            need(
                !frames.is_empty()
                    && frames.len() == indexes.len()
                    && v.get("start_frame").is_none()
                    && v.get("end_frame").is_none()
                    && v["loop"] != true,
                "invalid_native_keyframe",
            )?;
            let cap = if edit {
                refs.iter()
                    .find(|r| r["role"] == "video")
                    .and_then(|r| r["frameCount"].as_u64())
                    .ok_or(Error("invalid_native_keyframe"))?
            } else {
                duration * 24
            };
            need(
                indexes.iter().all(|i| i.as_u64().is_some_and(|i| i <= cap)),
                "invalid_native_keyframe",
            )?;
        } else {
            need(
                target.get("keyframe_indexes").is_none(),
                "invalid_native_keyframe",
            )?;
        }
    }
    Ok(())
}

fn runway_policy(raw: &Value, spec: &Value, model: &Value) -> Result<()> {
    if model["provider"] != "runway" {
        return Ok(());
    }
    let refs = spec["input"]["references"].as_array().unwrap();
    let source = if model["apiModel"] == "act_two" {
        raw["reference"]["uri"].as_str()
    } else {
        raw["videoUri"].as_str()
    };
    let mut source_duration = None;
    if let Some(source) = source {
        let index = source
            .strip_prefix("@reference:")
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or(Error("unverified_native_reference"))?;
        let r = refs
            .get(index)
            .ok_or(Error("invalid_native_reference_slot"))?;
        let duration = r["durationMs"]
            .as_u64()
            .ok_or(Error("invalid_reference_duration"))?;
        need(
            r["role"] == "video"
                && duration > 0
                && duration <= 30000
                && (model["apiModel"] != "act_two" || duration >= 3000),
            "invalid_reference_duration",
        )?;
        source_duration = Some(duration as f64 / 1000.);
    }
    if let Some(frames) = raw["keyframes"].as_array() {
        let duration = source_duration.ok_or(Error("invalid_reference_duration"))?;
        let ranges = frames.iter().filter(|f| f.get("range").is_some()).count();
        need(
            ranges == 0 || ranges == frames.len(),
            "invalid_native_keyframe",
        )?;
        for f in frames {
            if let Some(at) = number(&f["seconds"]) {
                need(at >= 0. && at <= duration, "invalid_native_keyframe")?;
                if let Some(range) = f.get("range") {
                    let start =
                        number(&range["start_seconds"]).ok_or(Error("invalid_native_keyframe"))?;
                    let end =
                        number(&range["end_seconds"]).ok_or(Error("invalid_native_keyframe"))?;
                    need(
                        start < end && end <= duration && at >= start && at <= end,
                        "invalid_native_keyframe",
                    )?;
                }
            }
        }
    }
    if let Some(profile) = raw["proresProfile"].as_str() {
        need(
            matches!(raw["outputFormat"].as_str(), Some("prores" | "hdr_prores")),
            "invalid_native_video_format",
        )?;
        if raw["outputFormat"] == "hdr_prores" {
            need(
                ["422", "422 HQ", "4444"].contains(&profile),
                "invalid_native_video_format",
            )?;
        }
    }
    Ok(())
}
