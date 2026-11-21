use crate::{
    Error, ErrorContext, Result,
    invitation::{
        DISABLED, PUBLIC_SCRIPT, configured, invited_keys, registry_required, valid_environment,
    },
    matches, need,
    network::{Http, Request},
    protocol::{digest, random},
    s,
};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use rand::rngs::OsRng;
use serde_json::{Value, json};
use x25519_dalek::{PublicKey, StaticSecret};

/// Phala operation history uses UTC, sometimes without the RFC3339 suffix.
pub(crate) fn operation_time(value: &Value) -> Option<i64> {
    let raw = value.as_str().filter(|s| s.len() <= 64)?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|d| d.timestamp_millis())
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|d| d.and_utc().timestamp_millis())
        })
}

pub fn environment(inv: &Value, credentials: &Value, provider: bool) -> Result<Value> {
    need(
        valid_environment(&inv["request"]["compose_file"]["allowed_envs"]),
        "unsafe_compose",
    )?;
    let allowed = invited_keys(inv);
    need(
        !provider || allowed.iter().any(|k| configured(&credentials[k])),
        "provider_key_required",
    )?;
    let re = regex::Regex::new(r"image:\s*ghcr\.io/([a-z0-9_-]+)/").unwrap();
    let matched = re
        .captures(s(&inv["request"]["compose_file"]["docker_compose_file"])?)
        .ok_or(Error("registry_access_required"))?;
    let mut env = json!({"MINER_ID":inv["minerId"]});
    if inv["authMode"] == "hotkey-v1" {
        crate::hotkey::validate(&credentials["WORKER_AUTH"], inv, "worker")?;
        env["MINER_AUTH"] = json!(credentials["WORKER_AUTH"].to_string());
    } else {
        env["MINER_TOKEN"] = credentials["MINER_TOKEN"].clone();
    }
    if registry_required(inv) {
        let user = credentials
            .get("DSTACK_DOCKER_USERNAME")
            .cloned()
            .unwrap_or(json!(&matched[1]));
        need(
            matches("[A-Za-z0-9_-]{1,100}", &user),
            "invalid_registry_username",
        )?;
        need(
            configured(&credentials["DSTACK_DOCKER_PASSWORD"]),
            "registry_access_required",
        )?;
        env["DSTACK_DOCKER_USERNAME"] = user;
        env["DSTACK_DOCKER_PASSWORD"] = credentials["DSTACK_DOCKER_PASSWORD"].clone();
        env["DSTACK_DOCKER_REGISTRY"] = json!("ghcr.io")
    } else {
        need(
            inv["request"]["compose_file"]["pre_launch_script"] == PUBLIC_SCRIPT,
            "public_image_startup_mismatch",
        )?
    };
    for k in allowed {
        env[&k] = if provider && configured(&credentials[&k]) {
            credentials[&k].clone()
        } else {
            json!(DISABLED)
        }
    }
    if inv["request"]["compose_file"]["allowed_envs"]
        .as_array()
        .is_some_and(|a| {
            a.iter()
                .any(|k| k == crate::invitation::ALIBABA_WORKSPACE_KEY)
        })
    {
        let key = crate::invitation::ALIBABA_WORKSPACE_KEY;
        if provider && configured(&credentials["DASHSCOPE_API_KEY"]) {
            need(
                crate::invitation::valid_workspace(&credentials[key]),
                "alibaba_workspace_required",
            )?;
            env[key] = credentials[key].clone();
        } else {
            env[key] = json!("");
        }
    }
    if inv["request"]["compose_file"]["allowed_envs"]
        .as_array()
        .is_some_and(|a| a.iter().any(|k| k == "EVERYFRAME_UPDATER_SEED"))
    {
        crate::updater_vm::image(&inv["request"]["compose_file"])?;
        // Only the isolated, pinned supervisor receives this environment key.
        // An empty value keeps an already enrolled persistent supervisor alive.
        let seed = credentials["UPDATER_SEED"].as_str().unwrap_or("");
        need(
            seed.len() <= crate::updater_vm::MAX_SEED_BYTES,
            "updater_seed_too_large",
        )?;
        env["EVERYFRAME_UPDATER_SEED"] = json!(seed);
    }
    Ok(env)
}
pub fn encrypt(inv: &Value, credentials: &Value, key: &str, provider: bool) -> Result<Value> {
    need(
        matches("[0-9a-fA-F]{64}", &json!(key)),
        "invalid_encryption_key",
    )?;
    let vals = environment(inv, credentials, provider)?;
    let peer: [u8; 32] = hex::decode(key)
        .map_err(|_| Error("invalid_encryption_key"))?
        .try_into()
        .map_err(|_| Error("invalid_encryption_key"))?;
    let secret = StaticSecret::random_from_rng(OsRng);
    let shared = secret.diffie_hellman(&PublicKey::from(peer));
    need(shared.was_contributory(), "invalid_encryption_key")?;
    let iv = random::<12>();
    let env = vals
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| json!({"key":k,"value":v}))
        .collect::<Vec<_>>();
    let ciphertext = Aes256Gcm::new_from_slice(shared.as_bytes())
        .unwrap()
        .encrypt(
            Nonce::from_slice(&iv),
            json!({"env":env}).to_string().as_bytes(),
        )
        .map_err(|_| Error("encryption_failed"))?;
    Ok(
        json!({"encrypted_env":hex::encode([PublicKey::from(&secret).as_bytes().as_slice(),&iv,&ciphertext].concat()),"env_keys":vals.as_object().unwrap().keys().collect::<Vec<_>>()}),
    )
}
pub fn compose_hash(v: &Value) -> Result<String> {
    digest(v)
}
pub struct Cloud<'a> {
    pub http: &'a dyn Http,
    key: String,
}
impl<'a> Cloud<'a> {
    pub fn new(http: &'a dyn Http, creds: &Value) -> Result<Self> {
        need(
            configured(&creds["PHALA_CLOUD_API_KEY"]),
            "phala_key_required",
        )?;
        Ok(Self {
            http,
            key: s(&creds["PHALA_CLOUD_API_KEY"])?.into(),
        })
    }
    pub fn request(
        &self,
        path: &str,
        method: &str,
        body: Option<&Value>,
        old_version: bool,
        empty: bool,
    ) -> Result<Value> {
        need(
            path.starts_with('/') && !path.starts_with("//"),
            "untrusted_destination",
        )?;
        let mut r = Request::get(
            format!("https://cloud-api.phala.com/api/v1{path}"),
            &["cloud-api.phala.com"],
        );
        r.method = method.into();
        r.timeout = 30;
        r.limit = 2000000;
        if let Some(body) = body {
            r = r.json(method, body)
        }
        r.headers.extend([
            ("X-API-Key".into(), self.key.clone()),
            (
                "X-Phala-Version".into(),
                if old_version {
                    "2026-05-22"
                } else {
                    "2026-06-23"
                }
                .into(),
            ),
        ]);
        let b = self
            .http
            .bytes(r)
            .map_err(crate::diagnostics::phala_error)?;
        if empty && b.is_empty() {
            Ok(json!({}))
        } else {
            serde_json::from_slice(&b).map_err(|_| Error("invalid_cloud_response"))
        }
    }
    pub fn get(&self, path: &str) -> Result<Value> {
        self.request(path, "GET", None, false, false)
    }
    pub fn mutate(&self, path: &str, method: &str, body: &Value, empty: bool) -> Result<Value> {
        if path == "/cvms/provision" && method == "POST" {
            let mut wire = body.clone();
            if let Some(compose) = wire.get_mut("compose_file").and_then(Value::as_object_mut)
                && compose
                    .get("gateway_enabled")
                    .is_some_and(Value::is_boolean)
            {
                // Current Phala creation rejects this legacy alias, even when
                // gateway_enabled is explicit. Omit it on the wire only. The
                // signed invitation, compose hash and response checks stay exact.
                compose.remove("tproxy_enabled");
            }
            self.request(path, method, Some(&wire), false, empty)
        } else {
            self.request(path, method, Some(body), false, empty)
        }
    }
    pub fn app_path(id: &str) -> Result<String> {
        need(matches("[0-9a-f]{40}", &json!(id)), "invalid_app_id")?;
        Ok(format!("/cvms/app_{id}"))
    }
    pub fn info(&self, id: &str) -> Result<Value> {
        self.get(&Self::app_path(id)?)
    }
    pub fn encryption_key(&self, id: &str, kms: &str) -> Result<Value> {
        Self::app_path(id)?;
        need(matches("[A-Za-z0-9_-]{1,200}", &json!(kms)), "invalid_kms")?;
        self.request(&format!("/kms/{kms}/pubkey/{id}"), "GET", None, true, false)
    }
    pub(crate) fn deployment_encryption_key(
        &self,
        info: &Value,
        deployment: &Value,
    ) -> Result<String> {
        need(
            info["app_id"] == deployment["appId"],
            "deployment_binding_mismatch",
        )?;
        let pinned = s(&deployment["encryptionKey"])?;
        let bound_key = if info["kms_info"]["chain_id"].is_null() {
            info["kms_info"]
                .get("encrypted_env_pubkey")
                .filter(|key| !key.is_null())
                .or_else(|| {
                    info.get("encrypted_env_pubkey")
                        .filter(|key| !key.is_null())
                })
        } else {
            None
        };
        // Centralized KMS reports the app's key as part of its authenticated
        // CVM record. A saved node slug may no longer identify the app's KMS.
        let key = if let Some(value) = bound_key {
            s(value)?.to_owned()
        } else {
            let app_id = s(&deployment["appId"])?;
            let saved_kms = s(&deployment["kms"])?;
            let response = match self.encryption_key(app_id, saved_kms) {
                Err(error)
                    if saved_kms != "phala"
                        && info["kms_type"] == "phala"
                        && info["kms_info"]["chain_id"].is_null()
                        && error.0 == "phala_http_409"
                        && matches!(&error.1, Some(ErrorContext::Phala(detail))
                            if detail.code.as_deref() == Some("ERR-12-005")) =>
                {
                    self.encryption_key(app_id, "phala")?
                }
                result => result?,
            };
            s(&response["public_key"])?.to_owned()
        };
        let key = key.strip_prefix("0x").unwrap_or(&key);
        need(
            matches("[0-9a-fA-F]{64}", &json!(key)),
            "invalid_encryption_key",
        )?;
        need(
            key.eq_ignore_ascii_case(pinned.strip_prefix("0x").unwrap_or(pinned)),
            "encryption_key_changed",
        )?;
        Ok(key.to_owned())
    }
    pub fn catalog(&self, path: &str) -> Result<Value> {
        let mut items = vec![];
        for page in 1..=100 {
            let result = self.get(&format!("{path}?page={page}&page_size=100"))?;
            let a = result["items"]
                .as_array()
                .ok_or(Error("invalid_cloud_response"))?;
            items.extend(a.iter().cloned());
            if !result["pages"].is_null() {
                let pages = crate::n(&result["pages"])?;
                need((0..=100).contains(&pages), "catalog_too_large")?;
                if page >= pages {
                    return Ok(json!({"items":items}));
                }
            } else if a.len() < 100 {
                return Ok(json!({"items":items}));
            }
        }
        Err(Error("catalog_too_large"))
    }
}
