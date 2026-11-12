//! Read-only provider checks. No generation, arbitrary URLs or provider messages.
//! The signed registry remains authoritative; discovery can only narrow it.
use crate::{
    Error, Result,
    invitation::{PROVIDERS, configured},
    models, need,
    network::{Http, Request},
    s,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};
static RETIRED: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/provider-model-status.json")).unwrap()
});
pub fn api_id<'a>(id: &'a str, model: &'a Value) -> &'a str {
    model["apiModel"].as_str().unwrap_or(id)
}
pub fn retired(id: &str) -> Option<&'static str> {
    let m = models::info(id).ok()?;
    RETIRED[m["provider"].as_str()?][api_id(id, m)].as_str()
}
pub fn label(status: &str) -> &'static str {
    match status {
        "removed" => "Removed by provider",
        "deprecated" => "Deprecated by provider",
        "access_denied" => "Provider access unavailable for this key",
        "temporarily_unavailable" => "Availability check temporarily unavailable",
        "available" => "Listed by provider",
        _ => "Account access not checked",
    }
}
/// Most provider entries a coordinator retirement list may name.
const MAX_SERVER_RETIREMENTS: usize = 512;
const FAL_KEY_CHECK_INTERVAL_MS: i64 = 15 * 60 * 1000;
struct FalKeyCheck {
    fingerprint: String,
    checked_at: i64,
    rejected: bool,
}
#[derive(Default)]
pub struct CatalogCache {
    next_check: i64,
    rows: Vec<Value>,
    fal_key: Option<FalKeyCheck>,
    // Provider API ids the coordinator reports as retired. Narrowing only: an
    // entry can stop this worker offering a model, never enable or reprice one.
    server_retired: BTreeMap<String, BTreeSet<String>>,
}
impl CatalogCache {
    /// Adopt the coordinator's signed retirement list from a claim reply.
    /// Invalid lists are ignored whole; a change forces the next catalog check.
    pub fn set_server_retirements(&mut self, v: &Value) -> bool {
        let Some(map) = v.as_object() else {
            return false;
        };
        let mut next: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut total = 0;
        for (provider, ids) in map {
            let Some(ids) = ids.as_array() else {
                return false;
            };
            if !PROVIDERS.iter().any(|(p, _)| *p == provider.as_str()) {
                continue; // Unknown providers cannot affect this worker.
            }
            for id in ids {
                let Some(id) = id.as_str() else {
                    return false;
                };
                total += 1;
                if total > MAX_SERVER_RETIREMENTS || id.is_empty() || id.len() > 200 {
                    return false;
                }
                next.entry(provider.clone())
                    .or_default()
                    .insert(id.to_string());
            }
        }
        if next == self.server_retired {
            return false;
        }
        self.server_retired = next;
        self.next_check = 0;
        true
    }
    fn retired_status(&self, id: &str) -> Option<&'static str> {
        retired(id).or_else(|| {
            let m = models::info(id).ok()?;
            self.server_retired
                .get(m["provider"].as_str()?)?
                .contains(api_id(id, m))
                .then_some("removed")
        })
    }
    pub fn invalidate(&mut self) {
        self.next_check = 0;
    }
    pub fn allows(&self, id: &str) -> bool {
        self.retired_status(id).is_none()
            && self
                .rows
                .iter()
                .find(|r| r["model"] == id)
                .is_none_or(|r| matches!(r["status"].as_str(), Some("available" | "unchecked")))
    }
    fn check_fal_key(&mut self, http: &dyn Http, key: &str, now: i64) -> Result<Option<Catalog>> {
        let fingerprint = crate::protocol::sha(key.as_bytes());
        let previous = self
            .fal_key
            .as_ref()
            .filter(|check| check.fingerprint == fingerprint);
        let rejected = if let Some(check) = previous.filter(|check| {
            now >= check.checked_at
                && now < check.checked_at.saturating_add(FAL_KEY_CHECK_INTERVAL_MS)
        }) {
            check.rejected
        } else {
            // The public model list says nothing about this key. Use the existing
            // authenticated, non-generating key check once per key/interval. An
            // outage is not evidence of denied model access. A known rejection
            // is cleared only by a successful check or a changed credential.
            let rejected = match crate::key_check::check_with_timeout(http, "fal", key, 10).0 {
                crate::key_check::Verdict::Valid => false,
                crate::key_check::Verdict::Rejected => true,
                crate::key_check::Verdict::Unverified => {
                    previous.is_some_and(|check| check.rejected)
                }
            };
            self.fal_key = Some(FalKeyCheck {
                fingerprint,
                checked_at: now,
                rejected,
            });
            rejected
        };
        if rejected {
            Err(Error("http_401"))
        } else {
            Ok(None)
        }
    }
    pub fn refresh(&mut self, http: &dyn Http, credentials: &Value, now: i64) -> Value {
        let fal_fingerprint = configured(&credentials["FAL_KEY"]).then(|| {
            crate::protocol::sha(credentials["FAL_KEY"].as_str().unwrap_or("").as_bytes())
        });
        let fal_changed =
            self.fal_key.as_ref().map(|check| &check.fingerprint) != fal_fingerprint.as_ref();
        if now < self.next_check && !fal_changed {
            return json!(self.rows);
        }
        if fal_fingerprint.is_none() {
            self.fal_key = None;
        }
        let mut rows = vec![];
        for (provider, key) in PROVIDERS {
            if !configured(&credentials[key]) {
                continue;
            }
            let contracts: Vec<_> = models::all()
                .into_iter()
                .filter(|(id, m)| crate::worker_catalog::visible(id) && m["provider"] == *provider)
                .collect();
            let ids: BTreeSet<_> = contracts
                .iter()
                .filter(|(id, _)| self.retired_status(id).is_none())
                .map(|(id, m)| api_id(id, m).to_string())
                .collect();
            let checked = if provider == "fal" && !ids.is_empty() {
                self.check_fal_key(http, s(&credentials[key]).unwrap_or(""), now)
            } else {
                check_provider(http, provider, s(&credentials[key]).unwrap_or(""), &ids)
            };
            for (id, m) in contracts {
                let status = self.retired_status(&id).unwrap_or_else(|| match &checked {
                    Ok(Some(found)) => found
                        .get(api_id(&id, m))
                        .copied()
                        .unwrap_or("temporarily_unavailable"),
                    Ok(None) => "unchecked",
                    Err(e) if matches!(e.0, "http_401" | "http_403") => "access_denied",
                    Err(_) => "temporarily_unavailable",
                });
                rows.push(json!({"model":id,"status":status,"checkedAt":now}));
            }
        }
        self.next_check = now
            + if rows
                .iter()
                .any(|r| r["status"] == "temporarily_unavailable")
            {
                60000
            } else {
                900000
            };
        self.rows = rows;
        json!(self.rows)
    }
}
type Catalog = BTreeMap<String, &'static str>;
fn get(http: &dyn Http, url: &str, host: &str, auth: Option<(&str, String)>) -> Result<Value> {
    let mut r = Request::get(url, &[host]);
    r.limit = if host == "api.wavespeed.ai" { 32 } else { 8 } * 1024 * 1024;
    r.timeout = if host == "api.wavespeed.ai" { 30 } else { 10 };
    if let Some((name, value)) = auth {
        r.headers.push((name.into(), value));
    }
    http.json(r)
}
fn list_ids(body: &Value) -> Result<BTreeSet<String>> {
    let rows = body["data"]
        .as_array()
        .ok_or(Error("invalid_provider_catalog"))?;
    need(
        rows.len() <= 10000
            && body["links"]["next"].is_null()
            && body["total_count"]
                .as_u64()
                .is_none_or(|n| n == rows.len() as u64),
        "incomplete_provider_catalog",
    )?;
    rows.iter()
        .map(|r| s(&r["id"]).map(str::to_string))
        .collect()
}
fn check_provider(
    http: &dyn Http,
    p: &str,
    key: &str,
    ids: &BTreeSet<String>,
) -> Result<Option<Catalog>> {
    if ids.is_empty() {
        return Ok(Some(BTreeMap::new()));
    }
    match p {
        "wavespeed" => {
            let body = get(
                http,
                "https://api.wavespeed.ai/api/v3/models",
                "api.wavespeed.ai",
                Some(("authorization", format!("Bearer {key}"))),
            )?;
            need(body["code"] == 200, "invalid_provider_catalog")?;
            let rows = body["data"]
                .as_array()
                .ok_or(Error("invalid_provider_catalog"))?;
            need(
                !rows.is_empty()
                    && rows.len() <= 10000
                    && body["has_more"] != true
                    && body["next"].is_null(),
                "incomplete_provider_catalog",
            )?;
            let found = rows
                .iter()
                .map(|r| s(&r["model_id"]).map(str::to_owned))
                .collect::<Result<BTreeSet<_>>>()?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if found.contains(id) {
                                "available"
                            } else {
                                "access_denied"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "elevenlabs" => {
            let data = get(
                http,
                "https://api.elevenlabs.io/v1/models",
                "api.elevenlabs.io",
                Some(("xi-api-key", key.into())),
            )?;
            let rows = data.as_array().ok_or(Error("invalid_provider_catalog"))?;
            need(
                !rows.is_empty() && rows.len() <= 10000,
                "incomplete_provider_catalog",
            )?;
            let found = rows
                .iter()
                .map(|r| s(&r["model_id"]).map(str::to_owned))
                .collect::<Result<BTreeSet<_>>>()?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if id.starts_with("music_") || id.starts_with("eleven_text_to_sound_") {
                                "unchecked"
                            } else if found.contains(id) {
                                "available"
                            } else {
                                "access_denied"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "openai" | "xai" => {
            let host = if p == "openai" {
                "api.openai.com"
            } else {
                "api.x.ai"
            };
            let data = get(
                http,
                &format!("https://{host}/v1/models"),
                host,
                Some(("authorization", format!("Bearer {key}"))),
            )?;
            let found = list_ids(&data)?;
            need(!found.is_empty(), "incomplete_provider_catalog")?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if p == "xai" && id == "tts" {
                                "unchecked"
                            } else if found.contains(id) {
                                "available"
                            } else {
                                "access_denied"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "google" => {
            let mut found = BTreeSet::new();
            let mut page: Option<String> = None;
            let mut complete = false;
            for _ in 0..10 {
                let mut url = url::Url::parse(
                    "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000",
                )
                .unwrap();
                if let Some(token) = page.as_ref() {
                    url.query_pairs_mut().append_pair("pageToken", token);
                }
                let data = get(
                    http,
                    url.as_str(),
                    "generativelanguage.googleapis.com",
                    Some(("x-goog-api-key", key.into())),
                )?;
                let rows = data["models"]
                    .as_array()
                    .ok_or(Error("incomplete_provider_catalog"))?;
                for row in rows {
                    let id = s(&row["name"])?
                        .strip_prefix("models/")
                        .ok_or(Error("invalid_provider_catalog"))?;
                    found.insert(id.to_string());
                }
                match data.get("nextPageToken") {
                    None => {
                        complete = true;
                        break;
                    }
                    Some(Value::String(token)) if token.is_empty() => {
                        complete = true;
                        break;
                    }
                    Some(Value::String(token))
                        if token.len() <= 2000 && page.as_ref() != Some(token) =>
                    {
                        page = Some(token.clone())
                    }
                    _ => return Err(Error("incomplete_provider_catalog")),
                }
            }
            need(complete, "incomplete_provider_catalog")?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if found.contains(id) {
                                "available"
                            } else {
                                "access_denied"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "pika" => {
            let data = get(
                http,
                "https://api.dev.pika.art/catalog/apis",
                "api.dev.pika.art",
                None,
            )?;
            let entries = data["apis"]
                .as_array()
                .ok_or(Error("incomplete_provider_catalog"))?;
            need(!entries.is_empty(), "incomplete_provider_catalog")?;
            let available: BTreeSet<_> = entries
                .iter()
                .filter_map(|r| {
                    let id = r["api_id"].as_str()?;
                    (r["call"]["method"] == "POST"
                        && r["call"]["path"] == format!("/v1/media/{id}"))
                    .then_some(id.to_string())
                })
                .collect();
            // Public catalog presence is availability, not account entitlement.
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if available.contains(id) {
                                "available"
                            } else {
                                "removed"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "openrouter" => {
            let public = list_ids(&get(
                http,
                "https://openrouter.ai/api/v1/models?output_modalities=all",
                "openrouter.ai",
                None,
            )?)?;
            need(!public.is_empty(), "incomplete_provider_catalog")?;
            let user = list_ids(&get(
                http,
                "https://openrouter.ai/api/v1/models/user?output_modalities=all",
                "openrouter.ai",
                Some(("authorization", format!("Bearer {key}"))),
            )?)?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if !public.contains(id) {
                                "removed"
                            } else if !user.contains(id) {
                                "access_denied"
                            } else {
                                "available"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        "vivix" => {
            let body = get(
                http,
                "https://api.vivix.ai/v1/models",
                "api.vivix.ai",
                Some(("authorization", format!("Bearer {key}"))),
            )?;
            need(body["code"] == 0, "invalid_provider_catalog")?;
            let rows = body["data"]
                .as_array()
                .or_else(|| body["data"]["models"].as_array())
                .ok_or(Error("invalid_provider_catalog"))?;
            need(rows.len() <= 1000, "invalid_provider_catalog")?;
            let listed: BTreeSet<_> = rows
                .iter()
                .map(|r| {
                    r.as_str()
                        .or_else(|| r["id"].as_str())
                        .or_else(|| r["name"].as_str())
                        .or_else(|| r["model"].as_str())
                        .ok_or(Error("invalid_provider_catalog"))
                })
                .collect::<Result<_>>()?;
            Ok(Some(
                ids.iter()
                    .map(|id| {
                        (
                            id.clone(),
                            if listed.contains(id.as_str()) {
                                "available"
                            } else {
                                "access_denied"
                            },
                        )
                    })
                    .collect(),
            ))
        }
        _ => Ok(None), // No fabricated account entitlement for other providers.
    }
}
