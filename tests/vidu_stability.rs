use everyframe_miner::{
    Error, Result, balances, key_check, models, native_media,
    network::{Http, Request},
    protocol::{b64, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";
const MP4: &[u8] = b"\x00\x00\x00\x18ftypisom";
const MP3: &[u8] = b"ID3fixture";
const VIDU_CDN: &str = "prod-ss-vidu.s3.cn-northwest-1.amazonaws.com.cn";

#[derive(Default)]
struct Api {
    calls: RefCell<Vec<Request>>,
    mode: Cell<&'static str>,
}
impl Http for Api {
    fn bytes(&self, request: Request) -> Result<Vec<u8>> {
        let url = url::Url::parse(&request.url).unwrap();
        let mode = self.mode.get();
        self.calls.borrow_mut().push(request.clone());
        let auth = |value| {
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == value)
            );
            assert!(request.redirect_hosts.is_empty());
        };
        let response = match url.host_str().unwrap() {
            "api.everyframe.studio" => {
                assert!(request.headers.is_empty());
                return Ok(PNG.to_vec());
            }
            VIDU_CDN => {
                assert!(request.headers.is_empty());
                return Ok(MP4.to_vec());
            }
            "api.vidu.com" => {
                auth("Token fixture-vidu");
                if url.path() == "/ent/v2/credits" {
                    assert_eq!(request.method, "GET");
                    return Ok(br#"{"remaining_credits":[]}"#.to_vec());
                }
                if request.method == "POST" {
                    assert_eq!(request.hosts, vec!["api.vidu.com"]);
                    let body: Value =
                        serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
                    assert!(body["model"] == "viduq3-turbo" || body["model"] == "viduq4-preview");
                    assert_eq!(body["prompt"], "A lighthouse at dawn");
                    assert!(!body.to_string().contains("@reference"));
                    if mode == "timeout" {
                        return Err(Error("request_timeout"));
                    }
                    json!({"task_id":"vidu-123","state":"created"})
                } else {
                    assert_eq!(url.path(), "/ent/v2/tasks/vidu-123/creations");
                    let host = if mode == "foreign" {
                        "example.invalid"
                    } else {
                        VIDU_CDN
                    };
                    json!({"id":if mode=="wrong-id" {"unrelated"} else {"vidu-123"},
                        "state":if mode=="processing" {"processing"} else {"success"},
                        "creations":[{"url":format!("https://{host}/job/video.mp4")}]})
                }
            }
            "api.stability.ai" => {
                auth("Bearer fixture-stability");
                if url.path() == "/v1/user/balance" {
                    assert_eq!(request.method, "GET");
                    return Ok(br#"{"credits":123.4}"#.to_vec());
                }
                assert_eq!(request.method, "POST");
                let multipart = String::from_utf8(request.body.unwrap()).unwrap();
                assert!(multipart.contains("name=\"prompt\"\r\n\r\nA lighthouse at dawn\r\n"));
                assert!(!multipart.contains("fixture-stability"));
                if url.path() == "/v2beta/audio/stable-audio-2/text-to-audio" {
                    assert!(multipart.contains("name=\"model\"\r\n\r\nstable-audio-2.5\r\n"));
                    assert!(multipart.contains("name=\"duration\"\r\n\r\n30\r\n"));
                    return Ok(MP3.to_vec());
                }
                assert!(matches!(
                    url.path(),
                    "/v2beta/stable-image/generate/core" | "/v2beta/stable-image/generate/ultra"
                ));
                json!({"image":b64(PNG),"finish_reason":if mode=="filtered" {"CONTENT_FILTERED"} else {"SUCCESS"}})
            }
            other => panic!("unexpected host {other}"),
        };
        Ok(response.to_string().into_bytes())
    }
}

fn credentials() -> Value {
    json!({"VIDU_API_KEY":"fixture-vidu","STABILITY_API_KEY":"fixture-stability"})
}

fn input(model: &str, config: &Value) -> Value {
    let mut refs = config["exampleReferences"].clone();
    for reference in refs.as_array_mut().unwrap() {
        reference["bytes"] = json!(PNG.len());
        reference["sha256"] = json!(sha(PNG));
    }
    models::spec(&json!({"model":model,"prompt":"A lighthouse at dawn","references":refs})).unwrap()
}

#[test]
fn all_reviewed_routes_deliver_the_expected_media_without_credential_leaks() {
    let all: Value =
        serde_json::from_str(include_str!("../config/vidu-stability-models.json")).unwrap();
    for (id, model) in all.as_object().unwrap() {
        let spec = input(id, model);
        let api = Api::default();
        let mut router = Router::new(&api, credentials());
        let reference = router.submit(&spec).unwrap_or_else(|e| panic!("{id}: {e}"));
        if model["provider"] == "vidu" {
            api.mode.set("processing");
            assert_eq!(router.poll(&reference, &spec).unwrap(), "IN_PROGRESS");
            api.mode.set("");
            router = Router::new(&api, credentials()); // Recover by GET after restart.
        } else {
            let mut fresh = Router::new(&api, credentials());
            assert_eq!(
                fresh.poll(&reference, &spec).unwrap_err().0,
                "synchronous_result_unavailable"
            );
        }
        let mut wrong = spec.clone();
        wrong["input"]["prompt"] = json!("A different lighthouse");
        assert_eq!(
            router.poll(&reference, &wrong).unwrap_err().0,
            "provider_ref_namespace"
        );
        assert_eq!(router.poll(&reference, &spec).unwrap(), "COMPLETED");
        let result = router.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], model["kind"]);
        assert_eq!(bundle["parts"][0]["mime"], model["mime"]);
        let calls = api.calls.borrow();
        assert_eq!(calls.iter().filter(|c| c.method == "POST").count(), 1);
        assert_eq!(
            calls.iter().find(|c| c.method == "POST").unwrap().url,
            format!(
                "https://{}{}",
                if model["provider"] == "vidu" {
                    "api.vidu.com"
                } else {
                    "api.stability.ai"
                },
                model["apiPath"].as_str().unwrap()
            )
        );
    }
}

#[test]
fn unknown_submission_and_invalid_outputs_fail_without_replay() {
    let model = "studio/native/vidu/viduq3-turbo/text-to-video";
    let spec = input(model, &native_media::MODELS[model]);
    let api = Api::default();
    let mut router = Router::new(&api, credentials());
    api.mode.set("timeout");
    assert!(router.submit(&spec).is_err());
    assert_eq!(api.calls.borrow().len(), 1);
    let reference = native_media::reference(&spec, "vidu-123").unwrap();
    api.mode.set("wrong-id");
    assert_eq!(
        router.poll(&reference, &spec).unwrap_err().0,
        "provider_id_mismatch"
    );
    api.mode.set("foreign");
    let e = match router.result(&reference, &spec) {
        Ok(_) => panic!("unapproved CDN"),
        Err(e) => e,
    };
    assert_eq!(e.0, "unapproved_destination");
    assert_eq!(api.calls.borrow().len(), 3);

    let model = "studio/native/stability/stable-image-core";
    let spec = input(model, &native_media::MODELS[model]);
    api.mode.set("filtered");
    assert_eq!(
        router.submit(&spec).unwrap_err().0,
        "provider_content_rejected"
    );
}

#[test]
fn read_only_key_validation_and_supported_balance() {
    let api = Api::default();
    for (provider, key) in [("vidu", "fixture-vidu"), ("stability", "fixture-stability")] {
        assert_eq!(
            key_check::check(&api, provider, key).0,
            key_check::Verdict::Valid
        );
    }
    let rows = balances::collect(&api, &credentials());
    let row = rows
        .iter()
        .find(|row| row["provider"] == "stability")
        .unwrap();
    assert!((row["remainingUsd"].as_f64().unwrap() - 1.234).abs() < 1e-9);
    assert!(api.calls.borrow().iter().all(|c| c.method == "GET"));
}
