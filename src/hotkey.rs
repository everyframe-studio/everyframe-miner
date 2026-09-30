//! Local sr25519 ownership proof. Only scoped delegates leave the machine.
use crate::{Error, Result, n, need, network::Request, now, protocol, s, state};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::SigningKey;
use schnorrkel::{ExpansionMode, MiniSecretKey, PublicKey, Signature};
use serde_json::{Value, json};
use std::path::Path;
use zeroize::Zeroizing;

pub fn public_address(address: &str) -> Result<[u8; 32]> {
    let b = bs58::decode(address)
        .into_vec()
        .map_err(|_| Error("invalid_hotkey_address"))?;
    need(b.len() == 35 && b[0] == 42, "invalid_hotkey_address")?;
    let h = Blake2b512::digest([b"SS58PRE".as_slice(), &b[..33]].concat());
    need(b[33..] == h[..2], "invalid_hotkey_address")?;
    b[1..33]
        .try_into()
        .map_err(|_| Error("invalid_hotkey_address"))
}
pub fn address(public: &[u8; 32]) -> String {
    let mut b = vec![42];
    b.extend_from_slice(public);
    let hash = Blake2b512::digest([b"SS58PRE".as_slice(), &b].concat());
    b.extend_from_slice(&hash[..2]);
    bs58::encode(b).into_string()
}
pub fn chain(inv: &Value) -> Result<Value> {
    let v: Value = serde_json::from_str(if inv["network"] == "mainnet" {
        include_str!("../config/mainnet.json")
    } else {
        include_str!("../config/testnet.json")
    })
    .map_err(|_| Error("invalid_chain"))?;
    Ok(json!({"network":v["network"],"netuid":v["netuid"],"genesis":v["genesisHash"]}))
}
pub fn create(path: &Path, inv: &Value, scope: &str) -> Result<Value> {
    need(
        inv["authMode"] == "hotkey-v1" && ["console", "worker"].contains(&scope),
        "hotkey_deployment_required",
    )?;
    let pair = local_pair(path, s(&inv["hotkey"])?)?;
    let key = SigningKey::from_bytes(&protocol::random::<32>());
    let issued = now();
    let expires = n(&inv["expiresAt"])?.min(issued + 30 * 86400000);
    need(expires > issued, "deployment_expired")?;
    let value = json!({"kind":"everyframe-hotkey-delegation-v1","chain":chain(inv)?,"audience":inv["coordinatorUrl"].as_str().unwrap_or("").trim_end_matches('/').to_string()+"/","minerId":inv["minerId"],"hotkey":inv["hotkey"],"delegateKey":protocol::export(key.verifying_key().as_bytes(),true),"scope":scope,"keyVersion":inv["keyVersion"],"issuedAt":issued,"expiresAt":expires});
    let signature = pair.sign_simple(b"substrate", protocol::canonical(&value)?.as_bytes());
    Ok(
        json!({"secret":hex::encode(key.to_bytes()),"certificate":{"value":value,"signature":hex::encode(signature.to_bytes())}}),
    )
}
fn local_pair(path: &Path, address: &str) -> Result<schnorrkel::Keypair> {
    let raw = Zeroizing::new(state::read(path, true, 64000)?);
    let v: Value = serde_json::from_slice(&raw)
        .map_err(|_| Error("unencrypted_sr25519_hotkey_file_required"))?;
    let seed = Zeroizing::new(
        hex::decode(s(&v["secretSeed"])?.trim_start_matches("0x"))
            .map_err(|_| Error("invalid_hotkey_seed"))?,
    );
    let pair = MiniSecretKey::from_bytes(&seed)
        .map_err(|_| Error("sr25519_seed_required"))?
        .expand_to_keypair(ExpansionMode::Ed25519);
    let pubkey = public_address(address)?;
    need(
        pair.public.to_bytes() == pubkey,
        "hotkey_does_not_match_deployment",
    )?;
    if let Some(declared) = v["ss58Address"].as_str() {
        need(
            public_address(declared)? == pubkey,
            "hotkey_file_identity_mismatch",
        )?;
    }
    Ok(pair)
}
// Never signs arbitrary remote bytes: verify the pinned coordinator, exact
// off-chain domain, requested identity/nonce and two-minute challenge lifetime.
pub fn enrollment_proof(
    path: &Path,
    envelope: &Value,
    trust: &Value,
    address: &str,
    nonce: &str,
) -> Result<String> {
    let c = protocol::verified(envelope, s(&trust["publicKey"])?)?;
    protocol::exact(
        &c,
        &[
            "kind",
            "chain",
            "audience",
            "hotkey",
            "nonce",
            "challenge",
            "releaseHash",
            "issuedAt",
            "expiresAt",
        ],
    )?;
    need(
        c["kind"] == "everyframe-public-enrollment-v1"
            && c["chain"] == chain(trust)?
            && c["audience"]
                == s(&trust["coordinatorUrl"])?
                    .trim_end_matches('/')
                    .to_string()
                    + "/"
            && c["hotkey"] == address
            && c["nonce"] == nonce
            && crate::matches("[a-f0-9]{48}", &c["challenge"])
            && crate::matches("[a-f0-9]{64}", &c["releaseHash"]),
        "invalid_enrollment_challenge",
    )?;
    let issued = n(&c["issuedAt"])?;
    let expires = n(&c["expiresAt"])?;
    need(
        issued <= now() + 30000 && expires > now() && expires.checked_sub(issued) == Some(120000),
        "enrollment_expired",
    )?;
    let pair = local_pair(path, address)?;
    Ok(hex::encode(
        pair.sign_simple(b"substrate", protocol::canonical(&c)?.as_bytes())
            .to_bytes(),
    ))
}
pub fn validate(auth: &Value, inv: &Value, scope: &str) -> Result<()> {
    let c = &auth["certificate"]["value"];
    need(
        c["kind"] == "everyframe-hotkey-delegation-v1"
            && c["chain"] == chain(inv)?
            && c["minerId"] == inv["minerId"]
            && c["hotkey"] == inv["hotkey"]
            && c["keyVersion"] == inv["keyVersion"]
            && c["scope"] == scope
            && c["audience"] == s(&inv["coordinatorUrl"])?.trim_end_matches('/').to_string() + "/",
        "invalid_hotkey_delegation",
    )?;
    let issued = n(&c["issuedAt"])?;
    let expires = n(&c["expiresAt"])?;
    need(
        issued <= now() + 30000
            && expires > now()
            && expires > issued
            && expires - issued <= 30 * 86400000,
        "hotkey_delegation_expired_reinitialize",
    )?;
    let pubkey = PublicKey::from_bytes(&public_address(s(&c["hotkey"])?)?)
        .map_err(|_| Error("invalid_hotkey"))?;
    let sig = Signature::from_bytes(
        &hex::decode(s(&auth["certificate"]["signature"])?)
            .map_err(|_| Error("invalid_hotkey_signature"))?,
    )
    .map_err(|_| Error("invalid_hotkey_signature"))?;
    pubkey
        .verify_simple(b"substrate", protocol::canonical(c)?.as_bytes(), &sig)
        .map_err(|_| Error("invalid_hotkey_signature"))?;
    need(
        protocol::export(signing_key(auth)?.verifying_key().as_bytes(), true) == c["delegateKey"],
        "delegate_key_mismatch",
    )
}
fn signing_key(auth: &Value) -> Result<SigningKey> {
    let seed = Zeroizing::new(
        hex::decode(s(&auth["secret"])?).map_err(|_| Error("invalid_delegate_key"))?,
    );
    let bytes: &[u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| Error("invalid_delegate_key"))?;
    Ok(SigningKey::from_bytes(bytes))
}
pub fn authorize(r: &mut Request, auth: &Value, body: &Value) -> Result<()> {
    let url = url::Url::parse(&r.url).map_err(|_| Error("invalid_auth_url"))?;
    let c = &auth["certificate"]["value"];
    let audience =
        url::Url::parse(s(&c["audience"])?).map_err(|_| Error("invalid_auth_audience"))?;
    need(
        url.origin() == audience.origin(),
        "hotkey_auth_wrong_origin",
    )?;
    let base = audience.path().trim_end_matches('/');
    let path = url
        .path()
        .strip_prefix(base)
        .ok_or(Error("hotkey_auth_wrong_path"))?;
    need(path.starts_with("/v1/"), "hotkey_auth_wrong_path")?;
    let path = format!(
        "{path}{}",
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    need(
        n(&c["expiresAt"])? > now(),
        "hotkey_delegation_expired_reinitialize",
    )?;
    let proof = protocol::signed(
        &json!({"kind":"everyframe-hotkey-request-v1","certificateHash":protocol::digest(&auth["certificate"])?,"method":r.method,"path":path,"bodyHash":protocol::digest(body)?,"nonce":protocol::id(),"at":now()}),
        &signing_key(auth)?,
    )?;
    r.headers.push((
        "x-everyframe-auth".into(),
        protocol::b64(
            json!({"certificate":auth["certificate"],"proof":proof})
                .to_string()
                .as_bytes(),
        ),
    ));
    Ok(())
}
