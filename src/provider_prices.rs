//! Read-only provider pricing. No generation endpoint, billing/admin key,
//! caller-supplied URL, historical-average inference or raw upstream messages.
use crate::{
    Error, Result,
    invitation::configured,
    need,
    network::{Http, Request},
    protocol, s,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::LazyLock};
pub static PROFILES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/automatic-offer-profiles.json"))
        .expect("reviewed automatic offer profiles")
});
pub const MAX_PRICE: u64 = 1_000_000_000_000_000;
/// Fixed-point pico-USD, rounded upward; floats never enter the signed wire.
pub fn picousd(value: &Value) -> Result<u64> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    let text = text.trim();
    need(
        !text.is_empty() && text.len() <= 80 && !text.starts_with(['-', '+']),
        "invalid_provider_price",
    )?;
    let parts: Vec<_> = text.split(['e', 'E']).collect();
    need(parts.len() <= 2, "invalid_provider_price")?;
    let exponent = if parts.len() == 2 {
        parts[1]
            .parse::<i32>()
            .map_err(|_| Error("invalid_provider_price"))?
    } else {
        0
    };
    need((-30..=30).contains(&exponent), "invalid_provider_price")?;
    let decimal: Vec<_> = parts[0].split('.').collect();
    need(
        decimal.len() <= 2
            && !decimal[0].is_empty()
            && decimal
                .iter()
                .all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())),
        "invalid_provider_price",
    )?;
    let digits = decimal.concat();
    need(digits.len() <= 36, "invalid_provider_price")?;
    let mantissa = digits
        .parse::<u128>()
        .map_err(|_| Error("invalid_provider_price"))?;
    let shift = 12 + exponent - decimal.get(1).map_or(0, |s| s.len() as i32);
    let amount = if shift >= 0 {
        mantissa
            .checked_mul(
                10_u128
                    .checked_pow(shift as u32)
                    .ok_or(Error("invalid_provider_price"))?,
            )
            .ok_or(Error("invalid_provider_price"))?
    } else {
        let denominator = 10_u128
            .checked_pow((-shift) as u32)
            .ok_or(Error("invalid_provider_price"))?;
        mantissa.div_ceil(denominator)
    };
    need(amount <= MAX_PRICE as u128, "invalid_provider_price")?;
    Ok(amount as u64)
}
pub fn model_request(model: &str) -> Option<(&'static str, String)> {
    if let Some(api) = PROFILES["fal"][model]["apiModel"].as_str() {
        return Some(("fal", api.into()));
    }
    let p = &crate::studio_video::POLICIES[model];
    if p["provider"] == "openrouter" {
        return Some(("openrouter", p["apiModel"].as_str()?.into()));
    }
    None
}
pub fn valid_unit(model: &str, unit: &str) -> bool {
    let u = unit.trim().to_ascii_lowercase();
    match PROFILES["fal"][model]["meter"].as_str() {
        Some("image") => ["image", "images"].contains(&u.as_str()),
        Some("audio") => ["audio", "audios", "audio clip", "audio clips"].contains(&u.as_str()),
        Some("second") => {
            ["second", "seconds", "video second", "video seconds"].contains(&u.as_str())
        }
        Some("character") => [
            "character",
            "characters",
            "1000 characters",
            "1k characters",
        ]
        .contains(&u.as_str()),
        Some("megapixel") => ["megapixel", "megapixels"].contains(&u.as_str()),
        _ => false,
    }
}
fn base(provider: &str, model: &str, key: &str, at: i64) -> Value {
    json!({"model":model,"checkedAt":at,"credentialRef":protocol::sha(format!("everyframe-worker-price-credential-v1\0{provider}\0{key}")),"source":if provider=="fal"{"fal-unit-estimate-v1"}else{"openrouter-video-catalog-v1"}})
}
fn fail(mut v: Value, code: &str, unsupported: bool) -> Value {
    v["status"] = json!(if unsupported {
        "unsupported"
    } else {
        "unavailable"
    });
    v["code"] = json!(code);
    v
}
fn request(url: impl Into<String>, hosts: &[&str]) -> Request {
    let mut r = Request::get(url, hosts);
    r.timeout = 5;
    r.limit = 2_000_000;
    r
}
pub fn collect(
    http: &dyn Http,
    credentials: &Value,
    provider: &str,
    models: &[Value],
    at: i64,
) -> Vec<Value> {
    let key_name = match provider {
        "fal" => "FAL_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => return vec![],
    };
    let key = credentials[key_name].as_str().unwrap_or("");
    let source: Result<Value> = (|| {
        need(models.len() <= 64, "invalid_price_plan")?;
        need(configured(&credentials[key_name]), "provider_key_required")?;
        if provider == "openrouter" {
            http.json(request(
                "https://openrouter.ai/api/v1/videos/models",
                &["openrouter.ai"],
            ))
        } else {
            let mut url = url::Url::parse("https://api.fal.ai/v1/models/pricing").unwrap();
            let mut ids = vec![];
            for m in models {
                let model = s(&m["model"])?;
                let (p, api) =
                    model_request(model).ok_or(Error("automatic_offer_pricing_unsupported"))?;
                need(p == provider && m["apiModel"] == api, "invalid_price_plan")?;
                if !ids.contains(&api) {
                    ids.push(api);
                }
            }
            need(!ids.is_empty() && ids.len() <= 50, "invalid_price_plan")?;
            for id in ids {
                url.query_pairs_mut().append_pair("endpoint_id", &id);
            }
            let mut r = request(url.as_str(), &["api.fal.ai"]);
            r.headers
                .push(("authorization".into(), format!("Key {key}")));
            http.json(r)
        }
    })();
    models
        .iter()
        .take(64)
        .map(|m| {
            let model = m["model"].as_str().unwrap_or("");
            let value = base(provider, model, key, at);
            let result: Result<Value> = (|| {
                let (p, api) =
                    model_request(model).ok_or(Error("automatic_offer_pricing_unsupported"))?;
                need(p == provider && m["apiModel"] == api, "invalid_price_plan")?;
                let data = source.as_ref().map_err(|e| Error(e.0))?;
                if provider == "fal" {
                    need(data["has_more"] == false, "invalid_provider_price")?;
                    let rows = data["prices"]
                        .as_array()
                        .ok_or(Error("invalid_provider_price"))?;
                    let matches: Vec<_> = rows.iter().filter(|v| v["endpoint_id"] == api).collect();
                    need(matches.len() == 1, "invalid_provider_price")?;
                    let unit = s(&matches[0]["unit"])?;
                    need(matches[0]["currency"] == "USD", "invalid_provider_price")?;
                    need(valid_unit(model, unit), "automatic_offer_unit_unsupported")?;
                    let mut endpoints = json!({});
                    endpoints[&api] = json!({"unit_quantity":1});
                    let mut r = request(
                        "https://api.fal.ai/v1/models/pricing/estimate",
                        &["api.fal.ai"],
                    )
                    .json(
                        "POST",
                        &json!({"estimate_type":"unit_price","endpoints":endpoints}),
                    );
                    r.headers
                        .push(("authorization".into(), format!("Key {key}")));
                    let estimate = http.json(r)?;
                    need(
                        estimate["estimate_type"] == "unit_price" && estimate["currency"] == "USD",
                        "invalid_provider_price",
                    )?;
                    Ok(json!({"unit":unit,"unitPricePicousd":picousd(&estimate["total_cost"])?}))
                } else {
                    let rows = data["data"]
                        .as_array()
                        .filter(|a| a.len() <= 1000)
                        .ok_or(Error("invalid_provider_price"))?;
                    let matches: Vec<_> = rows.iter().filter(|v| v["id"] == api).collect();
                    need(matches.len() == 1, "invalid_provider_price")?;
                    let rates = matches[0]["pricing_skus"]
                        .as_object()
                        .filter(|r| !r.is_empty() && r.len() <= 40)
                        .ok_or(Error("invalid_provider_price"))?;
                    let mut safe = HashMap::new();
                    for (k, v) in rates {
                        need(
                            !k.is_empty()
                                && k.len() <= 120
                                && k.bytes().all(|b| {
                                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
                                }),
                            "invalid_provider_price",
                        )?;
                        safe.insert(k.clone(), picousd(v)?);
                    }
                    Ok(json!({"ratesPicousd":safe}))
                }
            })();
            match result {
                Ok(details) => {
                    let mut v = value;
                    v["status"] = json!("ok");
                    v["currency"] = json!("USD");
                    for (k, x) in details.as_object().unwrap() {
                        v[k] = x.clone();
                    }
                    v
                }
                Err(e) => fail(
                    value,
                    if [
                        "provider_key_required",
                        "automatic_offer_unit_unsupported",
                        "automatic_offer_pricing_unsupported",
                        "invalid_price_plan",
                        "invalid_provider_price",
                    ]
                    .contains(&e.0)
                    {
                        e.0
                    } else {
                        "provider_price_unavailable"
                    },
                    [
                        "automatic_offer_unit_unsupported",
                        "automatic_offer_pricing_unsupported",
                        "invalid_price_plan",
                    ]
                    .contains(&e.0),
                ),
            }
        })
        .collect()
}
