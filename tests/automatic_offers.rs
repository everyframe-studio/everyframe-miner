use everyframe_miner::{automatic_offers, cli, display};
use serde_json::json;
fn args(extra: &[&str]) -> cli::Args {
    cli::parse(
        &[&["auto-offers"][..], extra]
            .concat()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn fields() -> Vec<&'static str> {
    vec![
        "--enable",
        "--provider",
        "fal",
        "--models",
        "studio/media/fal/xai/grok-imagine-image",
        "--max-discount-pct",
        "0",
        "--max-daily-usd",
        "1.000001",
        "--max-job-usd",
        "0.001001",
    ]
}
#[test]
fn exact_explicit_limits_required_and_zero_discount_allowed() {
    let a = args(&fields());
    cli::validate_args(&a).unwrap();
    let p = automatic_offers::limits(&a).unwrap();
    assert_eq!(p["dailyCapMicrousd"], 1000001);
    assert_eq!(p["jobCapMicrousd"], 1001);
    assert_eq!(p["maxDiscountBp"], 0);
    assert_eq!(p["bufferBp"], 2000);
    cli::validate_args(&args(&[])).unwrap();
    assert!(cli::validate_args(&args(&["--enable", "--provider", "fal"])).is_err());
}
#[test]
fn invalid_caps_scope_and_conflicting_flags_fail_before_network() {
    for (flag, value) in [
        ("--max-daily-usd", "0"),
        ("--max-job-usd", "2"),
        ("--max-job-usd", "0.0000001"),
        ("--max-job-usd", "1e-3"),
        ("--max-discount-pct", "100"),
        ("--buffer-pct", "NaN"),
        ("--models", "x,x"),
        ("--provider", "google"),
    ] {
        let mut f = fields();
        if let Some(i) = f.iter().position(|s| *s == flag) {
            f[i + 1] = value;
        } else {
            f.extend([flag, value]);
        }
        assert!(cli::validate_args(&args(&f)).is_err(), "{flag} {value}");
    }
    let mut f = fields();
    f.push("--disable");
    assert!(cli::validate_args(&args(&f)).is_err());
    assert!(
        cli::validate_args(&args(&["--disable", "--provider", "fal", "--models", "x"])).is_err()
    );
}
#[test]
fn terminal_preview_reports_paused_and_costs_without_claiming_income() {
    let text = display::render(
        "auto-offers",
        &json!({"preview":{"rows":[{"model":"test","status":"paused","reason":"automatic_offer_job_limit","estimatedCostMicrousd":250000}]}}),
    );
    assert!(text.contains("$0.250000"));
    assert!(text.contains("paused"));
    assert!(text.contains("No offer was changed"));
}
