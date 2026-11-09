//! Owner-reviewed policies. Keys remain local; the attested worker supplies its
//! own price observations after opt-in. No mutation is retried on uncertainty.
use crate::{
    Error, Result,
    cli::Args,
    invitation,
    miner::{Context, Miner},
    need, now, provider_prices, s,
};
use serde_json::{Value, json};

fn decimal(text: &str, places: u32, max: u64) -> Result<u64> {
    let parts: Vec<_> = text.split('.').collect();
    need(
        parts.len() <= 2
            && !parts[0].is_empty()
            && parts
                .iter()
                .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            && parts.get(1).is_none_or(|s| s.len() <= places as usize),
        "invalid_automatic_offer_limits",
    )?;
    let scale = 10_u64.pow(places);
    let whole = parts[0]
        .parse::<u64>()
        .map_err(|_| Error("invalid_automatic_offer_limits"))?;
    let fraction = parts.get(1).map_or(Ok(0), |s| {
        s.parse::<u64>()
            .map(|v| v * 10_u64.pow(places - s.len() as u32))
            .map_err(|_| Error("invalid_automatic_offer_limits"))
    })?;
    let value = whole
        .checked_mul(scale)
        .and_then(|v| v.checked_add(fraction))
        .ok_or(Error("invalid_automatic_offer_limits"))?;
    need(value <= max, "invalid_automatic_offer_limits")?;
    Ok(value)
}
pub fn limits(args: &Args) -> Result<Value> {
    let daily = decimal(args.required("--max-daily-usd")?, 6, 1_000_000_000_000)?;
    let job = decimal(args.required("--max-job-usd")?, 6, daily)?;
    need(daily > 0 && job > 0, "invalid_automatic_offer_limits")?;
    Ok(
        json!({"maxDiscountBp":invitation::discount(args.required("--max-discount-pct")?)?,"bufferBp":decimal(args.get("--buffer-pct").unwrap_or("20"),2,100000)?,"dailyCapMicrousd":daily,"jobCapMicrousd":job}),
    )
}
pub fn models(text: &str) -> Result<Vec<&str>> {
    let out: Vec<_> = text.split(',').collect();
    let unique: std::collections::HashSet<_> = out.iter().copied().collect();
    need(
        !out.is_empty()
            && out.len() <= 64
            && unique.len() == out.len()
            && out.iter().all(|m| {
                !m.is_empty()
                    && m.len() <= 150
                    && m.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/-_.".contains(&c))
            }),
        "invalid_automatic_offer_models",
    )?;
    Ok(out)
}
pub fn validate(args: &Args) -> Result<()> {
    let selected = ["--enable", "--disable", "--preview"]
        .iter()
        .filter(|f| args.flag(f))
        .count();
    need(selected <= 1, "conflicting_automatic_offer_options")?;
    if let Some(provider) = args.get("--provider") {
        need(
            ["fal", "openrouter"].contains(&provider),
            "automatic_offer_provider_unsupported",
        )?;
    }
    if selected == 0 {
        need(
            args.options
                .keys()
                .all(|k| ["--network", "--state-dir"].contains(&k.as_str())),
            "invalid_option",
        )?;
        return Ok(());
    }
    args.required("--provider")?;
    if args.flag("--disable") {
        need(
            args.options
                .keys()
                .all(|k| ["--network", "--state-dir", "--provider"].contains(&k.as_str())),
            "invalid_option",
        )?;
    } else {
        limits(args)?;
        models(args.required("--models")?)?;
    }
    Ok(())
}
impl Miner<'_> {
    fn save_automatic_policy(
        &self,
        c: &Context,
        provider: &str,
        enabled: bool,
        revision: u64,
        policy: Value,
    ) -> Result<Value> {
        let out=self.api(c,"automatic-offers-policy",json!({"provider":provider,"enabled":enabled,"expectedPolicyRevision":revision,"policy":policy})).map_err(|e|{
            if ["coordinator_auth_rejected","automatic_offer_policy_changed","automatic_offer_pricing_unsupported","invalid_automatic_offer_limits","offer_revision_conflict","offer_pricing_changed","provider_model_unavailable","http_400","http_401","http_403","http_404","http_409","http_413","http_422","http_429"].contains(&e.0){e}else{Error("automatic_offer_result_unknown")}
        })?;
        let p = out["automaticOffers"]["providers"]
            .as_array()
            .and_then(|rows| rows.iter().find(|p| p["provider"] == provider))
            .ok_or(Error("automatic_offer_result_unknown"))?;
        need(
            p["enabled"] == enabled && p["revision"] == revision + 1,
            "automatic_offer_result_unknown",
        )?;
        if enabled {
            for key in [
                "maxDiscountBp",
                "bufferBp",
                "dailyCapMicrousd",
                "jobCapMicrousd",
            ] {
                need(
                    p["policy"][key] == policy[key],
                    "automatic_offer_result_unknown",
                )?;
            }
            let expected = policy["models"]
                .as_array()
                .ok_or(Error("invalid_coordinator_response"))?;
            let saved = p["policy"]["models"]
                .as_array()
                .ok_or(Error("automatic_offer_result_unknown"))?;
            need(
                saved.len() == expected.len()
                    && expected.iter().all(|e| {
                        saved
                            .iter()
                            .filter(|s| {
                                s["model"] == e["model"]
                                    && s["pricingHash"] == e["pricingHash"]
                                    && s["expectedRevision"] == e["expectedRevision"]
                            })
                            .count()
                            == 1
                    }),
                "automatic_offer_result_unknown",
            )?;
        }
        Ok(out)
    }
    pub fn automatic_offers(
        &self,
        args: &Args,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        validate(args)?;
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let status = self.api(&c, "automatic-offers", json!({})).map_err(|e| {
            if e.0 == "http_404" {
                Error("automatic_offers_not_supported")
            } else {
                e
            }
        })?;
        let policies = status["automaticOffers"]["providers"]
            .as_array()
            .ok_or(Error("invalid_coordinator_response"))?;
        if !args.flag("--enable") && !args.flag("--disable") && !args.flag("--preview") {
            return Ok(status);
        }
        let provider = args.required("--provider")?;
        let current = policies
            .iter()
            .find(|p| p["provider"] == provider)
            .ok_or(Error("invalid_coordinator_response"))?;
        let revision = current["revision"]
            .as_u64()
            .filter(|r| *r < 9007199254740991)
            .ok_or(Error("invalid_coordinator_response"))?;
        if args.flag("--disable") {
            if current["enabled"] == false {
                return Ok(status);
            }
            let review = crate::protocol::digest(
                &json!({"minerId":status["minerId"],"provider":provider,"revision":revision,"enabled":false}),
            )?;
            confirm(&format!(
                "Disable automatic offers for {provider} and withdraw only models still managed by this policy? Started jobs keep their agreed price. Manual offers remain under your control.\nReview ID: {review}"
            ))?;
            return self.save_automatic_policy(&c, provider, false, revision, Value::Null);
        }
        let catalog = status["catalog"]
            .as_array()
            .ok_or(Error("invalid_coordinator_response"))?;
        let mut policy = limits(args)?;
        let mut scope = vec![];
        let mut requests = vec![];
        for model in models(args.required("--models")?)? {
            let entry = catalog
                .iter()
                .find(|m| {
                    m["model"] == model && m["provider"] == provider && m["supported"] == true
                })
                .ok_or(Error("automatic_offer_pricing_unsupported"))?;
            let (known, api) = provider_prices::model_request(model)
                .ok_or(Error("automatic_offer_pricing_unsupported"))?;
            need(
                known == provider && entry["apiModel"] == api,
                "automatic_offer_pricing_unsupported",
            )?;
            scope.push(json!({"model":model,"pricingHash":entry["pricingHash"],"expectedRevision":entry["expectedRevision"]}));
            requests.push(json!({"model":model,"apiModel":api}));
        }
        policy["models"] = json!(scope);
        let prices =
            provider_prices::collect(self.http, &c.credentials, provider, &requests, now());
        let preview = self.api(
            &c,
            "automatic-offers-preview",
            json!({"provider":provider,"policy":policy,"prices":prices}),
        )?;
        if !args.flag("--enable") {
            return Ok(preview);
        }
        let rows = preview["preview"]["rows"]
            .as_array()
            .ok_or(Error("invalid_coordinator_response"))?;
        need(
            rows.len() == scope.len()
                && rows
                    .iter()
                    .all(|r| r["status"] == "ready" || r["status"] == "paused"),
            "automatic_offer_preview_blocked",
        )?;
        let review = crate::protocol::digest(
            &json!({"minerId":status["minerId"],"provider":provider,"expectedPolicyRevision":revision,"policy":policy}),
        )?;
        confirm(&format!(
            "Enable automatic offers for {provider} on these {} models?\n{}\nMaximum discount: {:.2}%; cost buffer: {:.2}%; estimated exposure limit: ${:.6} per rolling 24 hours; estimated cost limit: ${:.6} per job.\nThe worker refreshes pricing while your dashboard is closed. Stale, incomplete or excessive costs pause new work. These are estimated limits for this policy, not provider billing limits; manual offers, other account use and hosting are excluded. Work value is not cash or guaranteed profit. Disabling withdraws managed offers; manual edits remove models from automation. A compatible worker with the same provider key must report fresh prices before new work is admitted.\nReview ID: {review}",
            scope.len(),
            scope
                .iter()
                .map(|v| s(&v["model"]).unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n"),
            policy["maxDiscountBp"].as_f64().unwrap_or(0.) / 100.,
            policy["bufferBp"].as_f64().unwrap_or(0.) / 100.,
            policy["dailyCapMicrousd"].as_f64().unwrap_or(0.) / 1e6,
            policy["jobCapMicrousd"].as_f64().unwrap_or(0.) / 1e6
        ))?;
        self.save_automatic_policy(&c, provider, true, revision, policy)
    }
}
