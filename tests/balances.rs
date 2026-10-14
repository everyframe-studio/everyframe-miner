mod common;
use everyframe_miner::{
    Error, Result, balances, cli, display,
    network::{Http, Request, safe_url},
    onboarding, protocol,
    state::State,
};
use serde_json::{Value, json};
use std::cell::RefCell;

struct Api {
    response: Value,
    fail: bool,
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        safe_url(&r.url, &r.hosts).unwrap();
        assert_eq!(r.method, "GET");
        assert!(r.redirect_hosts.is_empty());
        assert_eq!(r.timeout, 5);
        assert!(r.body.is_none());
        self.calls.borrow_mut().push(r);
        if self.fail {
            Err(Error("provider_error_including_secret"))
        } else {
            Ok(self.response.to_string().into_bytes())
        }
    }
}
fn api(v: Value) -> Api {
    Api {
        response: v,
        fail: false,
        calls: RefCell::default(),
    }
}

#[test]
fn balance_adapters_read_usd_never_infer_missing_as_zero() {
    let cases = [
        // A generation key reads fal's own balance endpoint: a bare USD number.
        ("FAL_KEY", json!(12.345678), 12.345678),
        // An admin key reads the documented billing API.
        (
            "FAL_ADMIN_KEY",
            json!({"credits":{"currency":"USD","current_balance":12.345678}}),
            12.345678,
        ),
        // BFL and Runway credits are one US cent each.
        ("BFL_API_KEY", json!({"credits":1234.5}), 12.345),
        (
            "RUNWAYML_API_SECRET",
            json!({"creditBalance":5000,"tier":{"maxMonthlyCreditSpend":100000},"usage":{"models":{}}}),
            50.0,
        ),
        (
            "OPENROUTER_API_KEY",
            json!({"data":{"total_credits":100,"total_usage":25.5}}),
            74.5,
        ),
        (
            "PHALA_CLOUD_API_KEY",
            json!({"credits":{"balance":"0","granted_balance":"16.66","is_post_paid":false}}),
            16.66,
        ),
    ];
    for (key, response, expected) in cases {
        let http = api(response);
        let rows = balances::collect(&http, &json!({key:"test-secret-key"}));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["remainingUsd"], expected);
        assert_eq!(rows[0]["status"], "ok");
        assert_eq!(http.calls.borrow().len(), 1);
        assert!(!serde_json::to_string(&rows).unwrap().contains("secret"));
        protocol::canonical(&json!(balances::reports(&rows))).unwrap();
    }
    for bad in [
        json!({}),
        json!({"credits":{"currency":"EUR","current_balance":5}}),
        json!({"credits":{"currency":"USD","current_balance":"NaN"}}),
        json!({"credits":{"currency":"USD","current_balance":true}}),
        json!({"credits":{"currency":"USD","current_balance":1e12}}),
    ] {
        let rows = balances::collect(&api(bad), &json!({"FAL_KEY":"test-secret-key"}));
        assert!(rows[0]["remainingUsd"].is_null());
        assert_eq!(rows[0]["status"], "unavailable");
    }
    for amount in [0.0, -2.5] {
        let rows = balances::collect(
            &api(json!({"credits":{"currency":"USD","current_balance":amount}})),
            &json!({"FAL_ADMIN_KEY":"test-secret-key"}),
        );
        assert_eq!(rows[0]["status"], "ok");
        assert_eq!(rows[0]["remainingUsd"], amount);
        // A locked fal account reports a negative balance; it must arrive as such, not as missing.
        let rows = balances::collect(&api(json!(amount)), &json!({"FAL_KEY":"test-secret-key"}));
        assert_eq!(rows[0]["status"], "ok");
        assert_eq!(rows[0]["remainingUsd"], amount);
    }
    // fal's balance endpoint answers a bare number; a string, an object or an absurd value is not one.
    for bad in [
        json!("12.5"),
        json!({"balance":12.5}),
        json!(null),
        json!(1e12),
    ] {
        let rows = balances::collect(&api(bad), &json!({"FAL_KEY":"test-secret-key"}));
        assert!(rows[0]["remainingUsd"].is_null());
        assert_eq!(rows[0]["status"], "unavailable");
    }
    for (key, bad) in [
        ("BFL_API_KEY", json!({})),
        ("BFL_API_KEY", json!({"credits":"lots"})),
        ("RUNWAYML_API_SECRET", json!({"tier":{}})),
        ("RUNWAYML_API_SECRET", json!({"creditBalance":true})),
    ] {
        let rows = balances::collect(&api(bad), &json!({key:"test-secret-key"}));
        assert!(rows[0]["remainingUsd"].is_null());
        assert_eq!(rows[0]["status"], "unavailable");
    }
}
#[test]
fn failures_and_unsupported_providers_are_safe_and_do_not_make_paid_requests() {
    let postpaid = balances::collect(
        &api(
            json!({"credits":{"balance":"0","granted_balance":"16.661526","is_post_paid":true,"outstanding_amount":null}}),
        ),
        &json!({"PHALA_CLOUD_API_KEY":"test-secret-key"}),
    );
    assert_eq!(postpaid[0]["remainingUsd"], 16.661526);
    let mut http = api(json!({}));
    http.fail = true;
    let rows = balances::collect(
        &http,
        &json!({"FAL_KEY":"test-secret-key","MINIMAX_API_KEY":"another-secret"}),
    );
    assert_eq!(rows[0]["status"], "unavailable");
    assert_eq!(rows[1]["status"], "unsupported");
    assert_eq!(http.calls.borrow().len(), 1);
    assert!(!serde_json::to_string(&rows).unwrap().contains("secret"));
    assert!(balances::collect(&http, &json!({})).is_empty());
}
#[test]
fn billing_key_precedence_storage_and_no_secret_in_output() {
    let http = api(json!({"credits":{"currency":"USD","current_balance":1}}));
    balances::collect(
        &http,
        &json!({"FAL_KEY":"test-generation-key","FAL_ADMIN_KEY":"test-billing-key"}),
    );
    assert_eq!(http.calls.borrow()[0].headers[0].1, "Key test-billing-key");
    let dir = common::tempdir();
    let state = State::new("mainnet", dir.path().join("profile").to_str()).unwrap();
    let out =
        onboarding::save_keys(&state, &json!({"FAL_ADMIN_KEY":"test-billing-key"}), None).unwrap();
    assert!(!out.to_string().contains("test-billing-key"));
    assert_eq!(
        onboarding::credential_key("fal-billing").unwrap(),
        "FAL_ADMIN_KEY"
    );
    assert_eq!(
        onboarding::credential_key("openrouter-billing").unwrap(),
        "OPENROUTER_MANAGEMENT_KEY"
    );
    onboarding::save_keys(&state, &json!({}), Some("fal-billing")).unwrap();
    assert!(state.read("credentials", false).unwrap()["FAL_ADMIN_KEY"].is_null());
}
#[test]
fn remote_snapshots_show_age_and_local_failures_override_old_success() {
    let now = 2_000_000;
    let remote = json!([{"provider":"fal","currency":"USD","remainingMicrousd":12_345_678,"status":"ok","checkedAt":now-900001,"raw":"secret"}]);
    let merged = balances::merge(&remote, vec![], now);
    assert_eq!(merged[0]["remainingUsd"], 12.345678);
    assert_eq!(merged[0]["stale"], true);
    assert_eq!(merged[0]["source"], "synced");
    assert!(!json!(merged).to_string().contains("secret"));
    let merged = balances::merge(
        &remote,
        vec![balances::row("fal", "unavailable", None, now)],
        now,
    );
    assert_eq!(merged[0]["source"], "local");
    assert_eq!(merged[0]["stale"], false);
    assert!(merged[0]["remainingUsd"].is_null());
    let future = json!([{"provider":"fal","currency":"USD","remainingMicrousd":1000,"status":"ok","checkedAt":now+30001}]);
    assert_eq!(balances::merge(&future, vec![], now)[0]["stale"], true);
}
#[test]
fn tables_show_discount_percent_and_preserve_json_values() {
    let offers = json!({"offers":[{"model":"minimax/h3-max-turbo/text-to-video","offered":true,"pricingCurrent":true,"discountBp":1000},{"model":"none","offered":false,"discountBp":null},{"model":"stale\u{001b}","offered":true,"pricingCurrent":false,"discountBp":125}]});
    let shown = display::render("offers", &offers);
    assert!(shown.contains("10.00%"));
    assert!(shown.contains("1.25%"));
    assert!(shown.contains("Inactive"));
    assert!(shown.contains("Stale pricing"));
    assert!(!shown.contains('\u{001b}'));
    assert_eq!(offers["offers"][0]["discountBp"], 1000);
    let shown = display::render(
        "balances",
        &json!({"balances":[{"provider":"fal","remainingUsd":0,"status":"ok","source":"local","checkedAt":everyframe_miner::now(),"stale":false},{"provider":"phala","remainingUsd":null,"status":"unavailable","source":"synced","stale":true}]}),
    );
    assert!(shown.contains("$0.00"));
    assert!(shown.contains("Stale / unavailable"));
    assert!(display::render("balances", &json!({"balances":[]})).contains("--publish"));
}
#[test]
fn balance_cli_flags_are_narrowly_scoped() {
    let args = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert!(cli::parse(&args("miner balances --publish --json")).is_ok());
    assert!(cli::parse(&args("miner offers --publish")).is_err());
    assert!(cli::parse(&args("miner balances --provider fal")).is_err());
}

#[test]
fn vivix_native_w_credits_are_not_a_credits_or_usd() {
    for available in [0.0, 0.125, 130.0, -1.0] {
        let http = api(
            json!({"code":0,"message":"private upstream message","data":{"workspace_id":"private-workspace","balances":[{"scope":"A","available":6750},{"scope":"W","available":available}]}}),
        );
        let rows = balances::collect(&http, &json!({"VIVIX_API_KEY":"test-secret-key"}));
        assert_eq!(
            http.calls.borrow()[0].url,
            "https://api.vivix.ai/v1/balance"
        );
        assert_eq!(rows[0]["status"], "ok");
        assert!(rows[0]["remainingUsd"].is_null());
        assert_eq!(
            rows[0]["nativeBalance"]["remainingMicrounits"],
            (available * 1_000_000.0) as i64
        );
        let report = balances::reports(&rows);
        protocol::canonical(&json!(report)).unwrap();
        assert!(!json!(report).to_string().contains("private"));
        assert!(!json!(report).to_string().contains("secret"));
        let merged = balances::merge(&json!(report), vec![], everyframe_miner::now());
        assert_eq!(merged[0]["nativeBalance"], rows[0]["nativeBalance"]);
        assert_eq!(merged[0]["status"], "ok");
        let shown = display::render("balances", &json!({"balances":merged}));
        assert!(shown.contains(&format!("{available:.3} W")));
        assert!(!shown.contains("$130"));
    }
    for response in [
        json!({}),
        json!({"code":10005,"data":{"balances":[{"scope":"W","available":0}]}}),
        json!({"code":0,"data":{"balances":[{"scope":"A","available":6750}]}}),
        json!({"code":0,"data":{"balances":[{"scope":"W","available":0},{"scope":"W","available":130}]}}),
        json!({"code":0,"data":{"balances":[{"scope":"W","available":"NaN"}]}}),
        json!({"code":0,"data":{"balances":[{"scope":"W","available":true}]}}),
    ] {
        let rows = balances::collect(&api(response), &json!({"VIVIX_API_KEY":"test-secret-key"}));
        assert_eq!(rows[0]["status"], "unavailable");
        assert!(rows[0].get("nativeBalance").is_none());
    }
}
#[test]
fn every_readable_provider_is_asked_at_its_own_read_only_endpoint() {
    for (key, response, url, header) in [
        (
            "FAL_KEY",
            json!(1),
            "https://rest.alpha.fal.ai/billing/user_balance",
            ("Authorization", "Key test-secret-key"),
        ),
        (
            "BFL_API_KEY",
            json!({"credits":1}),
            "https://api.bfl.ai/v1/credits",
            ("x-key", "test-secret-key"),
        ),
        (
            "RUNWAYML_API_SECRET",
            json!({"creditBalance":1}),
            "https://api.dev.runwayml.com/v1/organization",
            ("Authorization", "Bearer test-secret-key"),
        ),
    ] {
        let http = api(response);
        let rows = balances::collect(&http, &json!({key:"test-secret-key"}));
        assert_eq!(rows[0]["status"], "ok");
        let calls = http.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, url);
        assert!(
            calls[0]
                .headers
                .iter()
                .any(|(k, v)| k == header.0 && v == header.1)
        );
        if key == "RUNWAYML_API_SECRET" {
            assert!(
                calls[0]
                    .headers
                    .iter()
                    .any(|(k, v)| k == "X-Runway-Version" && v == "2024-11-06")
            );
        }
    }
}
