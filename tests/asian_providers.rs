use everyframe_miner::{
    Error, Result, invitation, models, native_media,
    network::{Http, Request},
    protocol::{b64, sha},
    providers::{Provider, Router},
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";
struct Api {
    provider: &'static str,
    calls: RefCell<Vec<Request>>,
    mode: &'static str,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        let post = r.method == "POST";
        if u.host_str() == Some("api.everyframe.studio") {
            assert!(r.headers.is_empty());
            self.calls.borrow_mut().push(r);
            return Ok(if self.mode == "tamper" {
                b"tampered".to_vec()
            } else {
                PNG.to_vec()
            });
        }
        if r.url.contains("/output.") {
            assert!(r.headers.is_empty());
            assert!(r.redirect_hosts.is_empty());
            self.calls.borrow_mut().push(r);
            return Ok(PNG.to_vec());
        }
        assert_eq!(
            u.host_str(),
            Some(match self.provider {
                "alibaba" => "workspace-fixture.ap-southeast-1.maas.aliyuncs.com",
                "byteplus" => "ark.ap-southeast.bytepluses.com",
                _ => "ark.cn-beijing.volces.com",
            })
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                    && v == "Bearer fixture-secret")
        );
        let out = if post {
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert!(b["model"].is_string());
            if self.mode == "timeout" {
                self.calls.borrow_mut().push(r);
                return Err(Error("network_timeout"));
            }
            if self.mode == "billing" {
                json!({"code":"Arrearage","message":"fixture-secret"})
            } else if r.url.contains("/images/generations") {
                if self.provider == "alibaba" {
                    assert_eq!(b["response_format"], "url");
                    json!({"data":[{"url":"https://test.oss-ap-southeast-1.aliyuncs.com/output.png"}]})
                } else {
                    assert!(b.get("n").is_none());
                    json!({"data":[{"b64_json":b64(PNG)}]})
                }
            } else if self.provider == "alibaba" {
                assert!(
                    r.headers
                        .iter()
                        .any(|(k, v)| k == "X-DashScope-Async" && v == "enable")
                );
                json!({"output":{"task_id":"task_123","task_status":"PENDING"}})
            } else {
                json!({"id":"task_123"})
            }
        } else {
            let task = if self.mode == "wrong-id" {
                "other_task"
            } else {
                "task_123"
            };
            if self.provider == "alibaba" {
                json!({"output":{"task_id":task,"task_status":if self.mode=="pending" {"RUNNING"}else{"SUCCEEDED"},"video_url":"https://test.oss-ap-southeast-1.aliyuncs.com/output.mp4"}})
            } else {
                json!({"id":task,"status":if self.mode=="pending" {"running"}else{"succeeded"},"content":{"video_url":if self.mode=="evil" {"https://example.com/output.mp4"}else if self.provider=="byteplus" {"https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/output.mp4"}else{"https://test.tos-cn-beijing.volces.com/output.mp4"}}})
            }
        };
        self.calls.borrow_mut().push(r);
        Ok(out.to_string().into_bytes())
    }
}
fn fixture(provider: &'static str, mode: &'static str) -> Api {
    Api {
        provider,
        mode,
        calls: RefCell::default(),
    }
}
fn credentials() -> Value {
    json!({"DASHSCOPE_WORKSPACE_ID":"workspace-fixture","DASHSCOPE_API_KEY":"fixture-secret","BYTEPLUS_API_KEY":"fixture-secret","VOLCENGINE_API_KEY":"fixture-secret"})
}
fn ids() -> Vec<String> {
    native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["asianMedia"] == true)
        .map(|(id, _)| id.clone())
        .collect()
}
fn spec(id: &str) -> Value {
    models::spec(&json!({"model":id,"prompt":"A quiet lake"})).unwrap()
}
#[test]
fn all_ten_models_submit_once_and_return_hash_bound_outputs() {
    assert_eq!(ids().len(), 10);
    for id in ids() {
        let m = models::info(&id).unwrap();
        let p = m["provider"].as_str().unwrap();
        let p = match p {
            "alibaba" => "alibaba",
            "byteplus" => "byteplus",
            _ => "volcengine",
        };
        let api = fixture(p, "ok");
        let s = spec(&id);
        let mut r = Router::new(&api, credentials());
        let task = r.submit(&s).unwrap();
        assert_eq!(r.poll(&task, &s).unwrap(), "COMPLETED");
        let out = r.result(&task, &s).unwrap();
        let bundle: Value = serde_json::from_slice(&out.media).unwrap();
        assert_eq!(bundle["parts"].as_array().unwrap().len(), 1);
        assert_eq!(out.response_hash.len(), 64);
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
fn absent_credentials_never_advertise_new_models() {
    let api = fixture("alibaba", "ok");
    let r = Router::new(&api, json!({}));
    for id in ids() {
        assert!(!r.models().contains(&id));
    }
    assert!(api.calls.borrow().is_empty());
    assert!(invitation::PROVIDERS.contains(&("byteplus", "BYTEPLUS_API_KEY")));
}
#[test]
fn a_lost_submit_is_not_retried() {
    let api = fixture("byteplus", "timeout");
    let mut r = Router::new(&api, credentials());
    assert_eq!(
        r.submit(&spec("direct/byteplus/seedance-2.5"))
            .unwrap_err()
            .0,
        "network_timeout"
    );
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn restored_videos_poll_without_resubmission_but_images_need_the_original_response() {
    for id in ids() {
        let m = models::info(&id).unwrap();
        let p = match m["provider"].as_str().unwrap() {
            "alibaba" => "alibaba",
            "byteplus" => "byteplus",
            _ => "volcengine",
        };
        let api = fixture(p, "ok");
        let s = spec(&id);
        let task = Router::new(&api, credentials()).submit(&s).unwrap();
        let mut restarted = Router::new(&api, credentials());
        if m["kind"] == "video" {
            assert_eq!(restarted.poll(&task, &s).unwrap(), "COMPLETED");
            restarted.result(&task, &s).unwrap();
        } else {
            assert_eq!(
                restarted.poll(&task, &s).unwrap_err().0,
                "synchronous_result_unavailable"
            );
        }
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
fn task_ids_and_reference_modes_are_bound() {
    let id = "direct/byteplus/seedance-2.5";
    let s = spec(id);
    let api = fixture("byteplus", "wrong-id");
    let mut r = Router::new(&api, credentials());
    let task = r.submit(&s).unwrap();
    assert_eq!(r.poll(&task, &s).unwrap_err().0, "provider_id_mismatch");
    let mut other = s.clone();
    other["input"]["prompt"] = json!("A different lake");
    assert_eq!(
        r.poll(&task, &other).unwrap_err().0,
        "provider_ref_namespace"
    );
    let api = fixture("byteplus", "evil");
    let mut r = Router::new(&api, credentials());
    let task = r.submit(&s).unwrap();
    assert!(r.result(&task, &s).is_err());
}
#[test]
fn references_are_verified_before_paid_submit_and_consumed_in_order() {
    let image = json!({"url":"https://api.everyframe.studio/v1/attachment/att_test?t=abcdefghijklmnopqrst","sha256":sha(PNG),"bytes":PNG.len(),"mime":"image/png","role":"image","width":512,"height":512});
    for id in ids() {
        let mut input = json!({"model":id,"prompt":"Move slowly","references":[image.clone()]});
        let s = models::spec(&input).unwrap();
        let (_, body) = native_media::body(&s, &["verified-image".into()]).unwrap();
        let encoded = body.to_string();
        assert_eq!(encoded.matches("verified-image").count(), 1);
        input["references"][0]["role"] = json!("last");
        assert!(models::spec(&input).is_err());
    }
    let api = fixture("byteplus", "tamper");
    let mut r = Router::new(&api, credentials());
    let s=models::spec(&json!({"model":"direct/byteplus/seedance-2.5","prompt":"Move slowly","references":[image]})).unwrap();
    assert_eq!(r.submit(&s).unwrap_err().0, "reference_integrity_mismatch");
    assert!(api.calls.borrow().iter().all(|r| r.method != "POST"));
}
#[test]
fn duration_limits_and_single_image_contracts_are_enforced() {
    for id in ids() {
        let mut s = spec(&id);
        if models::info(&id).unwrap()["kind"] == "video" {
            s["input"]["parameters"]["duration"] = json!(30);
            native_media::check(&s).unwrap();
            s["input"]["parameters"]["duration"] = json!(31);
        } else {
            s["input"]["parameters"]["n"] = json!(2);
        }
        assert!(native_media::check(&s).is_err());
    }
}

#[test]
fn coordinator_canonical_contracts_and_bodies_match_exactly() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/asian-providers.json")).unwrap();
    for f in fixtures.as_array().unwrap() {
        let spec = models::spec(&f["request"]).unwrap();
        assert_eq!(spec, f["spec"]);
        assert_eq!(
            everyframe_miner::protocol::digest(&spec).unwrap(),
            f["inputHash"]
        );
        let model = models::info(spec["model"].as_str().unwrap()).unwrap();
        assert_eq!(
            everyframe_miner::protocol::digest(model).unwrap(),
            f["contractHash"]
        );
        let (_, body) = native_media::body(&spec, &[]).unwrap();
        assert_eq!(body, f["body"]);
    }
}

#[test]
fn alibaba_workspace_cannot_override_the_api_origin() {
    let api = fixture("alibaba", "ok");
    for workspace in [
        "https://evil.example",
        "abc.evil.example",
        "localhost:8080",
        "../escape",
        "",
        "UPPER",
    ] {
        let mut c = credentials();
        c["DASHSCOPE_WORKSPACE_ID"] = json!(workspace);
        let mut router = Router::new(&api, c);
        assert!(
            !router
                .models()
                .contains(&"direct/alibaba/wan-3.0".to_string())
        );
        assert_eq!(
            router
                .submit(&spec("direct/alibaba/wan-3.0"))
                .unwrap_err()
                .0,
            "alibaba_workspace_required"
        );
    }
    assert!(api.calls.borrow().is_empty());
}
#[test]
fn model_listing_probe_is_read_only_and_balance_remains_unknown() {
    let api = fixture("byteplus", "ok");
    let rows = everyframe_miner::balances::collect(
        &api,
        &json!({"BYTEPLUS_API_KEY":"fixture-secret","VOLCENGINE_API_KEY":"fixture-secret","DASHSCOPE_API_KEY":"fixture-secret"}),
    );
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r["status"] == "unsupported" && r["remainingUsd"].is_null())
    );
    assert!(api.calls.borrow().is_empty());
}
