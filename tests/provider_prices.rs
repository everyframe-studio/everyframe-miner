use everyframe_miner::{
    Error, Result,
    network::{Http, Request},
    provider_prices::{collect, picousd},
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
const MODEL: &str = "studio/media/fal/xai/grok-imagine-image";
struct Api {
    calls: RefCell<Vec<Request>>,
    unit: String,
    fail: Cell<bool>,
}
impl Api {
    fn new(unit: &str) -> Self {
        Self {
            calls: RefCell::default(),
            unit: unit.into(),
            fail: Cell::new(false),
        }
    }
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert_eq!(r.timeout, 5);
        assert_eq!(r.limit, 2_000_000);
        assert!(r.redirect_hosts.is_empty());
        let url = url::Url::parse(&r.url).unwrap();
        assert_eq!(url.host_str(), Some("api.fal.ai"));
        assert_eq!(
            r.headers
                .iter()
                .filter(|(k, _)| k == "authorization")
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>(),
            vec!["Key generation-key"]
        );
        assert!(
            r.headers
                .iter()
                .all(|(k, _)| ["authorization", "content-type"].contains(&k.as_str()))
        );
        let estimate = url.path().ends_with("/estimate");
        if estimate {
            assert_eq!(r.method, "POST");
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert_eq!(
                b,
                json!({"estimate_type":"unit_price","endpoints":{"xai/grok-imagine-image":{"unit_quantity":1}}})
            );
        } else {
            assert_eq!(r.method, "GET");
            assert_eq!(
                url.query_pairs().collect::<Vec<_>>(),
                vec![("endpoint_id".into(), "xai/grok-imagine-image".into())]
            );
        }
        self.calls.borrow_mut().push(r);
        if self.fail.get() {
            return Err(Error("RAW_PROVIDER_MESSAGE_WITH_SECRET"));
        }
        Ok(if estimate{json!({"estimate_type":"unit_price","total_cost":"0.0100000000001","currency":"USD"})}else{json!({"has_more":false,"prices":[{"endpoint_id":"xai/grok-imagine-image","unit_price":0,"unit":self.unit,"currency":"USD"}]})}.to_string().into_bytes())
    }
}
fn request() -> Vec<Value> {
    vec![json!({"model":MODEL,"apiModel":"xai/grok-imagine-image"})]
}
#[test]
fn exact_fixed_point_rounds_up_without_float_arithmetic() {
    for (text, want) in [
        ("0", 0),
        ("0.02", 20_000_000_000),
        ("2e-2", 20_000_000_000),
        ("0.0000000000001", 1),
        ("0.0100000000001", 10_000_000_001),
        ("1000", 1_000_000_000_000_000),
    ] {
        assert_eq!(picousd(&json!(text)).unwrap(), want);
    }
    for text in [
        "-1", "+1", "NaN", "1e99", ".1", "1.", "1001", "1.2.3", "0x10", "", "1e-99",
    ] {
        assert!(picousd(&json!(text)).is_err(), "{text}");
    }
    let exact: Value = serde_json::from_str("0.10000000000100000001").unwrap();
    assert_eq!(picousd(&exact).unwrap(), 100_000_000_002);
    assert!(picousd(&Value::Null).is_err());
    assert!(picousd(&json!(true)).is_err());
}
#[test]
fn fal_uses_normal_generation_key_and_read_only_account_estimate() {
    let api = Api::new("images");
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key","FAL_ADMIN_KEY":"NEVER-DEPLOY"}),
        "fal",
        &request(),
        123,
    );
    assert_eq!(api.calls.borrow().len(), 2);
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["unitPricePicousd"], 10_000_000_001_u64);
    assert_eq!(rows[0]["checkedAt"], 123);
    assert!(!json!(rows).to_string().contains("generation-key"));
}
#[test]
fn zero_compute_placeholder_is_blocked_without_estimate_or_generation() {
    let api = Api::new("compute seconds");
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &request(),
        123,
    );
    assert_eq!(rows[0]["status"], "unsupported");
    assert_eq!(rows[0]["code"], "automatic_offer_unit_unsupported");
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn billing_only_keys_and_unknown_plans_make_no_provider_calls() {
    let api = Api::new("images");
    let rows = collect(
        &api,
        &json!({"FAL_ADMIN_KEY":"admin"}),
        "fal",
        &request(),
        123,
    );
    assert_eq!(rows[0]["status"], "unavailable");
    assert!(api.calls.borrow().is_empty());
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &[json!({"model":MODEL,"apiModel":"https://evil.example/generate"})],
        123,
    );
    assert_eq!(rows[0]["status"], "unsupported");
    assert!(api.calls.borrow().is_empty());
}
#[test]
fn provider_failures_only_emit_fixed_codes() {
    let api = Api::new("images");
    api.fail.set(true);
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &request(),
        123,
    );
    assert_eq!(rows[0]["code"], "provider_price_unavailable");
    assert!(!json!(rows).to_string().contains("SECRET"));
}
#[test]
fn openrouter_public_catalog_preserves_all_skus_and_never_uses_management_key() {
    struct OrApi;
    impl Http for OrApi {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            assert_eq!(r.url, "https://openrouter.ai/api/v1/videos/models");
            assert_eq!(r.method, "GET");
            assert!(r.headers.is_empty());
            assert!(r.body.is_none());
            Ok(json!({"data":[{"id":"bytedance/seedance-2.5","pricing_skus":{"video_tokens":"0.0000107","video_tokens_without_audio":"0.0000107","video_tokens_with_video_input":"0.0000064"}}]}).to_string().into_bytes())
        }
    }
    let rows = collect(
        &OrApi,
        &json!({"OPENROUTER_API_KEY":"generation","OPENROUTER_MANAGEMENT_KEY":"management"}),
        "openrouter",
        &[
            json!({"model":"studio/openrouter/bytedance/seedance-2.5","apiModel":"bytedance/seedance-2.5"}),
        ],
        123,
    );
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["ratesPicousd"]["video_tokens"], 10_700_000);
    assert_eq!(
        rows[0]["ratesPicousd"]["video_tokens_with_video_input"],
        6_400_000
    );
}

const H3: &str = "studio/fal/minimax/h3-max/text-to-video";
/// fal catalog for H3: promotional text/image prices, full reference price,
/// and an optional zero placeholder for one endpoint.
struct VideoApi {
    calls: RefCell<Vec<String>>,
    zero: &'static str,
}
impl Http for VideoApi {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        let url = url::Url::parse(&r.url).unwrap();
        assert_eq!(url.host_str(), Some("api.fal.ai"));
        let price = |id: &str| {
            if id.ends_with(self.zero) {
                "0"
            } else if id.ends_with("/reference-to-video") {
                "0.05"
            } else {
                "0.035"
            }
        };
        let out = if url.path().ends_with("/estimate") {
            assert_eq!(r.method, "POST");
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            let ids: Vec<_> = b["endpoints"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            assert_eq!(ids.len(), 1);
            self.calls.borrow_mut().push(ids[0].clone());
            json!({"estimate_type":"unit_price","total_cost":price(&ids[0]),"currency":"USD"})
        } else {
            let ids: Vec<String> = url.query_pairs().map(|(_, v)| v.into_owned()).collect();
            assert_eq!(
                ids,
                [
                    "minimax/h3-max/text-to-video",
                    "minimax/h3-max/image-to-video",
                    "minimax/h3-max/reference-to-video"
                ]
            );
            json!({"has_more":false,"prices":ids.iter().map(|id|json!({"endpoint_id":id,"unit_price":0,"unit":"seconds","currency":"USD"})).collect::<Vec<_>>()})
        };
        Ok(out.to_string().into_bytes())
    }
}
fn video_request() -> Vec<Value> {
    vec![json!({"model":H3,"apiModel":"minimax/h3-max/text-to-video"})]
}
#[test]
fn video_models_price_every_endpoint_a_job_may_use() {
    let api = VideoApi {
        calls: RefCell::default(),
        zero: "none",
    };
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &video_request(),
        123,
    );
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["unit"], "seconds");
    assert_eq!(rows[0]["unitPricePicousd"], 35_000_000_000_u64);
    assert_eq!(
        rows[0]["endpoints"],
        json!({"image-to-video":{"unit":"seconds","unitPricePicousd":35_000_000_000_u64},
            "reference-to-video":{"unit":"seconds","unitPricePicousd":50_000_000_000_u64}})
    );
    assert_eq!(api.calls.borrow().len(), 3);
}
#[test]
fn a_zero_priced_endpoint_is_left_out_and_a_zero_main_price_blocks() {
    let api = VideoApi {
        calls: RefCell::default(),
        zero: "/reference-to-video",
    };
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &video_request(),
        123,
    );
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(
        rows[0]["endpoints"],
        json!({"image-to-video":{"unit":"seconds","unitPricePicousd":35_000_000_000_u64}})
    );
    let api = VideoApi {
        calls: RefCell::default(),
        zero: "/text-to-video",
    };
    let rows = collect(
        &api,
        &json!({"FAL_KEY":"generation-key"}),
        "fal",
        &video_request(),
        123,
    );
    assert_eq!(rows[0]["status"], "unavailable");
    assert_eq!(rows[0]["code"], "invalid_provider_price");
}
