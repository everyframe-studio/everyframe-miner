use everyframe_miner::{invitation, model_availability, models, protocol};
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn every_direct_catalog_contract_matches_the_coordinator() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/provider-catalog.json")).unwrap();
    assert_eq!(rows.len(), models::MODELS.as_object().unwrap().len());
    let mut seen = BTreeSet::new();
    for row in rows {
        let id = row["model"].as_str().unwrap();
        assert!(seen.insert(id.to_owned()), "duplicate fixture: {id}");
        let contract = models::info(id).unwrap();
        assert_eq!(contract["provider"], row["provider"], "{id}");
        let spec = models::spec(&row["request"]).unwrap();
        assert_eq!(spec, row["spec"], "{id}");
        assert_eq!(protocol::digest(&spec).unwrap(), row["hash"], "{id}");
        models::check(&spec).unwrap();
        assert!(
            invitation::PROVIDERS
                .iter()
                .any(|(p, _)| Some(*p) == row["provider"].as_str())
        );
    }
}

#[test]
fn new_google_and_direct_kling_models_are_available_without_reviving_retired_models() {
    for id in models::MODELS.as_object().unwrap().keys() {
        if id.starts_with("direct/google/") && id != "direct/google/veo-3.1"
            || id == "direct/kling/kling-3.0"
        {
            assert!(model_availability::retired(id).is_none(), "{id}");
        }
    }
    for id in [
        "direct/google/veo-3.1",
        "direct/luma/ray-2",
        "direct/luma/ray-flash-2",
        "fal-ai/sora-2/text-to-video",
    ] {
        assert!(model_availability::retired(id).is_some(), "{id}");
    }
}
