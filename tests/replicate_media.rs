use everyframe_miner::{
    Error, Result, native_media,
    network::{Http, Request},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

const MODELS: &str = include_str!("../config/replicate-models.json");

#[test]
fn every_replicate_offer_has_a_valid_provider_body() {
    let registry: Value = serde_json::from_str(MODELS).unwrap();
    for (id, model) in registry.as_object().unwrap() {
        let spec = native_media::spec(&json!({
            "model":id,"prompt":"A quiet lake","references":model["exampleReferences"].as_array().cloned().unwrap_or_default()
        })).unwrap_or_else(|error| panic!("{id}: {error:?}"));
        let refs = spec["input"]["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["url"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let (path, body) = native_media::body(&spec, &refs).unwrap();
        assert_eq!(path, model["apiPath"]);
        assert!(!body.is_null());
    }
}

#[derive(Default)]
struct Predictions {
    posted: Cell<u32>,
    body: RefCell<Value>,
    changed: Cell<bool>,
    unsafe_output: Cell<bool>,
    downloads: Cell<usize>,
}

impl Http for Predictions {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        if request.url
            == "https://api.replicate.com/v1/models/wan-video/wan-2.7-image-pro/predictions"
        {
            assert_eq!(request.method, "POST");
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(k, v)| k == "authorization" && v == "Bearer replicate-fixture-token")
            );
            self.posted.set(self.posted.get() + 1);
            let envelope: Value = serde_json::from_slice(&request.body.unwrap()).unwrap();
            assert_eq!(envelope["input"]["num_outputs"], 2);
            self.body.replace(envelope["input"].clone());
            return Ok(serde_json::to_vec(&json!({"id":"prediction_123"})).unwrap());
        }
        if request.url == "https://api.replicate.com/v1/predictions/prediction_123" {
            let mut input = self.body.borrow().clone();
            if self.changed.get() {
                input["prompt"] = json!("Changed");
            }
            let output = if self.unsafe_output.get() {
                "https://untrusted.example/1.png"
            } else {
                "https://replicate.delivery/1.png"
            };
            return Ok(serde_json::to_vec(&json!({
                "id":"prediction_123","model":"wan-video/wan-2.7-image-pro","status":"succeeded",
                "input":input,"output":[output,"https://replicate.delivery/2.png"]
            }))
            .unwrap());
        }
        if request.url.starts_with("https://replicate.delivery/") {
            assert!(request.headers.is_empty());
            self.downloads.set(self.downloads.get() + 1);
            return Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec());
        }
        Err(Error("unexpected_network"))
    }
}

#[test]
fn a_restarted_worker_reads_bound_multi_image_outputs_without_resubmitting() {
    let spec = native_media::spec(&json!({
        "model":"direct/replicate/wan-2.7-image-pro","prompt":"A quiet lake","parameters":{"num_outputs":2}
    })).unwrap();
    let api = Predictions::default();
    let auth = || json!({"REPLICATE_API_TOKEN":"replicate-fixture-token"});
    let receipt = Router::new(&api, auth()).submit(&spec).unwrap();
    let mut worker = Router::new(&api, auth());
    assert_eq!(worker.poll(&receipt, &spec).unwrap(), "COMPLETED");
    let out = worker.result(&receipt, &spec).unwrap();
    let bundle: Value = serde_json::from_slice(&out.media).unwrap();
    assert_eq!(bundle["parts"].as_array().unwrap().len(), 2);
    assert_eq!(api.posted.get(), 1);
    assert_eq!(api.downloads.get(), 2);
    api.changed.set(true);
    assert_eq!(
        worker.poll(&receipt, &spec).unwrap_err().0,
        "provider_input_mismatch"
    );
    api.changed.set(false);
    api.unsafe_output.set(true);
    assert!(worker.result(&receipt, &spec).is_err());
    assert_eq!(api.downloads.get(), 2);
}
