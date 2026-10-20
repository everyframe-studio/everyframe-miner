use everyframe_miner::{cli, cli_errors, display, job_history};
use serde_json::{Value, json};

fn args(words: &[&str]) -> everyframe_miner::Result<cli::Args> {
    cli::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn terminal_history_filters_are_bounded_and_read_only() {
    let parsed = args(&[
        "jobs",
        "--status",
        "failed",
        "--limit",
        "20",
        "--model",
        "direct/vivix/vivix-w1",
        "--cursor",
        "safe_cursor-1",
    ])
    .unwrap();
    cli::validate_args(&parsed).unwrap();
    assert_eq!(
        job_history::query(&parsed).unwrap(),
        json!({"status":"failed","limit":"20","model":"direct/vivix/vivix-w1","cursor":"safe_cursor-1"})
    );
    for words in [
        vec!["jobs", "--limit", "0"],
        vec!["jobs", "--limit", "101"],
        vec!["jobs", "--limit", "2.5"],
        vec!["jobs", "--limit", "02"],
        vec!["jobs", "--status", "__proto__"],
        vec!["jobs", "--cursor", "bad value"],
        vec!["jobs", "--summary", "--status", "all"],
    ] {
        let parsed = args(&words).unwrap();
        assert!(cli::validate_args(&parsed).is_err(), "{words:?}");
    }
    assert!(args(&["resume", "--limit", "5"]).is_err());
    assert!(args(&["jobs", "--withdraw"]).is_err());
    assert!(job_history::validate_query(&json!({"minerId":"other"})).is_err());
}

fn history() -> Value {
    json!({"version":1,"currency":"USD","unit":"microusd","limit":20,"nextCursor":"next_cursor","filters":{"status":"failed","model":"direct/vivix/vivix-w1"},"rows":[{
        "jobId":"job-id","attemptId":"attempt-id","model":"direct/vivix/vivix-w1","name":"Vivix W1","provider":"vivix","state":"failed","reviewState":"not_applicable","verification":"not_verified","rewardEligibility":"not_applicable","recordedWorkValueMicrousd":null,"failureReason":"provider_billing_required"}]})
}

#[test]
fn history_validates_schema_and_preserves_complete_next_page_filters() {
    job_history::validate_page(&history()).unwrap();
    let mut bad = history();
    bad["rows"] = json!(vec![json!({}); 101]);
    assert!(job_history::validate_page(&bad).is_err());
    bad = history();
    bad["currency"] = json!("CREDITS");
    assert!(job_history::validate_page(&bad).is_err());
    bad = history();
    bad["nextCursor"] = json!("secret\n");
    assert!(job_history::validate_page(&bad).is_err());
    let out = display::render(
        "jobs",
        &json!({"historyAvailable":true,"history":history()}),
    );
    assert!(out.contains("provider_billing_required"));
    assert!(out.contains("Attempt: attempt-id"));
    assert!(out.contains("everycli jobs --status failed --model direct/vivix/vivix-w1 --limit 20 --cursor next_cursor"));
    assert!(
        !out.contains("$0.000000"),
        "unknown work value cannot become zero"
    );
}

#[test]
fn earnings_never_calls_work_value_a_payment_or_invents_profit() {
    let out = display::render(
        "earnings",
        &json!({"currency":"USD","unit":"microusd","summary":{"totalWorkValueMicrousd":400000,"pendingReviewMicrousd":200000,"approvedMicrousd":200000,"markedRewardEligibleMicrousd":100000,"excludedMicrousd":0},"fees":[{"state":"review","jobs":1,"amountMicrousd":200000}],"payoutsImplemented":false,"estimates":{"providerExposureMicrousd":350000,"quotedAttempts":2,"unpricedAttempts":1,"uncertainAttempts":1}}),
    );
    for text in [
        "$0.400000",
        "Pending review",
        "subset of approved",
        "Alpha received",
        "Not measured",
        "NOT a bill",
        "Wallet payments",
        "Not implemented",
        "Net profit",
        "Unavailable",
        "not prove inclusion",
    ] {
        assert!(out.contains(text), "{text}");
    }
    assert!(
        !out.contains("$0.050000"),
        "do not subtract a quote from accounting value to invent profit"
    );
    let legacy = display::render("earnings", &json!({"fees":[],"payoutsImplemented":false}));
    assert!(legacy.contains("not reported"));
    assert!(!legacy.contains("$0.000000"));
    let err = cli_errors::diagnostic(&everyframe_miner::Error("invalid_history_cursor"), None);
    assert!(err.to_string().contains("same status and model"));
}
