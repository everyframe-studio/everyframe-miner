use everyframe_miner::{
    Error, Result, key_check, models, native_media,
    network::{Http, Request},
    protocol::{b64, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;

const STANDARD: &str = "direct/azure-foundry/mai-image-2.7";
const FLASH: &str = "direct/azure-foundry/mai-image-2.7-flash";
const KEY: &str = "mock-azure-key-0123456789abcdef";

fn input(model: &str, parameters: Value, references: Vec<Value>) -> Value {
    json!({"model":model,"prompt":"Design a blue paper crane","parameters":parameters,"references":references})
}

fn fixture(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn asset() -> Value {
    let bytes = fixture(1024, 1024);
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_mai?t={}","a".repeat(32)),
        "sha256":sha(&bytes),"bytes":bytes.len(),"mime":"image/png","role":"image","width":1024,"height":1024})
}

fn config(deployments: Value) -> Value {
    json!({"resource":"everyframe-fixture","apiKey":KEY,"deployments":deployments})
}

fn router<'a>(http: &'a Mock, deployments: Value) -> Router<'a> {
    Router::new(
        http,
        json!({"AZURE_FOUNDRY_CREDENTIALS":config(deployments).to_string()}),
    )
}

struct Mock {
    requests: RefCell<Vec<Request>>,
    response: Result<Vec<u8>>,
}

impl Mock {
    fn image(width: u32, height: u32) -> Self {
        Self {
            requests: RefCell::default(),
            response: Ok(json!({"data":[{"b64_json":b64(&fixture(width,height))}]})
                .to_string()
                .into_bytes()),
        }
    }
}

impl Http for Mock {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        let attachment = request.method == "GET";
        self.requests.borrow_mut().push(request);
        if attachment {
            Ok(fixture(1024, 1024))
        } else {
            self.response.clone()
        }
    }
}

#[test]
fn both_deployments_use_only_their_foundry_origin_and_return_one_bound_image() {
    for (model, name) in [(STANDARD, "maker-standard"), (FLASH, "maker-flash")] {
        let http = Mock::image(2048, 1024);
        let mut provider = router(
            &http,
            json!({"standard":"maker-standard","flash":"maker-flash"}),
        );
        let spec =
            models::spec(&input(model, json!({"width":2048,"height":1024}), vec![])).unwrap();
        let reference = provider.submit(&spec).unwrap();
        assert_eq!(provider.poll(&reference, &spec).unwrap(), "COMPLETED");
        let output = provider.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&output.media).unwrap();
        assert_eq!(bundle["parts"][0]["mime"], "image/png");
        assert!(
            provider
                .result(
                    &reference,
                    &models::spec(&input(model, json!({}), vec![])).unwrap()
                )
                .is_err()
        );
        let requests = http.requests.borrow();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(
            request.url,
            "https://everyframe-fixture.services.ai.azure.com/mai/v1/images/generations"
        );
        assert_eq!(request.hosts, ["everyframe-fixture.services.ai.azure.com"]);
        assert!(request.redirect_hosts.is_empty());
        assert_eq!(request.method, "POST");
        assert!(
            request
                .headers
                .iter()
                .any(|(k, v)| k == "api-key" && v == KEY)
        );
        let request_body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
        assert_eq!(
            request_body,
            json!({"model":name,"prompt":"Design a blue paper crane","width":2048,"height":1024})
        );
    }
}

#[test]
fn edits_send_verified_bytes_and_withhold_credentials_from_attachment_origin() {
    let http = Mock::image(1024, 1536);
    let mut provider = router(&http, json!({"flash":"fast-preview"}));
    let spec = models::spec(&input(FLASH, json!({}), vec![asset(); 5])).unwrap();
    assert!(provider.submit(&spec).is_ok());
    let all = http.requests.borrow();
    assert_eq!(all.len(), 6);
    assert!(
        all[..5]
            .iter()
            .all(|r| r.method == "GET" && r.headers.is_empty() && r.redirect_hosts.is_empty())
    );
    let edit = &all[5];
    assert!(edit.url.ends_with("/mai/v1/images/edits"));
    let body = String::from_utf8_lossy(edit.body.as_ref().unwrap());
    assert_eq!(body.matches("name=\"image\"").count(), 5);
    assert!(body.contains("name=\"model\"\r\n\r\nfast-preview"));
    assert!(!body.contains("name=\"width\"") && !body.contains("https://"));
    drop(all);

    let mut mismatch = asset();
    mismatch["sha256"] = json!("0".repeat(64));
    let spec = models::spec(&input(FLASH, json!({}), vec![mismatch])).unwrap();
    assert_eq!(
        provider.submit(&spec).unwrap_err().0,
        "reference_integrity_mismatch"
    );
    assert_eq!(
        http.requests
            .borrow()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}

#[test]
fn deployment_capability_and_contract_validation_are_variant_specific() {
    let http = Mock::image(1024, 1024);
    let mut provider = router(&http, json!({"standard":"maker-standard"}));
    assert!(provider.models().iter().any(|id| id == STANDARD));
    assert!(!provider.models().iter().any(|id| id == FLASH));
    let missing = models::spec(&input(FLASH, json!({}), vec![])).unwrap();
    assert_eq!(
        provider.submit(&missing).unwrap_err().0,
        "foundry_deployment_required"
    );
    assert!(http.requests.borrow().is_empty());
    let creds = config(json!({"standard":"maker-standard"})).to_string();
    assert_eq!(
        key_check::check(&http, "azure-foundry", &creds),
        (
            key_check::Verdict::Unverified,
            "foundry_key_access_unverified"
        )
    );
    assert!(http.requests.borrow().is_empty());
    let mut invalid = config(json!({"standard":"maker"}));
    invalid["resource"] = json!("sub.example.com/azure");
    assert_eq!(
        key_check::check(&http, "azure-foundry", &invalid.to_string()).0,
        key_check::Verdict::Rejected
    );
    assert!(
        native_media::spec(&input(
            STANDARD,
            json!({"width":2048,"height":2048,"background":"transparent"}),
            vec![]
        ))
        .is_ok()
    );
    for (model, params, refs) in [
        (FLASH, json!({"background":"transparent"}), vec![]),
        (FLASH, json!({"width":2049,"height":2048}), vec![]),
        (FLASH, json!({"width":767}), vec![]),
        (FLASH, json!({"height":1024}), vec![asset()]),
        (FLASH, json!({}), vec![asset(); 6]),
    ] {
        assert!(native_media::spec(&input(model, params, refs)).is_err());
    }
}

#[test]
fn a_lost_or_malformed_paid_response_is_never_retried() {
    for response in [Err(Error("request_timeout")), Ok(b"{}".to_vec())] {
        let http = Mock {
            requests: RefCell::default(),
            response,
        };
        let mut provider = router(&http, json!({"standard":"maker-standard"}));
        let spec = models::spec(&input(STANDARD, json!({}), vec![])).unwrap();
        let error = provider.submit(&spec).unwrap_err();
        assert!(everyframe_miner::submission::rejected(error.0).is_none());
        assert_eq!(http.requests.borrow().len(), 1);
    }
}

#[test]
fn foundry_canonical_request_matches_the_coordinator() {
    let spec = models::spec(&json!({
        "model":FLASH,
        "prompt":"青い折り鶴",
        "parameters":{"width":2048,"height":1024},
    }))
    .unwrap();
    assert_eq!(
        everyframe_miner::protocol::digest(&spec).unwrap(),
        "5869fc9c3c44275d9c2e93a8b5a568603a1be80dc8ba8ed2b95fc6f6a3699f2c"
    );
}
