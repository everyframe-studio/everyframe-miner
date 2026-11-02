use everyframe_miner::{
    Result, models, native_media,
    network::{Http, Request},
    protocol::{digest, sha},
    providers::{Provider, Router},
    submission,
};
use serde_json::{Value, json};
use std::cell::RefCell;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nverified-image";
struct Api {
    model: String,
    fail: bool,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let u = url::Url::parse(&r.url).unwrap();
        let b = if u.host_str() == Some("api.everyframe.studio") {
            assert!(r.headers.is_empty());
            PNG.to_vec()
        } else if u.host_str() == Some("video-product.cdn.minimax.io") {
            assert!(r.headers.is_empty());
            b"test-mp4".to_vec()
        } else {
            assert_eq!(u.host_str(), Some("api.minimax.io"));
            assert!(
                r.headers
                    .iter()
                    .any(|(k, v)| k == "authorization" && v == "Bearer fixture-secret")
            );
            if r.method=="POST"{let body:Value=serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();assert_eq!(body["model"],self.model);assert!(body["content"][0]["text"].is_string());if body["content"].as_array().unwrap().len()>1{assert!(body["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("https://api.everyframe.studio/"));}
if self.fail{json!({"base_resp":{"status_code":1008}})}else{json!({"task_id":"task123","base_resp":{"status_code":0}})}}
  else{assert_eq!(u.path(),"/v2/query/video_generation/task123");json!({"task":{"id":"task123","model":self.model,"status":"succeeded","content":{"url":"https://video-product.cdn.minimax.io/video.mp4"}}})}.to_string().into_bytes()
        };
        self.calls.borrow_mut().push(r);
        Ok(b)
    }
}
fn asset() -> Value {
    json!({"url":format!("https://api.everyframe.studio/v1/attachment/att_image?t={}","a".repeat(32)),"sha256":sha(PNG),"bytes":PNG.len(),"mime":"image/png","role":"image","width":1024,"height":1024,"durationMs":0,"frameCount":0})
}
#[test]
fn all_four_h3_variants_submit_owned_references_and_recover_the_same_task() {
    let mut n = 0;
    for (id, m) in native_media::MODELS
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["adapter"] == "schema-minimax-video")
    {
        let api = Api {
            model: m["apiModel"].as_str().unwrap().into(),
            fail: false,
            calls: RefCell::default(),
        };
        let keys = json!({"MINIMAX_API_KEY":"fixture-secret"});
        let spec=models::spec(&json!({"model":id,"prompt":"A river","parameters":{"content":[{"type":"text"},{"type":"image_url","image_url":{"url":"@reference:0"},"role":"reference_image"}]},"references":[asset()]})).unwrap();
        digest(&spec).unwrap();
        let r = Router::new(&api, keys.clone()).submit(&spec).unwrap();
        let mut restarted = Router::new(&api, keys);
        assert_eq!(restarted.poll(&r, &spec).unwrap(), "COMPLETED");
        let result = restarted.result(&r, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&result.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], "video");
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
        n += 1;
    }
    assert_eq!(n, 4);
}
#[test]
fn admission_errors_are_distinct_from_uncertain_results_and_invalid_references_are_not_submitted() {
    let model = "studio/native/minimax/minimax-h3";
    let spec = models::spec(&json!({"model":model,"prompt":"A river"})).unwrap();
    let api = Api {
        model: "MiniMax-H3".into(),
        fail: true,
        calls: RefCell::default(),
    };
    let error = Router::new(&api, json!({"MINIMAX_API_KEY":"fixture-secret"}))
        .submit(&spec)
        .unwrap_err();
    assert_eq!(
        submission::rejected(error.0),
        Some("provider_billing_required")
    );
    assert_eq!(api.calls.borrow().len(), 1);
    assert_eq!(submission::rejected("request_timeout"), None);
    assert_eq!(submission::rejected("invalid_provider_response"), None);
    let mut small = asset();
    small["width"] = json!(1);
    assert!(models::spec(&json!({"model":model,"prompt":"A river","parameters":{"content":[{"type":"text"},{"type":"image_url","image_url":{"url":"@reference:0"},"role":"reference_image"}]},"references":[small]})).is_err());
}
