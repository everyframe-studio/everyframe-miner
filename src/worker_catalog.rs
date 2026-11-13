//! Signed, immutable model additions for provider adapters already in this worker.
//! Provider origins, authentication, transport paths and executable flags stay
//! in the measured adapter template. A contract ID is never replaced in place.
use crate::{Error, Result, models, need, protocol, s};
use serde_json::{Value, json};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, RwLock},
};

pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEFINITION_BYTES: usize = 128 * 1024;
const MAX_MODELS: usize = 512;
const INTERVAL_MS: i64 = 300_000;
const CACHE_PATH: &str = "/tmp/everyframe-worker-catalog/catalog.json";
const MUTABLE: &[&str] = &[
    "apiModel",
    "name",
    "vendor",
    "family",
    "description",
    "params",
    "parameterSchema",
    "nativeSchema",
    "nativeCapabilities",
    "duration",
    "resolution",
    "aspect",
    "dimensions",
    "codecs",
    "audio",
    "maxPrompt",
    "schemaUrl",
    "schemaCheckedAt",
    "documentationCheckedAt",
    "validationStatus",
    "nativeVideoDimensions",
    "providerAccess",
    "pricingStatus",
    "apiPath",
    "apiPrompt",
    "apiModelField",
    "referencePaths",
    "referenceRoles",
    "exampleReferences",
    "pricingRule",
    "sourceSha256",
    "catalogPolicy",
    "catalogAdapter",
    "app",
    "feeMultiplier",
    "deadlineMs",
    "seed",
    "mime",
];

#[derive(Default)]
struct Registry {
    // Entries have process lifetime, like compiled contracts. Allocation is
    // append-only and bounded by both total bytes and model count. There is no
    // invalidation of references used by in-flight jobs.
    models: BTreeMap<String, &'static Value>,
    definition_bytes: usize,
    visible: Option<BTreeSet<String>>,
    revision: Option<String>,
    next_refresh: i64,
    session: String,
}
static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::default()));
pub fn contract(id: &str) -> Option<&'static Value> {
    REGISTRY.read().ok()?.models.get(id).copied()
}
pub fn models() -> Vec<(String, &'static Value)> {
    REGISTRY
        .read()
        .map(|r| r.models.iter().map(|(id, m)| (id.clone(), *m)).collect())
        .unwrap_or_default()
}
pub fn visible(id: &str) -> bool {
    REGISTRY
        .read()
        .is_ok_and(|r| r.visible.as_ref().is_none_or(|ids| ids.contains(id)))
}
/// Typed structural hash avoids JavaScript/Rust float serialization differences.
/// The signed envelope carries definitions as inert JSON strings.
fn typed(v: &Value) -> Result<Value> {
    Ok(match v {
        Value::Null => json!(["null"]),
        Value::Bool(b) => json!(["bool", b]),
        Value::Number(n) => {
            let n = n
                .as_f64()
                .filter(|n| n.is_finite() && n.abs() <= 9007199254740991.0)
                .ok_or(Error("invalid_catalog_number"))?;
            json!(["number", hex::encode(n.to_be_bytes())])
        }
        Value::String(s) => json!(["string", s]),
        Value::Array(a) => json!(["array", a.iter().map(typed).collect::<Result<Vec<_>>>()?]),
        Value::Object(o) => {
            let mut keys = o.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            json!([
                "object",
                keys.into_iter()
                    .map(|k| Ok(json!([k, typed(&o[k])?])))
                    .collect::<Result<Vec<_>>>()?
            ])
        }
    })
}
pub fn fingerprint(v: &Value) -> Result<String> {
    protocol::digest(&typed(v)?)
}
pub fn known() -> Result<Value> {
    Ok(json!(
        models::MODELS
            .as_object()
            .unwrap()
            .iter()
            .chain(models::STUDIO_MODELS.as_object().unwrap().iter())
            .map(|(id, m)| Ok(
                json!({"model":id,"hash":crate::catalog_adapters::baseline_hash(id,m)?})
            ))
            .collect::<Result<Vec<_>>>()?
    ))
}
fn same_shape(base: &Value, value: &Value) -> bool {
    match base {
        Value::Object(o) => value.as_object().is_some_and(|v| {
            o.iter()
                .all(|(k, b)| v.get(k).is_some_and(|x| same_shape(b, x)))
        }),
        Value::Array(_) => value.is_array(),
        Value::String(_) => value.is_string(),
        Value::Bool(_) => value.is_boolean(),
        Value::Number(n) => {
            if n.is_u64() {
                value.as_u64().is_some()
            } else {
                value.is_number()
            }
        }
        Value::Null => true,
    }
}
// Model-specific paths may change only inside a compiled generation namespace.
// Fixed task paths (e.g. Runway /v1/text_to_video) stay fixed.
fn compatible_path(template: &Value, model: &Value) -> bool {
    if model["apiPath"] == template["apiPath"] {
        return true;
    }
    let (Some(path), Some(old), Some(new), Some(next)) = (
        template["apiPath"].as_str(),
        template["apiModel"].as_str(),
        model["apiModel"].as_str(),
        model["apiPath"].as_str(),
    ) else {
        return false;
    };
    path.strip_suffix(old)
        .filter(|prefix| !prefix.is_empty() && prefix.ends_with('/'))
        .is_some_and(|prefix| next == format!("{prefix}{new}"))
}
pub fn compatible(template: &Value, model: &Value) -> bool {
    let (Some(t), Some(m)) = (template.as_object(), model.as_object()) else {
        return false;
    };
    t.keys()
        .chain(m.keys())
        .all(|k| MUTABLE.contains(&k.as_str()) || t.get(k) == m.get(k))
        && model["nativeMedia"] == template["nativeMedia"]
        && (model["nativeMedia"] == true || model["catalogAdapter"].is_string())
        && model["catalogAdapter"] == template["catalogAdapter"]
        && !(template["catalogAdapter"] == "studio-media"
            && template["catalogPolicy"]["h3Image"] == true
            && model["apiModel"] != template["apiModel"])
        && crate::catalog_adapters::policy_identity(template, model)
        && crate::catalog_adapters::policy_compatible(
            &template["catalogPolicy"],
            &model["catalogPolicy"],
        )
        && model["provider"] == template["provider"]
        && model["kind"] == template["kind"]
        && model["adapter"] == template["adapter"]
        && compatible_path(template, model)
        && (!template["nativeSchema"].is_object() || model["nativeSchema"].is_object())
        && model["params"].is_object()
        && (template["nativeSchema"].is_object()
            || same_shape(&template["params"], &model["params"]))
        && same_shape(
            &template["nativeCapabilities"],
            &model["nativeCapabilities"],
        )
        && model["deadlineMs"]
            .as_i64()
            .is_some_and(|n| n > 0 && n <= models::max_job_ms())
        && model["maxPrompt"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 8_388_608)
}
pub struct Prepared {
    additions: Vec<(String, Value)>,
    visible: BTreeSet<String>,
    revision: String,
}
pub fn prepare(
    envelope: &Value,
    key: &str,
    chain: &Value,
    miner: &str,
    binding: Option<(&str, &str, i64)>,
) -> Result<Prepared> {
    need(
        envelope.to_string().len() <= MAX_CATALOG_BYTES,
        "catalog_too_large",
    )?;
    let v = protocol::verified(envelope, key)?;
    protocol::exact(
        &v,
        &[
            "protocol",
            "action",
            "minerId",
            "chain",
            "sessionId",
            "nonce",
            "at",
            "revision",
            "models",
            "entries",
            "blocked",
        ],
    )?;
    need(
        v["protocol"] == 1
            && v["action"] == "worker-catalog"
            && v["minerId"] == miner
            && v["chain"] == *chain,
        "catalog_scope_mismatch",
    )?;
    if let Some((session, nonce, now)) = binding {
        need(
            v["sessionId"] == session
                && v["nonce"] == nonce
                && v["at"].as_i64().is_some_and(|at| now.abs_diff(at) <= 30000),
            "catalog_response_replayed",
        )?;
    }
    need(
        crate::matches("[a-f0-9]{64}", &v["revision"]),
        "invalid_catalog_revision",
    )?;
    let ids = v["models"]
        .as_array()
        .ok_or(Error("invalid_catalog_models"))?;
    need(ids.len() <= MAX_MODELS, "invalid_catalog_models")?;
    let mut visible = BTreeSet::new();
    for id in ids {
        need(
            crate::matches("[A-Za-z0-9_./:-]{1,150}", id) && visible.insert(s(id)?.to_owned()),
            "invalid_catalog_models",
        )?;
    }
    let entries = v["entries"]
        .as_array()
        .ok_or(Error("invalid_catalog_entries"))?;
    need(entries.len() <= MAX_MODELS, "invalid_catalog_entries")?;
    let mut seen = BTreeSet::new();
    let mut additions = vec![];
    let mut total = 0;
    for entry in entries {
        protocol::exact(entry, &["model", "template", "definition", "example"])?;
        let id = s(&entry["model"])?;
        need(
            crate::matches("[A-Za-z0-9_./:-]{1,150}", &entry["model"])
                && seen.insert(id.to_owned()),
            "invalid_catalog_entries",
        )?;
        let raw = s(&entry["definition"])?;
        total += raw.len();
        need(
            raw.len() <= MAX_DEFINITION_BYTES && total <= MAX_CATALOG_BYTES,
            "catalog_too_large",
        )?;
        let m: Value =
            serde_json::from_str(raw).map_err(|_| Error("invalid_catalog_definition"))?;
        if let Some(old) = contract(id) {
            need(
                fingerprint(old)? == fingerprint(&m)?,
                "catalog_contract_changed",
            )?;
            continue;
        }
        if models::info(id).is_ok() {
            let compiled = crate::catalog_adapters::compiled(id)?;
            need(
                crate::catalog_adapters::definition_hash(&compiled)?
                    == crate::catalog_adapters::definition_hash(&m)?,
                "catalog_contract_changed",
            )?;
            continue;
        }
        let template = crate::catalog_adapters::compiled(s(&entry["template"])?)?;
        need(compatible(&template, &m), "catalog_adapter_update_required")?;
        let provider = s(&m["provider"])?;
        need(
            [
                format!("direct/{provider}/"),
                format!("studio/native/{provider}/"),
                format!("studio/{provider}/"),
                format!("studio/media/{provider}/"),
                format!("studio/tool/{provider}/"),
            ]
            .iter()
            .any(|prefix| id.starts_with(prefix)),
            "catalog_provider_mismatch",
        )?;
        need(
            crate::matches("[A-Za-z0-9_./:-]{1,200}", &m["apiModel"])
                && !s(&m["apiModel"])?.contains("//")
                && !s(&m["apiModel"])?.split('/').any(|p| p == ".." || p == "."),
            "invalid_catalog_api_model",
        )?;
        let example: Value = serde_json::from_str(s(&entry["example"])?)
            .map_err(|_| Error("invalid_catalog_example"))?;
        need(example["model"] == id, "invalid_catalog_example")?;
        crate::catalog_adapters::check_example(&example, &m)?;
        additions.push((id.to_owned(), m));
    }
    need(
        visible.iter().all(|id| {
            models::MODELS.get(id).is_some()
                || models::STUDIO_MODELS.get(id).is_some()
                || seen.contains(id)
        }),
        "catalog_definition_missing",
    )?;
    // Descriptors are retained for old jobs even when a model stops being
    // offered. A catalog publisher must not erase contract history.
    let r = REGISTRY.read().map_err(|_| Error("catalog_lock_failed"))?;
    need(
        r.models.keys().all(|id| seen.contains(id)),
        "catalog_history_missing",
    )?;
    need(
        models::MODELS.as_object().unwrap().len()
            + models::STUDIO_MODELS.as_object().unwrap().len()
            + r.models.len()
            + additions.len()
            <= MAX_MODELS,
        "catalog_model_limit",
    )?;
    let bytes = additions
        .iter()
        .map(|(_, m)| m.to_string().len())
        .sum::<usize>();
    need(
        r.definition_bytes + bytes <= MAX_CATALOG_BYTES,
        "catalog_too_large",
    )?;
    Ok(Prepared {
        additions,
        visible,
        revision: s(&v["revision"])?.into(),
    })
}
pub fn install(prepared: Prepared) -> Result<usize> {
    install_cached(prepared, None)
}
fn install_cached(prepared: Prepared, cache: Option<(&Path, &Value)>) -> Result<usize> {
    let mut r = REGISTRY.write().map_err(|_| Error("catalog_lock_failed"))?;
    // Recheck under the write lock; no publication occurs until all conflicts
    // and allocation limits pass. Refreshes run only between worker jobs.
    let fresh = prepared
        .additions
        .iter()
        .filter(|(id, _)| !r.models.contains_key(id))
        .count();
    need(r.models.len() + fresh <= MAX_MODELS, "catalog_model_limit")?;
    for (id, m) in &prepared.additions {
        if let Some(old) = r.models.get(id) {
            need(
                fingerprint(old)? == fingerprint(m)?,
                "catalog_contract_changed",
            )?;
        }
    }
    let bytes = prepared
        .additions
        .iter()
        .filter(|(id, _)| !r.models.contains_key(id))
        .map(|(_, m)| m.to_string().len())
        .sum::<usize>();
    need(
        models::MODELS.as_object().unwrap().len()
            + models::STUDIO_MODELS.as_object().unwrap().len()
            + r.models.len()
            + fresh
            <= MAX_MODELS,
        "catalog_model_limit",
    )?;
    need(
        r.definition_bytes + bytes <= MAX_CATALOG_BYTES,
        "catalog_too_large",
    )?;
    if let Some((path, envelope)) = cache {
        save_cache(path, envelope)?;
    }
    let mut installed = 0;
    for (id, m) in prepared.additions {
        if !r.models.contains_key(&id) {
            r.definition_bytes += m.to_string().len();
            r.models.insert(id, Box::leak(Box::new(m)));
            installed += 1;
        }
    }
    r.visible = Some(prepared.visible);
    r.revision = Some(prepared.revision);
    Ok(installed)
}
fn safe_directory(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or(Error("invalid_catalog_cache"))?;
    match fs::symlink_metadata(parent) {
        Ok(m) => need(
            m.is_dir()
                && !m.file_type().is_symlink()
                && m.uid() == effective_uid()
                && m.mode() & 0o077 == 0,
            "unsafe_catalog_cache",
        )?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(parent)
                .map_err(|_| Error("catalog_cache_failed"))?;
        }
        _ => return Err(Error("catalog_cache_failed")),
    }
    Ok(())
}
fn effective_uid() -> u32 {
    // POSIX identity query; the registry never dereferences raw pointers.
    unsafe { libc::geteuid() }
}
pub fn save_cache(path: &Path, envelope: &Value) -> Result<()> {
    safe_directory(path)?;
    if let Ok(m) = fs::symlink_metadata(path) {
        need(
            m.is_file() && !m.file_type().is_symlink() && m.uid() == effective_uid(),
            "unsafe_catalog_cache",
        )?;
    }
    let bytes = envelope.to_string().into_bytes();
    need(bytes.len() <= MAX_CATALOG_BYTES, "catalog_too_large")?;
    let tmp = path.with_extension(format!("{}.tmp", protocol::id()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| Error("catalog_cache_failed"))?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| Error("catalog_cache_failed"))?;
        fs::rename(&tmp, path).map_err(|_| Error("catalog_cache_failed"))?;
        fs::File::open(path.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(|_| Error("catalog_cache_failed"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
pub fn restore(path: &Path, key: &str, chain: &Value, miner: &str) -> Result<usize> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err(Error("catalog_cache_failed")),
        Ok(_) => {}
    }
    safe_directory(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| Error("catalog_cache_failed"))?;
    need(
        meta.is_file()
            && !meta.file_type().is_symlink()
            && meta.uid() == effective_uid()
            && meta.mode() & 0o077 == 0
            && meta.len() <= MAX_CATALOG_BYTES as u64,
        "unsafe_catalog_cache",
    )?;
    let mut bytes = vec![];
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .and_then(|f| f.take(MAX_CATALOG_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| Error("catalog_cache_failed"))?;
    let envelope: Value =
        serde_json::from_slice(&bytes).map_err(|_| Error("invalid_catalog_cache"))?;
    install(prepare(&envelope, key, chain, miner, None)?)
}
pub fn restore_default(key: &str, chain: &Value, miner: &str) -> Result<usize> {
    restore(&PathBuf::from(CACHE_PATH), key, chain, miner)
}
pub fn refresh<R: crate::worker::Runtime, P: crate::providers::Provider>(
    worker: &crate::worker::Worker<R, P>,
) -> Result<bool> {
    refresh_to(worker, Path::new(CACHE_PATH))
}
/// Alternate cache location for embedded runtimes and offline transport tests.
pub fn refresh_to<R: crate::worker::Runtime, P: crate::providers::Provider>(
    worker: &crate::worker::Worker<R, P>,
    cache: &Path,
) -> Result<bool> {
    let now = worker.runtime.now();
    let session = s(&worker.session["sessionId"])?;
    {
        let mut r = REGISTRY.write().map_err(|_| Error("catalog_lock_failed"))?;
        if r.session == session && now < r.next_refresh {
            return Ok(false);
        }
        r.session = session.into();
        r.next_refresh = now + 60000;
    }
    let (result, nonce) = worker.call("catalog_sync", json!({"known":known()?}))?;
    if result["busy"] == true && result["catalog"].is_null() {
        return Ok(false);
    }
    let envelope = &result["catalog"];
    let prepared = prepare(
        envelope,
        &worker.coordinator_key,
        &worker.chain,
        &worker.miner_id,
        Some((session, &nonce, now)),
    )?;
    // A cache failure must not disable working providers or publish a partially
    // persisted catalog. The previous signed cache remains intact.
    let changed = REGISTRY
        .read()
        .map_err(|_| Error("catalog_lock_failed"))?
        .visible
        .as_ref()
        != Some(&prepared.visible);
    let installed = install_cached(prepared, Some((cache, envelope)))?;
    if let Ok(mut r) = REGISTRY.write() {
        r.next_refresh = now + INTERVAL_MS;
    }
    if installed > 0 {
        println!(
            "{}",
            json!({"event":"worker_catalog_updated","modelsAdded":installed})
        );
    }
    Ok(changed || installed > 0)
}
