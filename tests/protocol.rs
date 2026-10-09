use everyframe_miner::{models, protocol::*};
use serde_json::{Value, json};

#[test]
fn configured_release_maps_genesis_hash_and_preserves_chain_fencing() {
    use everyframe_miner::worker::validate_release;
    for (config, url) in [
        (
            include_str!("../config/mainnet.json"),
            "https://subnet.everyframe.studio/mainnet/",
        ),
        (
            include_str!("../config/testnet.json"),
            "https://subnet.everyframe.studio/",
        ),
    ] {
        let c: Value = serde_json::from_str(config).unwrap();
        let v = json!({"release":"test-configured","coordinatorUrl":url,
            "coordinatorSigningKey":Keys::default().signing_key(),
            "chain":{"network":c["network"],"netuid":c["netuid"],"genesis":c["genesisHash"]}});
        assert!(validate_release(&v).is_ok());
        for (field, value) in [
            ("network", json!("other")),
            ("netuid", json!(999)),
            ("genesis", json!("0xwrong")),
        ] {
            let mut bad = v.clone();
            bad["chain"][field] = value;
            assert!(validate_release(&bad).is_err());
        }
        let mut bad = v.clone();
        bad["coordinatorUrl"] = json!(if c["network"] == "finney" {
            "https://subnet.everyframe.studio/"
        } else {
            "https://subnet.everyframe.studio/mainnet/"
        });
        assert!(validate_release(&bad).is_err());
    }
}

#[test]
fn decrypts_original_node_worker_assignment() {
    let f: Value = serde_json::from_str(include_str!("fixtures/worker-wire.json")).unwrap();
    let secret = x25519_dalek::StaticSecret::from([0x11; 32]);
    assert_eq!(
        unseal(&f["packet"], &secret, f["context"].as_str().unwrap()).unwrap(),
        f["value"]
    );
    assert_eq!(
        hex::encode(report_data(&f["value"]).unwrap()),
        f["reportData"]
    );
}

#[test]
fn original_canonical_and_ed25519_vectors() {
    let f: Value = serde_json::from_str(include_str!("fixtures/wire-vectors.json")).unwrap();
    assert_eq!(canonical(&f["value"]).unwrap(), f["canonical"]);
    assert_eq!(
        verified(&f["envelope"], f["publicKey"].as_str().unwrap()).unwrap(),
        f["value"]
    );
    assert_eq!(digest(&f["compose"]).unwrap(), f["composeHash"]);
    let mut changed = f["envelope"].clone();
    changed["value"]["array"][0] = json!(2);
    assert!(verified(&changed, f["publicKey"].as_str().unwrap()).is_err());
}
#[test]
fn all_45_original_model_hashes_match() {
    let f: Value = serde_json::from_str(include_str!("fixtures/compatibility.json")).unwrap();
    assert_eq!(models::MODELS.as_object().unwrap().len(), 45);
    for (id, hash) in f["models"].as_object().unwrap() {
        let mut input = json!({"model":id,"prompt":"Everyframe protocol compatibility fixture"});
        if models::info(id).unwrap()["seed"] == true {
            input["seed"] = json!(42)
        }
        let spec = models::spec(&input).unwrap();
        assert_eq!(digest(&spec).unwrap(), *hash, "{id}");
        models::check(&spec).unwrap();
        let mut invalid = spec.clone();
        invalid["input"]["unexpected"] = json!(true);
        assert!(models::check(&invalid).is_err(), "{id}");
    }
}
#[test]
fn authenticated_encryption_binds_context_recipient_and_contents() {
    let keys = Keys::default();
    let other = Keys::default();
    let v = json!({"prompt":"Rust 🎬","seed":42});
    let p = seal(&v, &keys.encryption_key(), "session:nonce").unwrap();
    assert_eq!(unseal(&p, &keys.encryption, "session:nonce").unwrap(), v);
    assert!(unseal(&p, &keys.encryption, "session:other").is_err());
    assert!(unseal(&p, &other.encryption, "session:nonce").is_err());
    let mut changed = p;
    changed["tag"] = json!(b64(&[0; 16]));
    assert!(unseal(&changed, &keys.encryption, "session:nonce").is_err());
    assert!(seal(&v, &export(&[0; 32], false), "x").is_err());
}
#[test]
fn signatures_and_spki_fail_closed() {
    let k = Keys::default();
    let v = json!({"foo":"bar"});
    let p = signed(&v, &k.signing).unwrap();
    assert_eq!(verified(&p, &k.signing_key()).unwrap(), v);
    assert!(verified(&p, &Keys::default().signing_key()).is_err());
    assert!(public(&k.encryption_key(), true).is_err());
    assert!(public(&b64(&[0; 32]), false).is_err());
    let mut p = p;
    p["extra"] = json!(1);
    assert!(verified(&p, &k.signing_key()).is_err());
}
#[test]
fn canonical_numbers_and_prompt_limits() {
    for v in [
        json!(1.1),
        json!(9007199254740992u64),
        json!(-9007199254740992i64),
    ] {
        assert!(canonical(&v).is_err());
    }
    assert_eq!(
        canonical(&json!(9007199254740991u64)).unwrap(),
        "9007199254740991"
    );
    for input in [
        json!({"prompt":" ","seed":1}),
        json!({"prompt":"x","seed":-1}),
        json!({"prompt":"x","seed":2147483648u64}),
        json!({"prompt":"x","seed":1,"url":"https://evil.test"}),
        json!({"prompt":"x"}),
        json!({"prompt":"a".repeat(8001),"seed":1}),
    ] {
        assert!(models::spec(&input).is_err());
    }
    assert!(serde_json::from_str::<Value>(r#""\ud800""#).is_err());
}
#[test]
fn report_data_is_domain_separated_sha512() {
    use sha2::{Digest, Sha512};
    let v = json!({"a":1});
    let mut expected = Sha512::new();
    expected.update(b"everyframe-attestation-v1\0{\"a\":1}");
    assert_eq!(report_data(&v).unwrap(), expected.finalize().to_vec());
}
