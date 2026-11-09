use everyframe_miner::{
    Error, ErrorContext, Result, cli_errors,
    cloud::Cloud,
    diagnostics,
    network::{Http, Request},
    phala_error,
};
use serde_json::{Value, json};
use std::cell::RefCell;

struct Capture(RefCell<Vec<Request>>);
impl Http for Capture {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        self.0.borrow_mut().push(request);
        Ok(b"{}".to_vec())
    }
}

#[test]
fn create_omits_rejected_alias_without_changing_signed_input_or_other_calls() {
    let http = Capture(RefCell::new(Vec::new()));
    let credentials = json!({"PHALA_CLOUD_API_KEY":"test-private-key"});
    let cloud = Cloud::new(&http, &credentials).unwrap();
    let request = json!({"name":"everyframe-test","compose_file":{
        "name":"everyframe-test","gateway_enabled":false,"tproxy_enabled":true,
        "kms_enabled":true,"docker_compose_file":"services: {}"},"image":"dstack-0.5.12"});
    let original = request.clone();
    cloud
        .mutate("/cvms/provision", "POST", &request, false)
        .unwrap();
    let requests = http.0.borrow();
    assert_eq!(requests.len(), 1);
    let actual: Value = serde_json::from_slice(requests[0].body.as_ref().unwrap()).unwrap();
    let mut expected = request.clone();
    expected["compose_file"]
        .as_object_mut()
        .unwrap()
        .remove("tproxy_enabled");
    assert_eq!(actual, expected);
    assert_eq!(request, original);
    drop(requests);
    // Existing VM operations must not have their approved compose rewritten.
    cloud
        .mutate(
            "/cvms/app_test/compose_file/provision",
            "POST",
            &request,
            false,
        )
        .unwrap();
    let actual: Value = serde_json::from_slice(http.0.borrow()[1].body.as_ref().unwrap()).unwrap();
    assert_eq!(actual, original);
}

#[test]
fn create_without_explicit_gateway_is_not_silently_reinterpreted() {
    let http = Capture(RefCell::new(Vec::new()));
    let credentials = json!({"PHALA_CLOUD_API_KEY":"test-private-key"});
    let cloud = Cloud::new(&http, &credentials).unwrap();
    let request = json!({"compose_file":{"tproxy_enabled":true}});
    cloud
        .mutate("/cvms/provision", "POST", &request, false)
        .unwrap();
    let actual: Value = serde_json::from_slice(http.0.borrow()[0].body.as_ref().unwrap()).unwrap();
    assert_eq!(actual, request);
}

#[test]
fn actual_phala_422_reason_survives_mapping_and_cli_rendering() {
    let request = Request::get(
        "https://cloud-api.phala.com/api/v1/cvms/provision",
        &["cloud-api.phala.com"],
    );
    let failure = phala_error::failure(422,
        br#"{"error_code":"ERR-03-041","message":"tproxy_enabled cannot be set when creating a CVM. Use gateway_enabled instead.","details":[{"field":"compose_file.tproxy_enabled","value":null,"message":"Omit this field."}],"request_id":"request-123"}"#,
        Some("request-123, request-123"), &request);
    let failure = diagnostics::phala_error(failure);
    assert_eq!(failure.0, "phala_http_422");
    let output = cli_errors::diagnostic(&failure, None);
    assert_eq!(output["phala"]["code"], "ERR-03-041");
    assert_eq!(
        output["phala"]["fields"],
        json!(["compose_file.tproxy_enabled"])
    );
    assert_eq!(output["phala"]["requestId"], "request-123");
    assert!(
        output["message"]
            .as_str()
            .unwrap()
            .contains("Use gateway_enabled instead.")
    );
    assert!(
        everyframe_miner::display::render("error", &output)
            .contains("Use gateway_enabled instead.")
    );
}

#[test]
fn validation_fields_are_useful_but_request_echoes_and_keys_are_removed() {
    let mut request = Request::get(
        "https://cloud-api.phala.com/api/v1/cvms/provision",
        &["cloud-api.phala.com"],
    )
    .json(
        "POST",
        &json!({"encrypted_env":"private-encrypted-value", "value":"private-input-value"}),
    );
    request
        .headers
        .push(("X-API-Key".into(), "private-api-key".into()));
    let body = json!({"detail":[{"loc":["body","image"],"type":"unsupported_image",
        "msg":"Unsupported image. private-api-key private-input-value private-encrypted-value api_key=exposed-value https://private.test/?token=secret",
        "input":{"password":"DO_NOT_SHOW"},"ctx":{"error":"DO_NOT_SHOW_CONTEXT"}}]});
    let error = phala_error::failure(
        422,
        body.to_string().as_bytes(),
        Some("trace-private-api-key"),
        &request,
    );
    let shown = cli_errors::diagnostic(&diagnostics::phala_error(error), None);
    assert_eq!(shown["phala"]["fields"], json!(["body.image"]));
    assert_eq!(shown["phala"]["code"], "unsupported_image");
    assert!(shown["phala"]["requestId"].is_null());
    let text = shown.to_string();
    assert!(text.contains("Unsupported image"));
    for secret in [
        "private-api-key",
        "private-input-value",
        "private-encrypted-value",
        "exposed-value",
        "private.test",
        "DO_NOT_SHOW",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
}

#[test]
fn malformed_and_oversized_bodies_do_not_replace_http_status() {
    let request = Request::get("https://cloud-api.phala.com", &["cloud-api.phala.com"]);
    for body in [
        b"<html>private response</html>".to_vec(),
        vec![b'x'; phala_error::MAX_BODY + 1],
    ] {
        let error = phala_error::failure(502, &body, None, &request);
        assert_eq!(error.0, "http_502");
        assert!(error.1.is_none());
    }
    let error = phala_error::failure(
        502,
        &vec![b'x'; phala_error::MAX_BODY + 1],
        Some("request-123"),
        &request,
    );
    let Some(ErrorContext::Phala(detail)) = error.1 else {
        panic!("missing request ID")
    };
    assert!(detail.truncated);
    assert_eq!(detail.request_id.as_deref(), Some("request-123"));
}

#[test]
fn request_ids_cannot_bypass_credential_redaction() {
    let request = Request::get("https://cloud-api.phala.com", &["cloud-api.phala.com"]);
    for secret in [
        "sk-testprivatecredential123456789",
        "AIzaTestPrivateCredential1234567890",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJwcml2YXRlIn0.signature",
    ] {
        let body = json!({"message":"Invalid request", "request_id":secret});
        for header in [None, Some(secret)] {
            let error = phala_error::failure(422, body.to_string().as_bytes(), header, &request);
            let output = cli_errors::diagnostic(&error, None);
            assert!(output["phala"]["requestId"].is_null());
            assert!(!output.to_string().contains(secret));
        }
    }
}

#[test]
fn diagnostic_truncation_and_overlapping_inputs_cannot_expose_credentials() {
    let secret = "private-credential-".repeat(24);
    let request = Request::get("https://cloud-api.phala.com", &["cloud-api.phala.com"]).json(
        "POST",
        &json!({"a":"private-input", "b":"private-input-sensitive-suffix"}),
    );
    let mut request = request;
    request.headers.push(("X-API-Key".into(), secret.clone()));
    let body = json!({"code":secret,"message":"private-input-sensitive-suffix"});
    let error = phala_error::failure(422, body.to_string().as_bytes(), None, &request);
    let output = cli_errors::diagnostic(&error, None).to_string();
    assert!(!output.contains("private-credential-"));
    assert!(!output.contains("sensitive-suffix"));
}

struct Rejected(RefCell<usize>);
impl Http for Rejected {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        *self.0.borrow_mut() += 1;
        Err(phala_error::failure(
            422,
            br#"{"code":"ERR-INVALID","message":"Unsupported image"}"#,
            None,
            &request,
        ))
    }
}

#[test]
fn a_cloud_rejection_is_never_retried_or_classified_as_a_generation_refusal() {
    let http = Rejected(RefCell::new(0));
    let credentials = json!({"PHALA_CLOUD_API_KEY":"test-private-key"});
    let cloud = Cloud::new(&http, &credentials).unwrap();
    let error = cloud
        .mutate("/cvms/provision", "POST", &json!({}), false)
        .unwrap_err();
    assert_eq!(error.0, "phala_http_422");
    assert_eq!(*http.0.borrow(), 1);
    assert!(matches!(error.1, Some(ErrorContext::Phala(_))));
    assert_eq!(error.refusal(), None);
    assert_eq!(
        diagnostics::phala_error(Error("request_timeout")).0,
        "phala_request_timeout"
    );
}
