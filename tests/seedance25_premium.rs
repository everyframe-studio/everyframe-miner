use everyframe_miner::{
    Error, Result, asian_media, models,
    network::{Http, Request},
    protocol,
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const MODEL: &str = "direct/byteplus/seedance-2.5-premium";
fn input(parameters: Value, references: Value) -> Value {
    json!({"model":MODEL,"prompt":"A quiet lake","parameters":parameters,"references":references})
}
fn asset(role: &str) -> Value {
    json!({"url":"https://api.everyframe.studio/v1/attachment/att_fixture?t=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sha256":"a".repeat(64),"bytes":128,"mime":if role=="audio"{"audio/wav"}else if role=="video"{"video/mp4"}else{"image/png"},"role":role,"width":1280,"height":720,"durationMs":if role=="audio"||role=="video"{3000}else{0}})
}
#[test]
fn canonical_requests_bodies_and_contract_hashes_match_coordinator() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/seedance25-premium.json")).unwrap();
    for f in fixtures.as_array().unwrap() {
        let s = models::spec(&f["request"]).unwrap();
        assert_eq!(s, f["spec"]);
        assert_eq!(protocol::digest(&s).unwrap(), f["inputHash"]);
        assert_eq!(
            protocol::digest(models::info(MODEL).unwrap()).unwrap(),
            f["contractHash"]
        );
        let urls: Vec<_> = s["input"]["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["url"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            asian_media::body(&s, models::info(MODEL).unwrap(), &urls).unwrap(),
            f["body"]
        );
    }
}
#[test]
fn finals_use_4k_and_preserve_the_separate_premium_model() {
    let s = models::spec(&input(json!({}), json!([]))).unwrap();
    let b = asian_media::body(&s, models::info(MODEL).unwrap(), &[]).unwrap();
    assert_eq!(b["model"], "dreamina-seedance-2-5-premium-260915");
    assert_eq!(b["resolution"], "4k");
    for duration in [-1, 4, 30] {
        assert!(models::spec(&input(json!({"duration":duration}), json!([]))).is_ok());
    }
    for duration in [0, 3, 31] {
        assert!(models::spec(&input(json!({"duration":duration}), json!([]))).is_err());
    }
    for resolution in ["480p", "720p", "1080p", "8k"] {
        assert!(models::spec(&input(json!({"resolution":resolution}), json!([]))).is_err());
    }
    for p in [
        json!({"draft":true}),
        json!({"endpoint":"https://evil.test"}),
        json!({"output_format":"mkv"}),
    ] {
        assert!(models::spec(&input(p, json!([]))).is_err());
    }
}
#[test]
fn all_fifty_references_audio_only_and_200mb_mov_inputs_are_validated() {
    let mut refs = Vec::new();
    for (role, count) in [("image", 30), ("video", 10), ("audio", 10)] {
        refs.extend((0..count).map(|_| asset(role)));
    }
    assert!(models::spec(&input(json!({}), json!(refs))).is_ok());
    refs.push(asset("image"));
    assert!(models::spec(&input(json!({}), json!(refs))).is_err());
    assert!(models::spec(&input(json!({}), json!([asset("audio")]))).is_ok());
    let mut mov = asset("video");
    mov["mime"] = json!("video/quicktime");
    mov["bytes"] = json!(190 * 1024 * 1024);
    assert!(models::spec(&input(json!({}), json!([mov]))).is_ok());
    let mut bad = asset("video");
    bad["durationMs"] = json!(30001);
    assert!(models::spec(&input(json!({}), json!([bad]))).is_err());
}
#[test]
fn derived_settings_and_edit_source_requirements_prevent_silent_parameter_loss() {
    for p in [
        json!({}),
        json!({"omni_reference_task_type":"edit"}),
        json!({"omni_reference_task_type":"edit","duration":-1}),
    ] {
        let mut video = asset("video");
        video["durationMs"] = json!(4000);
        let refs = if p == json!({}) {
            json!([asset("first")])
        } else {
            json!([video])
        };
        assert!(models::spec(&input(p, refs)).is_err());
    }
    assert!(
        models::spec(&input(
            json!({"aspect_ratio":"adaptive"}),
            json!([asset("first"), asset("last")])
        ))
        .is_ok()
    );
    for task in ["edit", "extend"] {
        assert!(
            models::spec(&input(
                json!({"duration":-1,"aspect_ratio":"adaptive","omni_reference_task_type":task}),
                json!([])
            ))
            .is_err()
        );
    }
}
struct Api {
    mode: &'static str,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let post = r.method == "POST";
        let is_api = r
            .url
            .starts_with("https://ark.ap-southeast.bytepluses.com/");
        if is_api {
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                        && v == "Bearer offline-fixture")
            );
        } else {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
        }
        let response = if post {
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert_eq!(b["model"], "dreamina-seedance-2-5-premium-260915");
            assert_eq!(b["resolution"], "4k");
            if self.mode == "timeout" {
                self.calls.borrow_mut().push(r);
                return Err(Error("network_timeout"));
            }
            json!({"id":"premium_task_1"}).to_string().into_bytes()
        } else if is_api {
            json!({"id":if self.mode=="wrong-id"{"other_task"}else{"premium_task_1"},"model":"dreamina-seedance-2-5-premium-260915","status":"succeeded","content":{"video_url":if self.mode=="evil"{"https://example.test/output.mp4"}else{"https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/output.mp4"}}}).to_string().into_bytes()
        } else {
            assert!(r.limit >= 64 * 1024 * 1024);
            b"offline generated video".to_vec()
        };
        self.calls.borrow_mut().push(r);
        Ok(response)
    }
}
fn api(mode: &'static str) -> Api {
    Api {
        mode,
        calls: RefCell::default(),
    }
}
#[test]
fn asynchronous_transport_restart_poll_and_credential_free_mp4_mov_delivery() {
    for format in ["mp4", "mov"] {
        let s = models::spec(&input(json!({"output_format":format}), json!([]))).unwrap();
        let api = api("success");
        let mut router = Router::new(&api, json!({"BYTEPLUS_API_KEY":"offline-fixture"}));
        let reference = router.submit(&s).unwrap();
        let mut restored = Router::new(&api, json!({"BYTEPLUS_API_KEY":"offline-fixture"}));
        assert_eq!(restored.poll(&reference, &s).unwrap(), "COMPLETED");
        let result = restored.result(&reference, &s).unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(
            bundle["parts"][0]["mime"],
            if format == "mov" {
                "video/quicktime"
            } else {
                "video/mp4"
            }
        );
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
fn uncertain_submission_is_never_repeated_and_task_ids_and_result_hosts_remain_bound() {
    let s = models::spec(&input(json!({}), json!([]))).unwrap();
    let uncertain = api("timeout");
    let mut r = Router::new(&uncertain, json!({"BYTEPLUS_API_KEY":"offline-fixture"}));
    assert_eq!(r.submit(&s).unwrap_err().0, "network_timeout");
    assert_eq!(uncertain.calls.borrow().len(), 1);
    for mode in ["wrong-id", "evil"] {
        let api = api(mode);
        let mut r = Router::new(&api, json!({"BYTEPLUS_API_KEY":"offline-fixture"}));
        let reference = r.submit(&s).unwrap();
        assert!(r.result(&reference, &s).is_err());
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
        let mut changed = s.clone();
        changed["input"]["prompt"] = json!("Changed");
        assert_eq!(
            r.poll(&reference, &changed).unwrap_err().0,
            "provider_ref_namespace"
        );
    }
}
