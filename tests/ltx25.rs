use everyframe_miner::{
    Error, Result, native_media,
    network::{Http, Request},
    protocol::{digest, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nltxfixture";
fn input(model: &str) -> Value {
    let m = &native_media::MODELS[model];
    let mut refs = m["exampleReferences"].clone();
    for r in refs.as_array_mut().unwrap() {
        r["bytes"] = json!(PNG.len());
        r["sha256"] = json!(sha(PNG));
    }
    native_media::spec(&json!({"model":model,"prompt":"A bird in flight","references":refs}))
        .unwrap()
}
struct Api {
    calls: RefCell<Vec<Request>>,
    provider: String,
    failure: bool,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        if u.host_str() == Some("api.everyframe.studio") {
            self.calls.borrow_mut().push(r);
            return Ok(PNG.to_vec());
        }
        if [Some("storage.googleapis.com"), Some("v3b.fal.media")].contains(&u.host_str()) {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            self.calls.borrow_mut().push(r);
            return Ok(b"fixture-mp4".to_vec());
        }
        assert_eq!(
            u.host_str(),
            Some(if self.provider == "ltx" {
                "api.ltx.io"
            } else {
                "queue.fal.run"
            })
        );
        assert!(r.headers.iter().any(|(k, v)| k == "authorization"
            && v == if self.provider == "ltx" {
                "Bearer fixture-ltx-key"
            } else {
                "Key fixture-ltx-key"
            }));
        let data = if r.method == "POST" {
            if self.failure {
                self.calls.borrow_mut().push(r);
                return Err(Error("network_timeout"));
            }
            let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            if self.provider == "ltx" {
                assert!(body["model"].as_str().unwrap().starts_with("ltx-2-5-"));
                json!({"id":"job_25"})
            } else {
                assert!(body.get("model").is_none());
                json!({"request_id":"job_25"})
            }
        } else if self.provider == "ltx" {
            json!({"id":"job_25","status":"completed","result":{"video_url":"https://storage.googleapis.com/ltx/video.mp4"}})
        } else if r.url.ends_with("/status") {
            json!({"request_id":"job_25","status":"COMPLETED"})
        } else {
            json!({"video":{"url":"https://v3b.fal.media/files/video.mp4"}})
        };
        self.calls.borrow_mut().push(r);
        Ok(serde_json::to_vec(&data).unwrap())
    }
}
#[test]
fn every_route_accepts_its_documented_example_and_binds_all_references() {
    let rows: Vec<_> = native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["ltx25"] == true)
        .collect();
    assert_eq!(rows.len(), 14);
    for (id, _) in rows {
        let spec = input(id);
        native_media::check(&spec).unwrap();
        assert!(digest(&spec).is_ok());
    }
}
#[test]
fn native_and_fal_submit_once_and_recover_by_get_after_restart() {
    for (id, m) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["ltx25"] == true && m["provider"] != "wavespeed")
    {
        let api = Api {
            calls: RefCell::default(),
            provider: m["provider"].as_str().unwrap().into(),
            failure: false,
        };
        let spec = input(id);
        let hash = digest(&spec).unwrap();
        let keys = json!({"LTX_API_KEY":"fixture-ltx-key","FAL_KEY":"fixture-ltx-key"});
        let reference = Router::new(&api, keys.clone()).submit(&spec).unwrap();
        let mut restarted = Router::new(&api, keys);
        assert_eq!(restarted.poll(&reference, &spec).unwrap(), "COMPLETED");
        let out = restarted.result(&reference, &spec).unwrap();
        assert!(!out.media.is_empty());
        assert_eq!(hash, digest(&spec).unwrap());
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
        let mut changed = spec.clone();
        changed["input"]["prompt"] = json!("Different input");
        assert!(restarted.poll(&reference, &changed).is_err());
    }
}
#[test]
fn unknown_submission_is_not_repeated() {
    let model = "studio/native/ltx/ltx-2.5-fast/text-to-video";
    let api = Api {
        calls: RefCell::default(),
        provider: "ltx".into(),
        failure: true,
    };
    let spec = input(model);
    assert!(
        Router::new(&api, json!({"LTX_API_KEY":"fixture-ltx-key"}))
            .submit(&spec)
            .is_err()
    );
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn cross_field_duration_and_reference_constraints_are_enforced() {
    let model = "studio/native/ltx/ltx-2.5-fast/text-to-video";
    let mut spec = input(model);
    spec["input"]["parameters"]["duration"] = json!(20);
    native_media::check(&spec).unwrap();
    spec["input"]["parameters"]["fps"] = json!(50);
    assert!(native_media::check(&spec).is_err());
    spec["input"]["parameters"]["duration"] = Value::Null;
    native_media::check(&spec).unwrap();
    spec["input"]["parameters"]["resolution"] = json!("3840x2160");
    native_media::check(&spec).unwrap();
    spec["input"]["parameters"]["duration"] = json!(12);
    assert!(native_media::check(&spec).is_err());
    let mut spec = input("studio/native/ltx/ltx-2.5-pro/audio-to-video");
    spec["input"]["references"][0]["durationMs"] = json!(11000);
    assert!(native_media::check(&spec).is_err());
    let mut spec = input("studio/native/ltx/ltx-2.5-fast/image-to-video");
    spec["input"]["references"][0]["role"] = json!("audio");
    assert!(native_media::check(&spec).is_err());
}
