//! Versioned miner agreement. Acceptance is local, explicit and identity bound.
use crate::{Error, Result, hotkey, need, now, state::State};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const VERSION: &str = "2026-11-04";
pub const TEXT: &str = include_str!("../docs/miner-terms.md");

pub fn hash() -> String {
    hex::encode(Sha256::digest(TEXT.as_bytes()))
}

pub fn accepted(state: &State, network: &str, address: &str) -> Result<bool> {
    let records = state.read("terms-acceptance", true)?;
    if records.is_null() {
        return Ok(false);
    }
    need(
        records["version"] == 1
            && records["acceptances"]
                .as_array()
                .is_some_and(|items| items.len() <= 64),
        "invalid_terms_record",
    )?;
    Ok(records["acceptances"].as_array().is_some_and(|items| {
        items.iter().any(|r| {
            r["termsVersion"] == VERSION
                && r["documentSha256"] == hash()
                && r["network"] == network
                && r["hotkey"] == address
                && r["acceptedAt"]
                    .as_i64()
                    .is_some_and(|at| at > 0 && at <= now())
        })
    }))
}

pub fn view(state: &State, network: &str, address: Option<&str>) -> Result<Value> {
    Ok(
        json!({"termsVersion":VERSION,"documentSha256":hash(),"text":TEXT,
        "network":network,"hotkey":address,
        "accepted":match address { Some(a) => accepted(state,network,a)?, None => false },
        "acceptanceScope":"local-profile"}),
    )
}

pub fn accept(state: &State, network: &str, address: &str, version: &str) -> Result<()> {
    need(version == VERSION, "terms_version_changed")?;
    need(["mainnet", "testnet"].contains(&network), "invalid_network")?;
    hotkey::public_address(address)?;
    state.prepare()?;
    let _lock = state.lock()?;
    if accepted(state, network, address)? {
        return Ok(());
    }
    let mut records = state.read("terms-acceptance", true)?;
    if records.is_null() {
        records = json!({"version":1,"acceptances":[]});
    }
    need(records["version"] == 1, "invalid_terms_record")?;
    let items = records["acceptances"]
        .as_array_mut()
        .ok_or(Error("invalid_terms_record"))?;
    need(items.len() < 64, "invalid_terms_record")?;
    items.push(
        json!({"termsVersion":VERSION,"documentSha256":hash(),"network":network,
        "hotkey":address,"acceptedAt":now(),"cliVersion":env!("CARGO_PKG_VERSION")}),
    );
    state.write("terms-acceptance", &records)
}

pub fn require(
    state: &State,
    network: &str,
    address: &str,
    explicit_version: Option<&str>,
    unattended: bool,
    confirm: &dyn Fn(&str) -> Result<()>,
) -> Result<()> {
    // A stale explicit approval must fail even if a current receipt exists.
    if let Some(version) = explicit_version {
        return accept(state, network, address, version);
    }
    if accepted(state, network, address)? {
        return Ok(());
    }
    need(!unattended, "terms_acceptance_required")?;
    confirm(&format!(
        "{TEXT}\nHotkey: {address}\nNetwork: {network}\n\nAccept Miner Terms {VERSION} and confirm that you are responsible for obtaining the required provider permissions and negotiating provider discounts directly?"
    ))?;
    accept(state, network, address, VERSION)
}
