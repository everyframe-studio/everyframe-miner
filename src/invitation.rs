use crate::{
    Error, Result, matches, n, need,
    protocol::{exact, verified},
    s,
};
use serde_json::{Value, json};
pub const DISABLED: &str = "DISABLED_PENDING_OPERATOR_ATTESTATION";
pub const PUBLIC_SCRIPT: &str =
    "#!/bin/sh\nset -eu\n# Public image: anonymous pull; no registry credentials.\n";
pub const PROVIDERS: [(&str, &str); 9] = [
    ("fal", "FAL_KEY"),
    ("minimax", "MINIMAX_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("bfl", "BFL_API_KEY"),
    ("replicate", "REPLICATE_API_TOKEN"),
    ("google", "GEMINI_API_KEY"),
    ("runway", "RUNWAYML_API_SECRET"),
    ("luma", "LUMA_API_KEY"),
    ("elevenlabs", "ELEVENLABS_API_KEY"),
];
const REGISTRY: [&str; 3] = [
    "DSTACK_DOCKER_USERNAME",
    "DSTACK_DOCKER_PASSWORD",
    "DSTACK_DOCKER_REGISTRY",
];
pub fn trust(network: &str) -> Result<Value> {
    match network {
        "mainnet" => Ok(
            json!({"network":"mainnet","netuid":117,"coordinatorUrl":"https://subnet.everyframe.studio/mainnet/","publicKey":"MCowBQYDK2VwAyEAkgU6E2ap4bOhyjJ4r/f5pjdE4abHpEAjJq0q2S/zan4=","genesis":"0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"}),
        ),
        "testnet" => Ok(
            json!({"network":"testnet","netuid":566,"coordinatorUrl":"https://subnet.everyframe.studio","publicKey":"MCowBQYDK2VwAyEAe8///9EC+z/zha4IdGNQYDvXV12MA2Z3aWe0oLAy4fc="}),
        ),
        _ => Err(Error("unsupported_network")),
    }
}
pub fn configured(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        (10..=8000).contains(&s.len()) && s != DISABLED && !s.contains(['\r', '\n', '\0'])
    })
}
pub fn valid_environment(v: &Value) -> bool {
    let Some(a) = v.as_array() else { return false };
    let Some(keys) = a.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let set = keys
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    set.len() == keys.len()
        && set.contains("MINER_ID")
        && set.contains("MINER_TOKEN")
        && PROVIDERS.iter().any(|(_, k)| set.contains(k))
        && (!REGISTRY.iter().any(|k| set.contains(k)) || REGISTRY.iter().all(|k| set.contains(k)))
        && keys.iter().all(|k| {
            ["MINER_ID", "MINER_TOKEN"].contains(k)
                || REGISTRY.contains(k)
                || PROVIDERS.iter().any(|(_, key)| key == k)
        })
}
pub fn registry_required(v: &Value) -> bool {
    v["request"]["compose_file"]["allowed_envs"]
        .as_array()
        .is_some_and(|a| a.iter().any(|k| k == "DSTACK_DOCKER_PASSWORD"))
}
pub fn invited_keys(v: &Value) -> Vec<String> {
    v["request"]["compose_file"]["allowed_envs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|k| {
                    k.as_str()
                        .filter(|k| PROVIDERS.iter().any(|(_, key)| key == k))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn provider_summary(v: &Value, creds: &Value) -> Value {
    let allowed = invited_keys(v);
    json!(PROVIDERS.iter().map(|(p,k)|json!({"provider":p,"key":k,"releaseAllows":allowed.iter().any(|a|a==k),"configured":configured(&creds[k]),"usable":allowed.iter().any(|a|a==k)&&configured(&creds[k])})).collect::<Vec<_>>())
}
pub fn validate(envelope: &Value, t: &Value, now: i64, allow_expired: bool) -> Result<Value> {
    let v = verified(envelope, s(&t["publicKey"])?)?;
    need(
        v.is_object()
            && v["kind"] == "everyframe-miner-invitation-v1"
            && ["network", "netuid", "coordinatorUrl"]
                .iter()
                .all(|k| v[k] == t[k]),
        "wrong_network_or_coordinator",
    )?;
    if t["network"] == "mainnet" {
        need(v["genesis"] == t["genesis"], "wrong_chain_genesis")?
    };
    let issued = n(&v["issuedAt"])?;
    let expiry = n(&v["expiresAt"])?;
    need(
        issued <= now + 30000 && expiry > issued && (allow_expired || expiry > now),
        "invitation_expired",
    )?;
    need(
        matches("[a-zA-Z0-9_-]{1,64}", &v["minerId"]) && matches("[0-9a-f]{64}", &v["tokenHash"]),
        "invalid_miner",
    )?;
    need(
        matches("[1-9A-HJ-NP-Za-km-z]{47,49}", &v["hotkey"])
            && v["ownershipVerifiedByOperator"] == true,
        "operator_wallet_verification_required",
    )?;
    need(
        matches("[a-zA-Z0-9_.-]{1,80}", &v["release"]),
        "invalid_release",
    )?;
    for (k, len) in [("appId", 40), ("previousComposeHash", 64)] {
        need(
            v.get(k).is_some() && (v[k].is_null() || matches(&format!("[0-9a-f]{{{len}}}"), &v[k])),
            "invalid_deployment_identity",
        )?
    }
    for k in ["composeHash", "osImageHash"] {
        need(matches("[0-9a-f]{64}", &v[k]), "invalid_deployment_hash")?
    }
    let r = &v["request"];
    exact(
        r,
        &[
            "compose_file",
            "disk_size",
            "image",
            "instance_type",
            "kms",
            "listed",
            "name",
        ],
    )?;
    need(
        r["name"] == format!("everyframe-{}", s(&v["minerId"])?)
            && r["instance_type"] == "tdx.small"
            && r["disk_size"] == 20
            && r["image"] == "dstack-0.5.9"
            && r["kms"] == "PHALA"
            && r["listed"] == false,
        "unsupported_deployment",
    )?;
    let c = &r["compose_file"];
    need(
        c.is_object()
            && c["name"] == r["name"]
            && c["kms_enabled"] == true
            && c["gateway_enabled"] == false
            && c["public_logs"] == false
            && c["public_sysinfo"] == false
            && c["public_tcbinfo"] == true
            && valid_environment(&c["allowed_envs"]),
        "unsafe_compose",
    )?;
    let docker = s(&c["docker_compose_file"])?;
    let script = s(&c["pre_launch_script"])?;
    need(
        docker.len() < 20000
            && script.len() < 10000
            && regex::Regex::new(r"image:\s*ghcr\.io/[a-z0-9/_-]+@sha256:[0-9a-f]{64}\s")
                .unwrap()
                .is_match(docker),
        "digest_pinned_image_required",
    )?;
    need(
        registry_required(&v) || script == PUBLIC_SCRIPT,
        "public_image_startup_mismatch",
    )?;
    Ok(v)
}
pub fn discount(v: &str) -> Result<i64> {
    need(
        matches(r"(?:0|[1-9]\d?)(?:\.\d{1,2})?", &json!(v)),
        "invalid_discount",
    )?;
    let (a, b) = v.split_once('.').unwrap_or((v, ""));
    let bp = a.parse::<i64>().map_err(|_| Error("invalid_discount"))? * 100
        + format!("{b:0<2}")
            .parse::<i64>()
            .map_err(|_| Error("invalid_discount"))?;
    need(bp <= 9990, "invalid_discount")?;
    Ok(bp)
}
