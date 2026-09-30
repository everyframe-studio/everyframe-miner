mod common;
use everyframe_miner::{hotkey, network::Request, now, protocol};
use schnorrkel::{ExpansionMode, MiniSecretKey};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
fn fixture() -> (tempfile::TempDir, Value) {
    let d = common::tempdir();
    let seed = [7u8; 32];
    let pair = MiniSecretKey::from_bytes(&seed)
        .unwrap()
        .expand_to_keypair(ExpansionMode::Ed25519);
    let address = hotkey::address(&pair.public.to_bytes());
    std::fs::write(
        d.path().join("hotkey"),
        json!({"secretSeed":format!("0x{}",hex::encode(seed)),"ss58Address":address}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(
        d.path().join("hotkey"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    (
        d,
        json!({"authMode":"hotkey-v1","network":"mainnet","coordinatorUrl":"https://subnet.everyframe.studio/mainnet/","minerId":"fixture","hotkey":address,"keyVersion":1,"expiresAt":now()+86400000}),
    )
}
#[test]
fn local_hotkey_delegates_scoped_auth_without_exporting_wallet() {
    let (d, inv) = fixture();
    let a = hotkey::create(&d.path().join("hotkey"), &inv, "worker").unwrap();
    hotkey::validate(&a, &inv, "worker").unwrap();
    assert!(hotkey::validate(&a, &inv, "console").is_err());
    assert!(!a.to_string().contains(&hex::encode([7u8; 32])));
    let body = json!({"nonce":"test"});
    let mut r = Request::get(
        "https://subnet.everyframe.studio/mainnet/v1/challenge",
        &["subnet.everyframe.studio"],
    )
    .json("POST", &body);
    hotkey::authorize(&mut r, &a, &body).unwrap();
    assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
    let header = r
        .headers
        .iter()
        .find(|(k, _)| k == "x-everyframe-auth")
        .unwrap();
    let payload: Value = serde_json::from_slice(&protocol::decode(&header.1).unwrap()).unwrap();
    let proof = protocol::verified(
        &payload["proof"],
        a["certificate"]["value"]["delegateKey"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(proof["path"], "/v1/challenge");
    assert_eq!(proof["bodyHash"], protocol::digest(&body).unwrap());
    r.url = "https://evil.example/v1/challenge".into();
    assert!(hotkey::authorize(&mut r, &a, &body).is_err());
}
#[test]
fn rejects_wrong_wallet_insecure_file_tampered_certificate() {
    let (d, mut inv) = fixture();
    let path = d.path().join("hotkey");
    let mut a = hotkey::create(&path, &inv, "console").unwrap();
    a["certificate"]["value"]["scope"] = json!("worker");
    assert!(hotkey::validate(&a, &inv, "worker").is_err());
    inv["hotkey"] = json!(hotkey::address(&[1u8; 32]));
    assert!(hotkey::create(&path, &inv, "console").is_err());
    std::fs::set_permissions(path.clone(), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(hotkey::create(&path, &inv, "console").is_err());
}
#[test]
fn rejects_bad_ss58_checksum() {
    let (_, inv) = fixture();
    let address = inv["hotkey"].as_str().unwrap();
    assert!(hotkey::public_address(address).is_ok());
    let mut bad = address.to_string();
    bad.replace_range(bad.len() - 1.., "1");
    assert!(hotkey::public_address(&bad).is_err());
}
