//! Local onboarding. Registration is a read-only chain check, not a transaction.
use crate::{
    Error, Result, hotkey, invitation, matches, need,
    network::{Http, Request},
    now, s,
    state::{self, State},
};
use blake2::{Blake2b, Digest, digest::consts::U16};
use serde_json::{Value, json};
use std::{
    hash::Hasher,
    path::{Path, PathBuf},
};
use twox_hash::XxHash64;

pub fn enroll(http: &dyn Http, trust: &Value, path: &Path) -> Result<Value> {
    let address = file_address(path)?;
    let base = s(&trust["coordinatorUrl"])?.trim_end_matches('/');
    let nonce = crate::protocol::id();
    let post = |route: &str, body: &Value| {
        http.json(
            Request::get(
                format!("{base}/onboarding/{route}"),
                &["subnet.everyframe.studio"],
            )
            .json("POST", body),
        )
    };
    let challenge = post("challenge", &json!({"hotkey":address,"nonce":nonce}))?;
    let signature = hotkey::enrollment_proof(path, &challenge, trust, &address, &nonce)?;
    let envelope = post(
        "enroll",
        &json!({"challenge":challenge,"signature":signature}),
    )?;
    let deployment = invitation::validate(&envelope, trust, now(), false)?;
    need(
        deployment["hotkey"] == address
            && deployment["publicOnboarding"] == true
            && deployment["authMode"] == "hotkey-v1",
        "invalid_public_deployment",
    )?;
    Ok(envelope)
}

pub fn wallet_path(wallet: &str, key: &str) -> Result<PathBuf> {
    need(
        matches("[A-Za-z0-9_-]{1,80}", &json!(wallet))
            && matches("[A-Za-z0-9_-]{1,80}", &json!(key)),
        "invalid_wallet_name",
    )?;
    Ok(
        PathBuf::from(std::env::var("HOME").map_err(|_| Error("home_required"))?)
            .join(".bittensor/wallets")
            .join(wallet)
            .join("hotkeys")
            .join(key),
    )
}

pub fn file_address(path: &Path) -> Result<String> {
    let raw = zeroize::Zeroizing::new(state::read(path, true, 64000)?);
    let v: Value = serde_json::from_slice(&raw)
        .map_err(|_| Error("hotkey_file_unreadable_use_hotkey_ss58"))?;
    let address = s(&v["ss58Address"])?;
    hotkey::public_address(address)?;
    Ok(address.into())
}

fn prefix(name: &str) -> Vec<u8> {
    ["SubtensorModule", name]
        .iter()
        .flat_map(|text| {
            (0..2)
                .flat_map(|seed| {
                    let mut h = XxHash64::with_seed(seed);
                    h.write(text.as_bytes());
                    h.finish().to_le_bytes()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
pub fn uid_key(netuid: u16, public: &[u8; 32]) -> String {
    // Subtensor Uids: Identity(NetUid/u16), Blake2_128Concat(AccountId32).
    // https://github.com/opentensor/subtensor/blob/main/pallets/subtensor/src/lib.rs
    // The reverse Keys lookup below must agree at the same finalized block.
    let mut key = prefix("Uids");
    key.extend(netuid.to_le_bytes());
    key.extend(Blake2b::<U16>::digest(public));
    key.extend(public);
    format!("0x{}", hex::encode(key))
}
fn reverse_key(netuid: u16, uid: u16) -> String {
    let mut key = prefix("Keys");
    key.extend(netuid.to_le_bytes());
    key.extend(uid.to_le_bytes());
    format!("0x{}", hex::encode(key))
}
fn rpc(http: &dyn Http, host: &str, method: &str, params: Value) -> Result<Value> {
    let r = Request::get(format!("https://{host}"), &[host]).json(
        "POST",
        &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    );
    let out = http.json(r)?;
    need(
        out["jsonrpc"] == "2.0"
            && out["id"] == 1
            && out.get("result").is_some()
            && out.get("error").is_none(),
        "chain_lookup_failed",
    )?;
    Ok(out["result"].clone())
}
pub fn check_registration(http: &dyn Http, network: &str, address: &str) -> Result<Value> {
    invitation::trust(network)?;
    let public = hotkey::public_address(address)?;
    let chain = hotkey::chain(&json!({"network":network}))?;
    let netuid = chain["netuid"].as_u64().ok_or(Error("invalid_chain"))? as u16;
    let host = if network == "mainnet" {
        "entrypoint-finney.opentensor.ai"
    } else {
        "test.finney.opentensor.ai"
    };
    need(
        rpc(http, host, "chain_getBlockHash", json!([0]))? == chain["genesis"],
        "wrong_chain_genesis",
    )?;
    let block = rpc(http, host, "chain_getFinalizedHead", json!([]))?;
    need(
        matches("0x[0-9a-fA-F]{64}", &block),
        "invalid_finalized_block",
    )?;
    let stored = rpc(
        http,
        host,
        "state_getStorage",
        json!([uid_key(netuid, &public), block]),
    )?;
    if stored.is_null() {
        return Ok(
            json!({"registered":false,"network":network,"netuid":netuid,"hotkey":address,"finalizedBlock":block,"chainTransactionSubmitted":false,"next":format!("Register with your wallet tool on {network} SN{netuid}, then rerun this command. No registration fee has been paid by everycli.")}),
        );
    }
    need(matches("0x[0-9a-fA-F]{4}", &stored), "invalid_chain_uid")?;
    let bytes = hex::decode(&s(&stored)?[2..]).map_err(|_| Error("invalid_chain_uid"))?;
    let uid = u16::from_le_bytes([bytes[0], bytes[1]]);
    let reverse = rpc(
        http,
        host,
        "state_getStorage",
        json!([reverse_key(netuid, uid), block]),
    )?;
    need(
        reverse == format!("0x{}", hex::encode(public)),
        "chain_hotkey_mapping_mismatch",
    )?;
    Ok(
        json!({"registered":true,"network":network,"netuid":netuid,"hotkey":address,"uid":uid,"finalizedBlock":block,"checkedAt":now(),"chainTransactionSubmitted":false,"next":"Hotkey membership verified. Set API keys, then run miner init to prove ownership and fetch the signed deployment automatically."}),
    )
}

pub fn save_registration(state: &State, receipt: &Value, path: Option<&Path>) -> Result<()> {
    need(receipt["registered"] == true, "hotkey_not_registered")?;
    let _lock = state.lock()?;
    let config = state.read("config", true)?;
    if !config.is_null() {
        need(
            config["invitation"]["value"]["hotkey"] == receipt["hotkey"]
                && config["invitation"]["value"]["network"] == receipt["network"],
            "hotkey_does_not_match_deployment",
        )?;
    }
    let mut record = receipt.clone();
    if let Some(path) = path {
        record["hotkeyFile"] = json!(
            state::absolute(path)?
                .to_str()
                .ok_or(Error("unsafe_path"))?
        );
    }
    state.write("registration", &record)
}

pub fn credential_key(provider: &str) -> Result<&'static str> {
    if let Some((_, key)) = crate::balances::BILLING_KEYS
        .iter()
        .find(|(p, _)| *p == provider)
    {
        return Ok(key);
    }
    if provider == "phala" {
        return Ok("PHALA_CLOUD_API_KEY");
    }
    invitation::PROVIDERS
        .iter()
        .find(|(p, _)| *p == provider)
        .map(|(_, key)| *key)
        .ok_or(Error("unknown_provider"))
}
pub fn save_keys(state: &State, updates: &Value, remove: Option<&str>) -> Result<Value> {
    let entries = updates.as_object().ok_or(Error("invalid_credential"))?;
    for (key, value) in entries {
        need(
            (key == "PHALA_CLOUD_API_KEY"
                || invitation::PROVIDERS.iter().any(|(_, k)| *k == key)
                || crate::balances::BILLING_KEYS.iter().any(|(_, k)| *k == key))
                && invitation::configured(value),
            "invalid_credential",
        )?;
    }
    let remove_key = remove.map(credential_key).transpose()?;
    let _lock = state.lock()?;
    let deployment = state.read("deployment", true)?;
    need(
        !deployment["phase"]
            .as_str()
            .unwrap_or("")
            .ends_with("_intent"),
        "reconciliation_required",
    )?;
    let mut creds = state.read("credentials", true)?;
    if creds.is_null() {
        creds = json!({});
    }
    let object = creds
        .as_object_mut()
        .ok_or(Error("invalid_credentials_file"))?;
    for (k, v) in entries {
        object.insert(k.clone(), v.clone());
    }
    if let Some(k) = remove_key {
        object.remove(k);
    }
    if !entries.is_empty() || remove_key.is_some() {
        state.write("credentials", &creds)?;
    }
    let billing_only = entries
        .keys()
        .map(String::as_str)
        .chain(remove_key)
        .all(|key| crate::balances::BILLING_KEYS.iter().any(|(_, k)| *k == key));
    Ok(
        json!({"savedLocally":true,"updated":entries.keys().collect::<Vec<_>>(),"removed":remove_key,"workerChanged":false,"next":if billing_only { "Billing-only keys stay on this device and never enter worker environments. Use miner balances to check, or miner balances --publish to sync amounts. Revoke leaked keys at the provider." } else if deployment["appId"].is_null() { "Keys saved locally. Initialize and deploy when ready." } else { "The running worker is unchanged. Use miner apply-api-keys to explicitly drain, encrypt and apply generation keys; then wait for fresh admission before resuming. Billing-only keys are never deployed. To revoke a leaked key, also revoke it at the provider." }}),
    )
}
