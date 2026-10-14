use everyframe_miner::{display, invitation, offer_catalog};
use serde_json::json;

#[test]
fn unconfigured_miner_reads_a_signed_catalog_without_credentials_or_cloud() {
    use everyframe_miner::{
        Result,
        miner::Miner,
        network::{Http, Request},
        protocol::{self, Keys},
        state::State,
    };
    struct Catalog(Keys);
    impl Http for Catalog {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            let url = url::Url::parse(&r.url).unwrap();
            assert_eq!(url.path(), "/mainnet/v1/catalog");
            assert_eq!(r.method, "GET");
            assert!(r.headers.is_empty());
            let nonce = url
                .query_pairs()
                .find(|(k, _)| k == "nonce")
                .unwrap()
                .1
                .to_string();
            let trust = invitation::trust("mainnet")?;
            Ok(protocol::signed(&json!({"protocol":1,"action":"catalog","nonce":nonce,"at":everyframe_miner::now(),"chain":everyframe_miner::hotkey::chain(&trust)?,"models":[{"model":"fixture","provider":"fal","pricing":{"minerRewardMicrousd":200000}}],"note":"No guaranteed earnings."}),&self.0.signing)?.to_string().into_bytes())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let http = Catalog(Keys::default());
    let mut trust = invitation::trust("mainnet").unwrap();
    trust["publicKey"] = json!(http.0.signing_key());
    let state = State {
        directory: std::fs::canonicalize(temp.path())
            .unwrap()
            .join("no-profile"),
    };
    let miner = Miner {
        state,
        trust,
        http: &http,
    };
    let out = miner.offers().unwrap();
    assert_eq!(out["discovery"], true);
    assert!(display::render("offers", &out).contains("$0.200000"));
    assert!(!miner.state.directory.exists());
}

#[test]
fn catalog_preserves_real_bids_and_explains_candidates_without_leaking_keys() {
    let id = "direct/vivix/vivix-w1";
    let status = json!({"minerId":"miner1","modelCatalogVersion":"test-v1",
    "offers":[{"model":"old-fal","offered":true,"discountBp":1000,"pricingCurrent":true}],
    "modelCatalog":[
        {"model":"old-fal","provider":"fal","offerable":true,"blockers":[]},
        {"model":id,"provider":"vivix","authorized":false,"offerable":false,"blockers":["worker_release_required"]},
        {"model":id,"provider":"vivix","offerable":true},
        {"provider":"vivix"}
    ]});
    let result = offer_catalog::offers(
        &status,
        &json!({}),
        &json!({"VIVIX_API_KEY":"do-not-leak-this-private-key"}),
    );
    assert_eq!(result["offers"].as_array().unwrap().len(), 2);
    assert_eq!(result["offers"][0]["discountBp"], 1000);
    assert_eq!(result["offers"][0]["offered"], true);
    assert_eq!(result["offers"][1]["localKeyConfigured"], true);
    assert_eq!(result["offers"][1]["releaseAllowsKey"], false);
    assert_eq!(result["offers"][1]["offered"], false);
    let output = display::render("offers", &result);
    assert!(output.contains("Worker release required"));
    assert!(output.contains("Unavailable"));
    assert!(output.contains("PROVIDER"));
    assert!(!output.contains("do-not-leak"));
    assert!(!result.to_string().contains("do-not-leak"));
}

#[test]
fn old_coordinator_and_legacy_offers_remain_supported() {
    let status = json!({"minerId":"miner1","offers":[{"model":"legacy","offered":true,"discountBp":1000,"pricingCurrent":true}]});
    let result = offer_catalog::offers(&status, &json!({}), &json!({}));
    assert_eq!(result["catalogAvailable"], false);
    assert_eq!(result["offers"], status["offers"]);
    let mut with_catalog = status.clone();
    with_catalog["modelCatalog"] = json!([]);
    assert_eq!(
        offer_catalog::offers(&with_catalog, &json!({}), &json!({}))["offers"],
        status["offers"]
    );
}

#[test]
fn readiness_is_not_inferred_from_local_credentials() {
    for (code, reason) in [
        ("integration_required", "Integration pending"),
        ("pricing_required", "Pricing required"),
        ("worker_release_required", "Worker release required"),
        ("worker_offline", "Worker offline"),
        ("worker_capabilities_unknown", "Worker check required"),
        ("worker_provider_required", "Enable provider on worker"),
    ] {
        assert_eq!(
            offer_catalog::readiness(&json!({"offerable":true,"blockers":[code]})),
            reason
        );
    }
    assert_eq!(
        offer_catalog::readiness(
            &json!({"offerable":true,"localKeyConfigured":false,"blockers":[]})
        ),
        "Available"
    );
}

#[test]
fn doctor_distinguishes_release_permission_from_api_key_validation() {
    let detail = invitation::provider_check_detail(false);
    assert!(detail.contains("signed deployment does not permit"));
    assert!(detail.contains("not a rejected API key"));
    assert!(detail.contains("Do not redeploy"));
    assert!(invitation::provider_check_detail(true).contains("does not verify the key"));
}
