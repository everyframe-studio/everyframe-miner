//! Read-only catalog checks. No generation, arbitrary URLs or provider messages.
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
#[derive(Default)]
pub struct CatalogCache {
    next_check: i64,
    rows: Vec<Value>,
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
    pub fn allows(&self, id: &str) -> bool {
        self.retired_status(id).is_none()
            && self
                .rows
                .iter()
                .find(|r| r["model"] == id)
                .is_none_or(|r| matches!(r["status"].as_str(), Some("available" | "unchecked")))
    }
    pub fn refresh(&mut self, http: &dyn Http, credentials: &Value, now: i64) -> Value {
        if now < self.next_check {
            return json!(self.rows);
        }
        let mut rows = vec![];
        for (provider, key) in PROVIDERS {
            if !configured(&credentials[key]) {
                continue;
            }
            let contracts: Vec<_> = models::MODELS
                .as_object()
                .unwrap()
                .iter()
                .chain(models::STUDIO_MODELS.as_object().unwrap().iter())
                .filter(|(_, m)| m["provider"] == *provider)
                .collect();
            let ids: BTreeSet<_> = contracts
                .iter()
                .filter(|(id, _)| self.retired_status(id).is_none())
                .map(|(id, m)| api_id(id, m).to_string())
                .collect();
            let checked = check_provider(http, provider, s(&credentials[key]).unwrap_or(""), &ids);
            for (id, m) in contracts {
                let status = self.retired_status(id).unwrap_or_else(|| match &checked {
                    Ok(Some(found)) => found
                        .get(api_id(id, m))
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
    r.limit = 8 * 1024 * 1024;
    r.timeout = 10;
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
        "fal" => {
            let mut found = BTreeMap::new();
            let mut requests = 0;
            let mut stopped = false;
            for chunk in ids.iter().cloned().collect::<Vec<_>>().chunks(50) {
                if !stopped && let Err(e) = fal(http, chunk, &mut found, &mut requests) {
                    // A rejected key still marks the whole provider.
                    if matches!(e.0, "http_401" | "http_403") {
                        return Err(e);
                    }
                    // Rate limits, outages and the request cap leave only the
                    // unresolved models temporarily unavailable; confirmed
                    // results stand. Stop asking until the next check.
                    stopped = true;
                }
            }
            for id in ids {
                found.entry(id.clone()).or_insert("temporarily_unavailable");
            }
            Ok(Some(found))
        }
        _ => Ok(None), // No fabricated account entitlement for other providers.
    }
}
fn fal(http: &dyn Http, ids: &[String], found: &mut Catalog, requests: &mut usize) -> Result<()> {
    *requests += 1;
    // A 50-model batch needs up to 13 calls to isolate one removed endpoint;
    // leave room for the next catalog batch as new models are added.
    need(*requests <= 16, "provider_catalog_request_limit")?;
    let mut url = url::Url::parse("https://api.fal.ai/v1/models").unwrap();
    for id in ids {
        url.query_pairs_mut().append_pair("endpoint_id", id);
    }
    // Catalog metadata is public. An inference-only key need not have billing
    // or admin permissions. Never send a key to a different metadata host.
    let result = get(http, url.as_str(), "api.fal.ai", None);
    match result {
        Err(e) if e.0 == "http_404" => {
            if ids.len() == 1 {
                found.insert(ids[0].clone(), "removed");
            } else {
                let mid = ids.len() / 2;
                fal(http, &ids[..mid], found, requests)?;
                fal(http, &ids[mid..], found, requests)?;
            }
        }
        Err(e) => return Err(e),
        Ok(body) => {
            let rows = body["models"]
                .as_array()
                .ok_or(Error("invalid_provider_catalog"))?;
            need(
                body["has_more"] == false && rows.len() <= 50,
                "incomplete_provider_catalog",
            )?;
            for id in ids {
                let row = rows
                    .iter()
                    .find(|r| r["endpoint_id"] == *id)
                    .ok_or(Error("incomplete_provider_catalog"))?;
                found.insert(
                    id.clone(),
                    if row["metadata"]["status"] == "deprecated" {
                        "deprecated"
                    } else {
                        "available"
                    },
                );
            }
        }
    }
    Ok(())
}
