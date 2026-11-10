use everyframe_miner::{
    Result, cloud, invitation, models,
    network::{Http, Request},
    providers::{Provider, Router},
    updater_vm,
};
use serde_json::{Value, json};
use std::cell::RefCell;

const BYTEPLUS_OUTPUT: &str =
    "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/output.mp4";

fn invitation() -> Value {
    json!({"minerId":"fixture-miner","request":{"compose_file":{
        "allowed_envs":["MINER_ID","MINER_TOKEN","DASHSCOPE_API_KEY","DASHSCOPE_WORKSPACE_ID","BYTEPLUS_API_KEY"],
        "docker_compose_file":"services:\n  worker:\n    image: ghcr.io/everyframe-studio/everyframe-miner:fixture\n",
        "pre_launch_script":invitation::PUBLIC_SCRIPT}}})
}
#[test]
fn signed_alibaba_workspace_survives_updater_capsule_and_environment_restore() {
    let source = json!({"DASHSCOPE_API_KEY":"fixture-key","DASHSCOPE_WORKSPACE_ID":"workspace-123","BYTEPLUS_API_KEY":"fixture-byteplus","UNRELATED_SECRET":"never-copy"});
    let inv = invitation();
    let capsule = updater_vm::permitted_credentials(&source, &inv);
    assert_eq!(capsule["DASHSCOPE_API_KEY"], "fixture-key");
    assert_eq!(capsule[invitation::ALIBABA_WORKSPACE_KEY], "workspace-123");
    assert!(capsule.get("UNRELATED_SECRET").is_none());
    assert_eq!(updater_vm::permitted_credentials(&capsule, &inv), capsule);
    let restored = cloud::environment(&inv, &capsule, true).unwrap();
    assert_eq!(restored["DASHSCOPE_WORKSPACE_ID"], "workspace-123");
    assert_eq!(restored["DASHSCOPE_API_KEY"], "fixture-key");
    assert_eq!(
        cloud::environment(&inv, &capsule, false).unwrap()["DASHSCOPE_WORKSPACE_ID"],
        ""
    );
}
#[test]
fn updater_workspace_is_not_exported_without_both_signed_permissions() {
    let source =
        json!({"DASHSCOPE_API_KEY":"fixture-key","DASHSCOPE_WORKSPACE_ID":"workspace-123"});
    for allowed in [
        json!(["DASHSCOPE_API_KEY"]),
        json!(["DASHSCOPE_WORKSPACE_ID"]),
        json!(["FAL_KEY"]),
    ] {
        let inv = json!({"request":{"compose_file":{"allowed_envs":allowed}}});
        assert!(
            updater_vm::permitted_credentials(&source, &inv)
                .get("DASHSCOPE_WORKSPACE_ID")
                .is_none()
        );
    }
}
struct Api {
    provider: &'static str,
    code: Option<&'static str>,
    output: &'static str,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let post = r.method == "POST";
        if post || r.url.contains("/tasks/") {
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                        && v == "Bearer fixture-key")
            );
        } else {
            assert!(
                r.headers.is_empty(),
                "downloads must never carry API credentials"
            );
            assert!(r.redirect_hosts.is_empty());
        }
        let result = if post {
            if self.provider == "alibaba" {
                json!({"output":{"task_id":"task_123","task_status":"PENDING"}})
            } else {
                json!({"id":"task_123"})
            }
        } else if r.url.contains("/tasks/") {
            if let Some(code) = self.code {
                let error = json!({"code":code,"message":"Specific refusal for fixture-key at https://private.example/ref?token=secret"});
                if self.provider == "alibaba" {
                    json!({"output":{"task_id":"task_123","task_status":"FAILED","code":error["code"],"message":error["message"]}})
                } else {
                    json!({"id":"task_123","status":"failed","error":error})
                }
            } else {
                json!({"id":"task_123","status":"succeeded","content":{"video_url":self.output}})
            }
        } else {
            self.calls.borrow_mut().push(r);
            return Ok(b"mock generated video".to_vec());
        };
        self.calls.borrow_mut().push(r);
        Ok(result.to_string().into_bytes())
    }
}
fn api(provider: &'static str, code: Option<&'static str>, output: &'static str) -> Api {
    Api {
        provider,
        code,
        output,
        calls: RefCell::default(),
    }
}
fn credentials() -> Value {
    json!({"BYTEPLUS_API_KEY":"fixture-key","VOLCENGINE_API_KEY":"fixture-key","DASHSCOPE_API_KEY":"fixture-key","DASHSCOPE_WORKSPACE_ID":"workspace-123"})
}
#[test]
fn documented_byteplus_output_host_is_downloadable_after_restart() {
    let api = api("byteplus", None, BYTEPLUS_OUTPUT);
    let spec =
        models::spec(&json!({"model":"direct/byteplus/seedance-2.5","prompt":"A boat"})).unwrap();
    let task = Router::new(&api, credentials()).submit(&spec).unwrap();
    let mut restored = Router::new(&api, credentials());
    assert_eq!(restored.poll(&task, &spec).unwrap(), "COMPLETED");
    restored.result(&task, &spec).unwrap();
    let calls = api.calls.borrow();
    assert_eq!(calls.iter().filter(|r| r.method == "POST").count(), 1);
    assert_eq!(calls.iter().filter(|r| r.url == BYTEPLUS_OUTPUT).count(), 1);
}
#[test]
fn byteplus_output_permission_does_not_allow_unreviewed_hosts() {
    for url in [
        "https://other-bucket.tos-ap-southeast-1.volces.com/output.mp4",
        "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com.attacker.example/output.mp4",
        "http://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/output.mp4",
    ] {
        let api = api("byteplus", None, url);
        let spec = models::spec(&json!({"model":"direct/byteplus/seedance-2.5","prompt":"A boat"}))
            .unwrap();
        let mut router = Router::new(&api, credentials());
        let task = router.submit(&spec).unwrap();
        assert!(router.result(&task, &spec).is_err(), "{url}");
        assert!(api.calls.borrow().iter().all(|r| r.url != url));
    }
}
#[test]
fn asynchronous_failure_keeps_known_and_unknown_sanitized_provider_details() {
    for (provider, model) in [
        ("byteplus", "direct/byteplus/seedance-2.5"),
        ("volcengine", "direct/volcengine/seedance-2.5"),
        ("alibaba", "direct/alibaba/wan-3.0"),
    ] {
        for code in ["InputImageSensitiveContentDetected", "NewProviderRule.2026"] {
            let api = api(provider, Some(code), BYTEPLUS_OUTPUT);
            let spec = models::spec(&json!({"model":model,"prompt":"A boat"})).unwrap();
            let mut router = Router::new(&api, credentials());
            let task = router.submit(&spec).unwrap();
            for error in [
                router.poll(&task, &spec).unwrap_err(),
                router.result(&task, &spec).err().unwrap(),
            ] {
                assert_eq!(
                    error.0,
                    if code == "InputImageSensitiveContentDetected" {
                        "provider_content_rejected"
                    } else {
                        "provider_generation_failed"
                    }
                );
                let detail = error.provider_error().unwrap();
                assert_eq!(detail.code.as_deref(), Some(code));
                let message = detail.message.as_deref().unwrap();
                assert!(message.contains("Specific refusal"));
                assert!(!message.contains("fixture-key"));
                assert!(!message.contains("private.example"));
                assert!(!message.contains("token=secret"));
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
}
fn reference(role: &str, duration: u64) -> Value {
    json!({"url":"https://api.everyframe.studio/v1/attachment/att_fixture?t=abcdefghijklmnopqrst","sha256":"a".repeat(64),"bytes":100,"mime":if role=="video" {"video/mp4"} else {"audio/mpeg"},"role":role,"width":1280,"height":720,"durationMs":duration})
}
#[test]
fn wan_enforces_per_clip_aggregate_and_output_duration_limits() {
    for model in ["direct/alibaba/wan-3.0", "direct/alibaba/wan-3.0-prime"] {
        for role in ["video", "audio"] {
            for duration in [1000, 15000] {
                assert!(models::spec(&json!({"model":model,"prompt":"A boat","references":[reference(role,duration)]})).is_ok());
            }
            for duration in [999, 15001, 20000] {
                assert!(models::spec(&json!({"model":model,"prompt":"A boat","references":[reference(role,duration)]})).is_err());
            }
            assert!(models::spec(&json!({"model":model,"prompt":"A boat","references":[reference(role,8000),reference(role,8000)]})).is_err());
        }
        assert!(models::spec(&json!({"model":model,"prompt":"A boat","parameters":{"duration":15},"references":[reference("video",15000)]})).is_ok());
        assert!(models::spec(&json!({"model":model,"prompt":"A boat","parameters":{"duration":16},"references":[reference("video",15000)]})).is_err());
    }
}
#[test]
fn ark_retains_its_thirty_second_reference_limit() {
    for model in [
        "direct/byteplus/seedance-2.5",
        "direct/volcengine/seedance-2.5",
    ] {
        assert!(
            models::spec(
                &json!({"model":model,"prompt":"A boat","references":[reference("video",20000)]})
            )
            .is_ok()
        );
        assert!(
            models::spec(
                &json!({"model":model,"prompt":"A boat","references":[reference("audio",30000)]})
            )
            .is_ok()
        );
    }
}

#[test]
fn wan_does_not_accept_an_unsupported_ratio_or_silently_truncated_prompt() {
    for model in ["direct/alibaba/wan-3.0", "direct/alibaba/wan-3.0-prime"] {
        assert!(
            models::spec(
                &json!({"model":model,"prompt":"A boat","parameters":{"aspect_ratio":"9:21"}})
            )
            .is_err()
        );
        assert!(models::spec(&json!({"model":model,"prompt":"a".repeat(20000)})).is_ok());
        assert!(models::spec(&json!({"model":model,"prompt":"a".repeat(20001)})).is_err());
    }
}
