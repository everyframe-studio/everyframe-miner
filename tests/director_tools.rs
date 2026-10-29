use everyframe_miner::{
    Result, models,
    network::{Http, Request},
    protocol::sha,
    studio_tools,
};
use serde_json::{Value, json};

fn equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equivalent(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| equivalent(v, w)))
        }
        _ => a == b,
    }
}
#[test]
fn director_contracts_match_coordinator_and_provider_bodies() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/director-tools.json")).unwrap();
    assert_eq!(fixtures.len(), 22);
    for fixture in fixtures {
        let spec = &fixture["spec"];
        models::check(spec).unwrap();
        let urls: Vec<String> = serde_json::from_value(fixture["urls"].clone()).unwrap();
        let (path, body) = studio_tools::body(spec, &urls).unwrap();
        assert_eq!(path, fixture["path"].as_str().unwrap());
        assert!(
            equivalent(&body, &fixture["body"]),
            "{}: {body} != {}",
            spec["model"],
            fixture["body"]
        );
        let mut invalid = spec.clone();
        invalid["input"]["parameters"]["unexpected"] = json!(true);
        assert!(models::check(&invalid).is_err());
    }
}
#[test]
fn corrupt_lora_or_reference_hash_is_rejected_before_submission() {
    struct Mock;
    impl Http for Mock {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.method, "GET");
            Ok(b"not a safe tensor".to_vec())
        }
    }
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/director-tools.json")).unwrap();
    let mut value = fixtures
        .into_iter()
        .find(|f| {
            f["spec"]["input"]["references"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["role"] == "lora")
        })
        .unwrap()["spec"]
        .clone();
    let a = value["input"]["references"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["role"] == "lora")
        .unwrap();
    a["sha256"] = json!(sha(b"not a safe tensor"));
    a["bytes"] = json!(17);
    assert!(studio_tools::references(&Mock, &value).is_err());
    assert!(studio_tools::verify_safetensors(b"not a safe tensor").is_err());
}

#[test]
fn director_worker_registry_matches_coordinator() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/director-tool-registry.json")).unwrap();
    assert_eq!(studio_tools::model_registry(), expected);
}
