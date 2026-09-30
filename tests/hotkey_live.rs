//! Opt-in read-only authentication check. Never deploys or submits paid work.
use everyframe_miner::{
    hotkey,
    network::{Http, PublicHttp, Request},
    now, protocol,
};
use serde_json::{Value, json};
use std::path::Path;
#[test]
#[ignore = "requires an explicitly prepared, disabled live probe identity"]
fn read_only_registered_hotkey_probe() {
    let config = std::env::var("EVERYFRAME_AUTH_PROBE_CONFIG").expect("probe config path");
    let v: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    assert_eq!(v["minerId"], "hotkey-auth-probe-20260930");
    let inv = json!({"authMode":"hotkey-v1","network":"mainnet","coordinatorUrl":"https://subnet.everyframe.studio/mainnet/","minerId":v["minerId"],"hotkey":v["hotkey"],"keyVersion":1,"expiresAt":now()+60000});
    let auth = hotkey::create(
        Path::new(v["hotkeyFile"].as_str().unwrap()),
        &inv,
        "console",
    )
    .unwrap();
    let mut r = Request::get(
        format!(
            "https://subnet.everyframe.studio/mainnet/v1/miner/status?nonce={}",
            protocol::id()
        ),
        &["subnet.everyframe.studio"],
    );
    hotkey::authorize(&mut r, &auth, &Value::Null).unwrap();
    let response = PublicHttp.json(r).unwrap();
    let result = protocol::verified(&response, v["coordinatorKey"].as_str().unwrap()).unwrap();
    assert_eq!(result["minerId"], v["minerId"]);
    assert_eq!(result["enabled"], false);
    assert_eq!(result["activeJobs"], 0);
    println!(
        "Live hotkey proof accepted: disabled identity, zero jobs, no miner token, no chain transaction."
    );
}
