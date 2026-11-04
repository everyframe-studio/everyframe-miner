mod common;
use everyframe_miner::{
    Error, Result, cli,
    network::{Http, Request},
    state::State,
    terms, ui,
};
use serde_json::json;
use std::{cell::Cell, fs, os::unix::fs::PermissionsExt};

const ADDRESS: &str = "5HQfvwpHm2njGiF716YwjdXgeeZoEiyFLZFvFAriZaiHNKc3";
const OTHER: &str = "5G67F1Aieac3h6VJnSMo9yjggDxCtVUXFaCQnMCjaNTGG2vX";
struct Offline(Cell<usize>);
impl Http for Offline {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        self.0.set(self.0.get() + 1);
        assert!(
            r.url.contains("opentensor.ai"),
            "only a read-only chain lookup may run"
        );
        Err(Error("offline_fixture"))
    }
}
fn args(state: &State, command: &str, extra: &[&str]) -> cli::Args {
    let mut raw = vec![
        command.into(),
        "--state-dir".into(),
        state.directory.to_str().unwrap().into(),
    ];
    raw.extend(extra.iter().map(|s| s.to_string()));
    cli::parse(&raw).unwrap()
}
fn state(dir: &std::path::Path) -> State {
    State {
        directory: dir.join("profile"),
    }
}
fn key(dir: &std::path::Path) -> std::path::PathBuf {
    let key = dir.join("hotkey");
    fs::write(&key, json!({"ss58Address":ADDRESS}).to_string()).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    key
}
#[test]
fn acceptance_is_private_durable_idempotent_and_bound_to_identity_and_document() {
    let d = common::tempdir();
    let state = state(d.path());
    state
        .write("credentials", &json!({"FAL_API_KEY":"fixture-secret"}))
        .unwrap();
    state
        .write("deployment", &json!({"phase":"stopped","appId":"fixture"}))
        .unwrap();
    assert!(!terms::accepted(&state, "mainnet", ADDRESS).unwrap());
    terms::accept(&state, "mainnet", ADDRESS, terms::VERSION).unwrap();
    let original = state.read("terms-acceptance", false).unwrap();
    terms::accept(&state, "mainnet", ADDRESS, terms::VERSION).unwrap();
    assert_eq!(state.read("terms-acceptance", false).unwrap(), original);
    assert!(terms::accepted(&state, "mainnet", ADDRESS).unwrap());
    assert!(!terms::accepted(&state, "testnet", ADDRESS).unwrap());
    assert!(!terms::accepted(&state, "mainnet", OTHER).unwrap());
    assert_eq!(
        fs::metadata(state.path("terms-acceptance").unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!original.to_string().contains("fixture-secret"));
    assert_eq!(
        state.read("credentials", false).unwrap()["FAL_API_KEY"],
        "fixture-secret"
    );
    assert_eq!(state.read("deployment", false).unwrap()["phase"], "stopped");
    for field in [
        "termsVersion",
        "documentSha256",
        "hotkey",
        "network",
        "acceptedAt",
    ] {
        let mut changed = original.clone();
        changed["acceptances"][0][field] = json!("changed");
        state.write("terms-acceptance", &changed).unwrap();
        assert!(
            !terms::accepted(&state, "mainnet", ADDRESS).unwrap(),
            "{field}"
        );
    }
    assert!(terms::TEXT.contains(&format!("Version: {}", terms::VERSION)));
    assert!(terms::TEXT.contains("Contact each provider directly"));
    assert!(terms::TEXT.contains("negotiate provider discounts on your behalf"));
}
#[test]
fn terms_are_readable_without_profile_wallet_or_transport() {
    let d = common::tempdir();
    let state = state(d.path());
    let http = Offline(Cell::new(0));
    let out = cli::run_with(
        &args(&state, "terms", &["--json"]),
        &http,
        &|_| panic!("read must not prompt"),
        &|_| {},
    )
    .unwrap();
    assert_eq!(out["termsVersion"], terms::VERSION);
    assert_eq!(out["accepted"], false);
    assert_eq!(out["text"], terms::TEXT);
    assert!(out["hotkey"].is_null());
    assert!(!state.directory.exists());
    assert_eq!(http.0.get(), 0);
}
#[test]
fn generic_yes_and_json_never_accept_terms_or_contact_enrollment() {
    let d = common::tempdir();
    let state = state(d.path());
    let key = key(d.path());
    for flags in [&["--yes"][..], &["--json"][..], &["--yes", "--json"][..]] {
        let http = Offline(Cell::new(0));
        let mut extra = vec!["--hotkey-file", key.to_str().unwrap()];
        extra.extend(flags);
        let e = cli::run_with(
            &args(&state, "init", &extra),
            &http,
            &|_| panic!("generic flags cannot approve"),
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(e.0, "terms_acceptance_required");
        assert_eq!(http.0.get(), 0);
        assert!(state.read("terms-acceptance", true).unwrap().is_null());
        assert!(state.read("config", true).unwrap().is_null());
    }
}
#[test]
fn declined_interactive_terms_stop_before_enrollment_and_save_nothing() {
    let d = common::tempdir();
    let state = state(d.path());
    let key = key(d.path());
    let http = Offline(Cell::new(0));
    let e = cli::run_with(
        &args(&state, "init", &["--hotkey-file", key.to_str().unwrap()]),
        &http,
        &|question| {
            assert!(question.contains(terms::TEXT));
            assert!(question.contains(ADDRESS));
            assert!(question.contains("negotiating provider discounts directly"));
            Err(Error("cancelled"))
        },
        &|_| {},
    )
    .unwrap_err();
    assert_eq!(e.0, "cancelled");
    assert_eq!(http.0.get(), 0);
    assert!(state.read("terms-acceptance", true).unwrap().is_null());
}
#[test]
fn explicit_version_allows_enrollment_preflight_and_wrong_versions_do_not() {
    let d = common::tempdir();
    let state = state(d.path());
    let key = key(d.path());
    let http = Offline(Cell::new(0));
    for version in ["old", terms::VERSION] {
        let e = cli::run_with(
            &args(
                &state,
                "init",
                &[
                    "--hotkey-file",
                    key.to_str().unwrap(),
                    "--accept-terms",
                    version,
                    "--json",
                ],
            ),
            &http,
            &|_| panic!("explicit approval"),
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(
            e.0,
            if version == terms::VERSION {
                "offline_fixture"
            } else {
                "terms_version_changed"
            }
        );
    }
    assert_eq!(http.0.get(), 1);
    assert!(terms::accepted(&state, "mainnet", ADDRESS).unwrap());
    let e = cli::run_with(
        &args(
            &state,
            "init",
            &["--hotkey-file", key.to_str().unwrap(), "--yes"],
        ),
        &http,
        &|_| panic!("receipt already saved"),
        &|_| {},
    )
    .unwrap_err();
    assert_eq!(e.0, "offline_fixture");
    assert_eq!(http.0.get(), 2);
    assert!(state.read("config", true).unwrap().is_null());
}
#[test]
fn ui_acceptance_is_a_mutation_and_pins_the_reviewed_terms_and_hotkey() {
    let d = common::tempdir();
    let state = state(d.path());
    state
        .write(
            "registration",
            &json!({"registered":true,"network":"mainnet","hotkey":ADDRESS}),
        )
        .unwrap();
    let sh = ui::Shared::new("mainnet", Some(state.directory.to_str().unwrap()), 47117);
    let (_, changes) = ui::action_args(&sh, "terms", &json!({})).unwrap();
    assert!(!changes);
    assert!(ui::action_args(&sh, "terms", &json!({"acceptTerms":terms::VERSION})).is_err());
    assert!(ui::action_args(&sh, "terms-accept", &json!({"acceptTerms":terms::VERSION})).is_err());
    assert!(
        ui::action_args(
            &sh,
            "terms-accept",
            &json!({"acceptTerms":"old","termsHotkey":ADDRESS,"termsSha256":terms::hash()})
        )
        .is_err()
    );
    let params =
        json!({"acceptTerms":terms::VERSION,"termsHotkey":OTHER,"termsSha256":terms::hash()});
    let (a, changes) = ui::action_args(&sh, "terms-accept", &params).unwrap();
    assert!(changes);
    let http = Offline(Cell::new(0));
    let e = cli::run_with(&a, &http, &|_| panic!("separate explicit action"), &|_| {}).unwrap_err();
    assert_eq!(e.0, "terms_review_changed");
    assert!(state.read("terms-acceptance", true).unwrap().is_null());
    let mut params = params;
    params["termsHotkey"] = json!(ADDRESS);
    let (a, _) = ui::action_args(&sh, "terms-accept", &params).unwrap();
    let out = cli::run_with(&a, &http, &|_| panic!("explicit action"), &|_| {}).unwrap();
    assert_eq!(out["accepted"], true);
    assert_eq!(http.0.get(), 0);
    let overview = ui::overview(&sh).unwrap();
    assert_eq!(overview["terms"]["accepted"], true);
    assert_eq!(overview["steps"][1]["id"], "terms");
    assert_eq!(overview["steps"][1]["done"], true);
    assert!(state.read("config", true).unwrap().is_null());
    assert!(state.read("deployment", true).unwrap().is_null());
    assert!(state.read("credentials", true).unwrap().is_null());
}
#[test]
fn wizard_decline_happens_before_keys_enrollment_or_any_paid_action() {
    let d = common::tempdir();
    let state = state(d.path());
    let mut seen = vec![];
    let a = args(&state, "setup", &["--wallet", "fixture"]);
    let e = cli::setup_with(
        &a,
        &state,
        &|question| {
            assert!(question.contains("Accept Miner Terms"));
            Ok("no".into())
        },
        &mut |a| {
            seen.push(a.command.clone());
            Ok(match a.command.as_str() {
                "register-hotkey" => json!({"registered":true}),
                "terms" => json!({"accepted":false}),
                _ => panic!("declined terms must stop setup"),
            })
        },
    )
    .unwrap_err();
    assert_eq!(e.0, "cancelled");
    assert_eq!(seen, ["register-hotkey", "terms"]);
}

#[test]
fn stale_review_and_corrupt_receipt_fail_closed_without_replacing_history() {
    let d = common::tempdir();
    let state = state(d.path());
    state
        .write(
            "registration",
            &json!({"registered":true,"network":"mainnet","hotkey":ADDRESS}),
        )
        .unwrap();
    let http = Offline(Cell::new(0));
    let a = args(
        &state,
        "terms",
        &[
            "--accept-terms",
            terms::VERSION,
            "--terms-sha256",
            "wrong",
            "--terms-hotkey",
            ADDRESS,
        ],
    );
    assert_eq!(
        cli::run_with(&a, &http, &|_| panic!("explicit"), &|_| {})
            .unwrap_err()
            .0,
        "terms_review_changed"
    );
    assert!(state.read("terms-acceptance", true).unwrap().is_null());
    terms::accept(&state, "mainnet", ADDRESS, terms::VERSION).unwrap();
    let original = state.read("terms-acceptance", false).unwrap();
    assert_eq!(
        terms::require(&state, "mainnet", ADDRESS, Some("old"), false, &|_| Ok(()))
            .unwrap_err()
            .0,
        "terms_version_changed"
    );
    assert_eq!(state.read("terms-acceptance", false).unwrap(), original);
    let mut corrupt = original;
    corrupt["version"] = json!(999);
    state.write("terms-acceptance", &corrupt).unwrap();
    assert_eq!(
        terms::accepted(&state, "mainnet", ADDRESS).unwrap_err().0,
        "invalid_terms_record"
    );
    assert_eq!(
        terms::accept(&state, "mainnet", ADDRESS, terms::VERSION)
            .unwrap_err()
            .0,
        "invalid_terms_record"
    );
    assert_eq!(state.read("terms-acceptance", false).unwrap(), corrupt);
    assert_eq!(http.0.get(), 0);
}
