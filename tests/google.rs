use everyframe_miner::{
    Error, Result, google, models,
    network::{Http, Request},
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/google-wire.json")).unwrap()
}

#[test]
fn google_contracts_match_coordinator_wire_fixtures_and_reject_extra_options() {
    let rows = fixtures();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let spec = &row["spec"];
        let m = models::info(spec["model"].as_str().unwrap()).unwrap();
        models::check(spec).unwrap();
        assert_eq!(protocol::digest(spec).unwrap(), row["hash"]);
        assert_eq!(google::body(spec, m).unwrap(), row["body"]);
        assert_eq!(row["body"]["background"], true);
        assert_eq!(row["body"]["store"], true);
        assert!(row["body"].get("tools").is_none());
        for key in ["tools", "endpoint", "previous_interaction_id"] {
            let mut changed = spec.clone();
            changed["input"][key] = json!("untrusted");
            assert!(models::check(&changed).is_err());
        }
        let mut changed = spec.clone();
        changed["input"]["generation_config"]["max_output_tokens"] = json!(
            everyframe_miner::google_settings::CAPS[spec["model"].as_str().unwrap()]["outputTokens"]
                .as_i64()
                .unwrap() + 1
        );
        assert!(models::check(&changed).is_err());
    }
}

struct Api {
    spec: Value,
    calls: RefCell<Vec<Request>>,
    fail: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert!(r.url.starts_with(google::ORIGIN));
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "x-goog-api-key" && v == "synthetic-key")
        );
        let m = models::info(self.spec["model"].as_str().unwrap())?;
        let d = if r.method == "POST" {
            assert_eq!(r.url, format!("{}/v1beta/interactions", google::ORIGIN));
            self.calls.borrow_mut().push(r);
            if self.fail {
                return Err(Error("request_timeout"));
            }
            return Ok(
                json!({"id":"v1_test","status":"in_progress","model":m["apiModel"]})
                    .to_string()
                    .into_bytes(),
            );
        } else {
            json!({"id":"v1_test","status":"completed","model":m["apiModel"],"steps":[{"type":"model_output","content":[{"type":m["kind"],"mime_type":m["mime"],"data":"dGVzdA=="}]}]})
        };
        assert!(r.url.ends_with("/v1beta/interactions/v1_test"));
        self.calls.borrow_mut().push(r);
        Ok(d.to_string().into_bytes())
    }
}
#[test]
fn google_restarts_resume_reads_and_bind_the_same_model_and_input() {
    for row in fixtures() {
        let spec = &row["spec"];
        let api = Api {
            spec: spec.clone(),
            calls: RefCell::new(vec![]),
            fail: false,
        };
        let mut router = Router::new(&api, json!({"GEMINI_API_KEY":"synthetic-key"}));
        let id = router
            .submit_before(spec, everyframe_miner::now() + 60000)
            .unwrap();
        assert!(id.len() <= 150);
        let mut restarted = Router::new(&api, json!({"GEMINI_API_KEY":"synthetic-key"}));
        assert_eq!(restarted.poll(&id, spec).unwrap(), "COMPLETED");
        let out = restarted.result(&id, spec).unwrap();
        assert_eq!(out.media, b"test");
        assert_eq!(out.response_hash.len(), 64);
        let mut changed = spec.clone();
        changed["input"]["prompt"] = json!("another input");
        let count = api.calls.borrow().len();
        assert!(restarted.poll(&id, &changed).is_err());
        assert!(restarted.poll("google_ix_../secret", spec).is_err());
        assert_eq!(count, api.calls.borrow().len());
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
    }
}
#[test]
fn ambiguous_google_submission_is_never_replayed() {
    let spec = &fixtures()[0]["spec"];
    let api = Api {
        spec: spec.clone(),
        calls: RefCell::new(vec![]),
        fail: true,
    };
    let mut router = Router::new(&api, json!({"GEMINI_API_KEY":"synthetic-key"}));
    assert_eq!(
        router.submit_before(spec, 0).unwrap_err().0,
        "submission_deadline_elapsed"
    );
    assert!(api.calls.borrow().is_empty());
    let e = router
        .submit_before(spec, everyframe_miner::now() + 60000)
        .unwrap_err();
    assert_eq!(e.0, "request_timeout");
    assert_eq!(everyframe_miner::submission::rejected(e.0), None);
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn google_outputs_and_downloads_fail_closed() {
    let row = &fixtures()[1];
    let m = models::info(row["spec"]["model"].as_str().unwrap()).unwrap();
    let block = json!({"type":"image","mime_type":"image/jpeg","data":"dGVzdA=="});
    let mut d = json!({"id":"v1_test","model":m["apiModel"],"status":"completed","steps":[{"type":"user_input","content":[block.clone()]},{"type":"model_output","content":[block.clone()]}]});
    assert_eq!(
        google::inline(google::media(&d, m).unwrap()).unwrap(),
        b"test"
    );
    d["steps"][1]["content"]
        .as_array_mut()
        .unwrap()
        .push(block.clone());
    assert!(google::media(&d, m).is_err());
    for uri in [
        "https://evil.invalid/out",
        "https://generativelanguage.googleapis.com/admin",
        "https://generativelanguage.googleapis.com/v1beta/files/x:download?alt=media&key=leak",
    ] {
        assert!(google::download_url(&json!({"uri":uri})).is_err());
    }
    assert!(google::download_url(&json!({"uri":"https://generativelanguage.googleapis.com/v1beta/files/x:download?alt=media"})).is_ok());
    assert!(google::inline(&json!({"data":"not base64!!!"})).is_err());
    for state in [
        "failed",
        "cancelled",
        "incomplete",
        "requires_action",
        "requires_continuation",
    ] {
        assert_eq!(google::state(&json!({"status":state})).unwrap(), "FAILED");
    }
    assert!(google::state(&json!({"status":"invented"})).is_err());
    assert!(google::interaction_id(&d, m, Some("other")).is_err());
}

#[test]
fn google_catalog_is_read_only_paginated_and_account_scoped() {
    struct Catalog(RefCell<usize>);
    impl Http for Catalog {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.method, "GET");
            assert!(r.body.is_none());
            assert!(r.url.starts_with(google::ORIGIN));
            *self.0.borrow_mut() += 1;
            Ok(if r.url.contains("pageToken=page2") {
                json!({"models":[{"name":"models/lyria-3.5"}]})
            } else {
                json!({"models":[{"name":"models/gemini-nano-banana-2.1"}],"nextPageToken":"page2"})
            }
            .to_string()
            .into_bytes())
        }
    }
    let api = Catalog(RefCell::new(0));
    let mut router = Router::new(&api, json!({"GEMINI_API_KEY":"synthetic-key"}));
    let rows = router.refresh_availability(1000).unwrap();
    assert_eq!(*api.0.borrow(), 2);
    assert!(
        router
            .models()
            .contains(&"direct/google/nano-banana-2.1".to_string())
    );
    assert!(
        router
            .models()
            .contains(&"direct/google/lyria-3.5".to_string())
    );
    assert!(
        !router
            .models()
            .contains(&"direct/google/gemini-omni-flash".to_string())
    );
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .any(|r| r["model"] == "direct/google/gemini-omni-flash"
                && r["status"] == "access_denied")
    );
}

#[test]
fn flexible_google_options_match_coordinator_and_provider_wire() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/google-flexible.json")).unwrap();
    assert_eq!(rows.len(), 11);
    for row in rows {
        let spec = models::spec(&row["request"]).unwrap();
        assert_eq!(spec, row["spec"]);
        assert_eq!(protocol::digest(&spec).unwrap(), row["hash"]);
        let model = models::info(spec["model"].as_str().unwrap()).unwrap();
        assert_eq!(
            google::body_with_references(&spec, model, row["uploaded"].as_array().unwrap())
                .unwrap(),
            row["body"]
        );
    }
}

#[test]
fn google_native_formats_and_provider_limits_are_not_preset_limits() {
    use everyframe_miner::google_settings::CAPS;
    for (id, caps) in CAPS.as_object().unwrap() {
        if let Some(sizes) = caps["resolutions"].as_array() {
            for size in sizes {
                for aspect in caps["aspects"].as_array().unwrap() {
                    models::spec(
                        &json!({"model":id,"prompt":"scene","resolution":size,"aspect":aspect}),
                    )
                    .unwrap();
                }
            }
        }
        if let Some(formats) = caps["formats"].as_array() {
            for mime in formats {
                models::spec(&json!({"model":id,"prompt":"scene","options":{"response_format":{"mime_type":mime}}})).unwrap();
            }
        }
        let spec = models::spec(&json!({"model":id,"prompt":"scene"})).unwrap();
        assert!(
            spec["input"]["generation_config"]
                .get("max_output_tokens")
                .is_none()
        );
        let mut invalid = spec;
        invalid["input"]["response_format"]["delivery"] = json!("external");
        assert!(models::check(&invalid).is_err());
    }
    let s = models::spec(&json!({"model":"direct/google/gemini-3.8-flash-tts","prompt":"hello","options":{"generation_config":{"structured_speech_config":[{"voice":"Puck"}]}}})).unwrap();
    assert!(
        s["input"]["generation_config"]
            .get("speech_config")
            .is_none()
    );
}

#[test]
fn google_reference_uploads_verify_bytes_before_generation() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    struct References {
        calls: RefCell<Vec<Request>>,
        tampered: bool,
    }
    impl Http for References {
        fn bytes(&self, req: Request) -> Result<Vec<u8>> {
            let result = if req.url.starts_with("https://api.everyframe.studio/") {
                assert!(req.headers.is_empty());
                if self.tampered {
                    b"tampered".to_vec()
                } else {
                    b"reference".to_vec()
                }
            } else if req.url.contains("/upload/") {
                assert!(
                    req.body
                        .as_ref()
                        .unwrap()
                        .windows(9)
                        .any(|v| v == b"reference")
                );
                assert!(
                    req.headers
                        .iter()
                        .any(|(k, v)| k == "x-goog-api-key" && v == "synthetic-key")
                );
                json!({"file":{"state":"ACTIVE","name":"files/ref1","uri":"https://generativelanguage.googleapis.com/v1beta/files/ref1","mimeType":"image/jpeg","sizeBytes":"9","sha256Hash":STANDARD.encode(Sha256::digest(b"reference"))}}).to_string().into_bytes()
            } else {
                let body: Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
                assert_eq!(body["input"][0]["content"][0]["type"], "image");
                assert_eq!(body["input"][0]["content"][1]["type"], "image");
                json!({"id":"v1_ref","status":"in_progress","model":"gemini-omni-1.1-flash"})
                    .to_string()
                    .into_bytes()
            };
            self.calls.borrow_mut().push(req);
            Ok(result)
        }
    }
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/google-flexible.json")).unwrap();
    for tampered in [false, true] {
        let http = References {
            calls: RefCell::new(vec![]),
            tampered,
        };
        let mut router = Router::new(&http, json!({"GEMINI_API_KEY":"synthetic-key"}));
        let result = router.submit_before(&rows[0]["spec"], everyframe_miner::now() + 60000);
        if tampered {
            assert_eq!(result.unwrap_err().0, "reference_integrity_mismatch");
            assert!(!http.calls.borrow().iter().any(|r| r.method == "POST"));
        } else {
            assert!(result.unwrap().starts_with("google_ix_"));
        }
    }
}
