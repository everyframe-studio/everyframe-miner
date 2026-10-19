//! Discovery never changes the signed model allowlist or enables an offer.
use crate::invitation::{PROVIDERS, configured, invited_keys};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn integrated(row: &Value) -> bool {
    row["contractAvailable"] != false
        && !row["blockers"]
            .as_array()
            .is_some_and(|b| b.iter().any(|v| v == "integration_required"))
}

pub fn offers(status: &Value, invitation: &Value, credentials: &Value) -> Value {
    let existing: Vec<_> = status["offers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| integrated(row))
        .cloned()
        .collect();
    let Some(catalog) = status["modelCatalog"].as_array().filter(|c| c.len() <= 256) else {
        return json!({"minerId":status["minerId"],"routingPolicy":status["routingPolicy"],
            "offers":existing,"catalogAvailable":false});
    };
    let permitted = invited_keys(invitation);
    let mut seen = HashSet::new();
    let mut rows = vec![];
    for model in catalog {
        let Some(id) = model["model"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 150)
        else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        if !integrated(model) {
            continue;
        }
        let mut row = existing
            .iter()
            .find(|o| o["model"] == id)
            .cloned()
            .unwrap_or_else(
                || json!({"model":id,"offered":false,"discountBp":null,"pricingCurrent":false}),
            );
        for key in [
            "appId",
            "name",
            "provider",
            "kind",
            "contractAvailable",
            "pricingAvailable",
            "authorized",
            "workerAvailable",
            "offerable",
            "blockers",
            "contract",
            "providerApiId",
            "providerAvailability",
            "workerAvailability",
            "catalogGroup",
        ] {
            row[key] = model[key].clone();
        }
        if let Some((_, key)) = PROVIDERS.iter().find(|(p, _)| model["provider"] == *p) {
            row["localKeyConfigured"] = json!(configured(&credentials[key]));
            row["releaseAllowsKey"] = json!(permitted.iter().any(|p| p == key));
        }
        rows.push(row);
    }
    for row in existing {
        if row["model"]
            .as_str()
            .is_some_and(|id| seen.insert(id.to_string()))
        {
            rows.push(row);
        }
    }
    json!({"minerId":status["minerId"],"routingPolicy":status["routingPolicy"],
        "offers":rows,"catalogAvailable":true,"catalogVersion":status["modelCatalogVersion"]})
}

pub fn readiness(row: &Value) -> &'static str {
    for key in ["providerAvailability", "workerAvailability"] {
        if let Some(status) = row[key]["status"].as_str()
            && [
                "removed",
                "deprecated",
                "access_denied",
                "temporarily_unavailable",
            ]
            .contains(&status)
        {
            return crate::model_availability::label(status);
        }
    }
    if !integrated(row) {
        return "Unavailable";
    }
    let blocked = |code: &str| {
        row["blockers"]
            .as_array()
            .is_some_and(|b| b.iter().any(|v| v == code))
    };
    if blocked("pricing_required") {
        "Pricing required"
    } else if blocked("worker_release_required") {
        "Worker release required"
    } else if blocked("worker_offline") {
        "Worker offline"
    } else if blocked("worker_capabilities_unknown") {
        "Worker check required"
    } else if blocked("worker_provider_required") {
        "Enable provider on worker"
    } else if row["offerable"] == true {
        if row["workerAvailability"]["status"] == "unchecked" {
            "Available · account access not checked"
        } else {
            "Available"
        }
    } else {
        "Not reported"
    }
}
