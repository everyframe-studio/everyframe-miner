//! everycli ui: request admission, single-use pairing, argument building and the
//! browser confirmation protocol. No network, no paid operations.
mod common;
use everyframe_miner::ui::{self, Request, Shared};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

#[test]
fn batch_and_jobs_ui_actions_use_the_same_scoped_cli_commands() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let (args, changes) = ui::action_args(
        &sh,
        "offer-batch",
        &json!({"models":"fixture/a,fixture/b","discountPct":"10"}),
    )
    .unwrap();
    assert!(changes);
    assert_eq!(args.command, "offer-batch");
    assert_eq!(args.get("--models"), Some("fixture/a,fixture/b"));
    assert!(
        ui::action_args(
            &sh,
            "offer-batch",
            &json!({"models":"fixture/a","discountPct":"10","withdraw":true})
        )
        .is_err()
    );
    let (args, changes) = ui::action_args(&sh, "jobs", &json!({})).unwrap();
    assert!(!changes);
    assert_eq!(args.command, "jobs");
    let (args,changes)=ui::action_args(&sh,"jobs",&json!({"limit":"20","status":"failed","model":"direct/vivix/vivix-w1","cursor":"safe_cursor"})).unwrap();
    assert!(!changes);
    assert_eq!(args.get("--status"), Some("failed"));
    assert_eq!(args.get("--cursor"), Some("safe_cursor"));
}

/// Answers every provider key check with a fixed result; never touches the network.
struct Answer(Result<&'static str, &'static str>);
impl everyframe_miner::network::Http for Answer {
    fn bytes(&self, _: everyframe_miner::network::Request) -> everyframe_miner::Result<Vec<u8>> {
        self.0
            .map(|b| b.as_bytes().to_vec())
            .map_err(everyframe_miner::Error)
    }
}
fn shared_with(dir: &std::path::Path, answer: Result<&'static str, &'static str>) -> Arc<Shared> {
    // A fresh child directory, created owner-only by the CLI itself.
    let profile = dir.join("profile");
    Arc::new(
        Shared::new("testnet", Some(profile.to_str().unwrap()), 47117)
            .with_http(Box::new(Answer(answer))),
    )
}
fn shared(dir: &std::path::Path) -> Arc<Shared> {
    shared_with(dir, Ok("{}"))
}
fn request(method: &str, path: &str, headers: &[(&str, &str)], body: Value) -> Request {
    let mut h: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
        .collect();
    h.entry("host".into()).or_insert("127.0.0.1:47117".into());
    Request {
        method: method.into(),
        path: path.into(),
        headers: h,
        body: if body.is_null() {
            vec![]
        } else {
            body.to_string().into_bytes()
        },
    }
}
fn json_of(r: &ui::Response) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}
const POST: &[(&str, &str)] = &[
    ("origin", "http://127.0.0.1:47117"),
    ("content-type", "application/json"),
];
fn pair(sh: &Arc<Shared>) -> String {
    let code = sh.pairing_code_for_tests();
    let r = ui::handle(
        sh,
        &request("POST", "/api/pair", POST, json!({"code":code})),
    );
    assert_eq!(r.status, 200);
    json_of(&r)["token"].as_str().unwrap().to_string()
}
fn authed(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("origin", "http://127.0.0.1:47117".into()),
        ("content-type", "application/json".into()),
        ("x-everycli-token", token.into()),
    ]
}
fn call(sh: &Arc<Shared>, method: &str, path: &str, token: &str, body: Value) -> ui::Response {
    let headers = authed(token);
    let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    ui::handle(sh, &request(method, path, &refs, body))
}

#[test]
fn rejects_foreign_hosts_origins_and_missing_tokens() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    // DNS rebinding: a page on evil.example resolving to 127.0.0.1.
    let r = ui::handle(
        &sh,
        &request(
            "GET",
            "/api/session",
            &[("host", "evil.example:47117")],
            Value::Null,
        ),
    );
    assert_eq!(r.status, 421);
    // Static assets need no token, but still need a loopback host.
    assert_eq!(
        ui::handle(&sh, &request("GET", "/", &[], Value::Null)).status,
        200
    );
    assert_eq!(
        ui::handle(&sh, &request("GET", "/api/session", &[], Value::Null)).status,
        401
    );
    let cross = request(
        "GET",
        "/api/session",
        &[("sec-fetch-site", "cross-site")],
        Value::Null,
    );
    assert_eq!(ui::handle(&sh, &cross).status, 403);
    // Pairing requires a loopback Origin and a JSON body (no form posts).
    let code = sh.pairing_code_for_tests();
    let foreign = request(
        "POST",
        "/api/pair",
        &[
            ("origin", "http://evil.example"),
            ("content-type", "application/json"),
        ],
        json!({"code":code}),
    );
    assert_eq!(ui::handle(&sh, &foreign).status, 403);
    let form = request(
        "POST",
        "/api/pair",
        &[("origin", "http://127.0.0.1:47117")],
        json!({"code":code}),
    );
    assert_eq!(ui::handle(&sh, &form).status, 403);
    // A valid token without an Origin still cannot change anything.
    let token = pair(&sh);
    let no_origin = request(
        "POST",
        "/api/run",
        &[
            ("content-type", "application/json"),
            ("x-everycli-token", &token),
        ],
        json!({"action":"status"}),
    );
    assert_eq!(ui::handle(&sh, &no_origin).status, 403);
    assert_eq!(
        call(&sh, "GET", "/api/session", "wrong", Value::Null).status,
        401
    );
    assert_eq!(
        call(&sh, "GET", "/api/session", &token, Value::Null).status,
        200
    );
}

#[test]
fn pairing_codes_work_once() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let first = sh.pairing_code_for_tests();
    pair(&sh);
    let again = ui::handle(
        &sh,
        &request("POST", "/api/pair", POST, json!({"code":first})),
    );
    assert_eq!(again.status, 403);
    assert_ne!(sh.pairing_code_for_tests(), first);
    assert!(sh.pairing_link().contains(&sh.pairing_code_for_tests()));
}

#[test]
fn actions_go_through_the_cli_parser() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let (args, changes) = ui::action_args(
        &sh,
        "offer",
        &json!({"model":"minimax/h3-max/text-to-video","discountPct":"10"}),
    )
    .unwrap();
    assert!(changes);
    assert_eq!(args.command, "offer");
    assert_eq!(args.get("--network"), Some("testnet"));
    assert_eq!(args.get("--discount-pct"), Some("10"));
    let (args, _) = ui::action_args(&sh, "stop", &json!({"drainOnly":true})).unwrap();
    assert!(args.flag("--drain-only"));
    // --yes and --json can never be supplied from the browser.
    assert_eq!(
        ui::action_args(&sh, "stop", &json!({"yes":true}))
            .err()
            .unwrap()
            .0,
        "invalid_option"
    );
    // Option-looking values are refused by the CLI parser.
    assert!(ui::action_args(&sh, "offer", &json!({"model":"--yes","discountPct":"5"})).is_err());
    // Options a command does not accept are refused as in the terminal.
    assert!(ui::action_args(&sh, "status", &json!({"model":"x"})).is_err());
    assert_eq!(
        ui::action_args(&sh, "rm", &json!({})).err().unwrap().0,
        "ui_unknown_action"
    );
    let (args, changes) = ui::action_args(&sh, "cli-update-check", &json!({})).unwrap();
    assert_eq!((args.command.as_str(), changes), ("self-update", false));
    assert!(args.flag("--check") && args.get("--network").is_none());
    let (args, _) = ui::action_args(&sh, "worker-update-check", &json!({})).unwrap();
    assert_eq!(args.command, "update");
    assert!(args.flag("--check"));
}

#[test]
fn keys_are_saved_but_never_echoed() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let token = pair(&sh);
    let secret = "fal-test-key-0123456789abcdef";
    let r = call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"fal","key":secret}),
    );
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert!(!String::from_utf8_lossy(&r.body).contains(secret));
    let o = call(&sh, "GET", "/api/overview", &token, Value::Null);
    let body = String::from_utf8_lossy(&o.body).to_string();
    assert!(!body.contains(secret));
    let v: Value = serde_json::from_str(&body).unwrap();
    let fal = v["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["provider"] == "fal")
        .unwrap();
    assert_eq!(fal["saved"], true);
    assert_eq!(v["steps"][2]["done"], true);
    assert_eq!(v["steps"][3]["done"], false);
    let short = call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"fal","key":"short"}),
    );
    assert_eq!(json_of(&short)["error"]["error"], "credential_too_short");
    let unknown = call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"nope","key":secret}),
    );
    assert_eq!(json_of(&unknown)["error"]["error"], "unknown_provider");
}

#[test]
fn questions_must_be_approved_by_exact_text() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let token = pair(&sh);
    call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"fal","key":"fal-test-key-0123456789abcdef"}),
    );
    let (args, _) = ui::action_args(&sh, "remove-api-key", &json!({"provider":"fal"})).unwrap();
    // Unapproved: the command stops at its question and nothing changes.
    let (state, result) = ui::execute(&sh, "t1", &args, &[]);
    assert_eq!(state, "confirm");
    let question = result["question"].as_str().unwrap().to_string();
    assert!(question.contains("Remove the locally saved fal key"));
    assert_eq!(result["hash"], ui::question_hash(&question));
    let saved = |sh: &Arc<Shared>| {
        let o = json_of(&call(sh, "GET", "/api/overview", &token, Value::Null));
        o["keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["provider"] == "fal" && k["saved"] == true)
    };
    assert!(saved(&sh));
    // Approving a different text does not approve this question.
    let (state, _) = ui::execute(&sh, "t2", &args, &[ui::question_hash("something else")]);
    assert_eq!(state, "confirm");
    assert!(saved(&sh));
    let (state, result) = ui::execute(&sh, "t3", &args, &[ui::question_hash(&question)]);
    assert_eq!(state, "done");
    assert_eq!(result["value"]["removed"], "FAL_KEY");
    assert!(!saved(&sh));
}

#[test]
fn wallets_list_public_addresses_only() {
    let home = common::tempdir();
    let hotkeys = home.path().join(".bittensor/wallets/miner/hotkeys");
    std::fs::create_dir_all(&hotkeys).unwrap();
    let address = "5H6rCSt4yGbUbMiKchacVfVd2yTuJZ6n5rL55jJMWsrmasfW";
    std::fs::write(
        hotkeys.join("default"),
        json!({"ss58Address":address,"secretSeed":"0x".to_string() + &"11".repeat(32)}).to_string(),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        hotkeys.join("default"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    std::fs::write(hotkeys.join("bad name!"), "{}").unwrap();
    // SAFETY: this test binary's only HOME-dependent test.
    unsafe { std::env::set_var("HOME", home.path()) };
    let w = ui::wallets();
    let text = w.to_string();
    assert!(!text.contains("secretSeed") && !text.contains(&"11".repeat(32)));
    assert_eq!(w["wallets"][0]["name"], "miner");
    assert_eq!(w["wallets"][0]["hotkeys"].as_array().unwrap().len(), 1);
    assert_eq!(w["wallets"][0]["hotkeys"][0]["address"], address);
}

#[test]
fn page_assets_stay_local_and_text_only() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    for path in ["/", "/app.js", "/app.css", "/icon.svg"] {
        let r = ui::handle(&sh, &request("GET", path, &[], Value::Null));
        assert_eq!(r.status, 200, "{path}");
        let body = String::from_utf8_lossy(&r.body);
        assert!(
            !body.contains("innerHTML") && !body.contains("eval("),
            "{path}"
        );
        // Only the help page links out (docs and GitHub); nothing is loaded from elsewhere.
        assert!(
            !body.contains("<script src=\"http") && !body.contains("@import"),
            "{path}"
        );
    }
    let js = String::from_utf8_lossy(
        &ui::handle(&sh, &request("GET", "/app.js", &[], Value::Null)).body,
    )
    .to_string();
    assert!(!js.contains("everyframe-studios"));
}

#[test]
fn overview_exposes_lifecycle_reconciliation_and_local_label() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let state = everyframe_miner::state::State::new("testnet", sh.state_dir.as_deref()).unwrap();
    state.prepare().unwrap();
    state
        .write("label", &json!({"name":"studio-worker-1"}))
        .unwrap();
    for phase in [
        "starting",
        "stopping",
        "provisioned",
        "updating",
        "stop_intent",
    ] {
        state
            .write(
                "deployment",
                &json!({"phase":phase,"appId":"11".repeat(20)}),
            )
            .unwrap();
        let o = ui::overview(&sh).unwrap();
        assert_eq!(o["deployment"]["needsReconcile"], true, "{phase}");
        assert_eq!(o["label"], "studio-worker-1");
    }
    // Deletion is confirmed by re-running delete; a deleted worker frees the deploy step.
    state
        .write(
            "deployment",
            &json!({"phase":"deleting","appId":"11".repeat(20)}),
        )
        .unwrap();
    assert_eq!(
        ui::overview(&sh).unwrap()["deployment"]["needsReconcile"],
        false
    );
    state
        .write(
            "deployment",
            &json!({"phase":"deleted","appId":"11".repeat(20)}),
        )
        .unwrap();
    let o = ui::overview(&sh).unwrap();
    assert!(o["deployment"]["appId"].is_null());
    assert_eq!(o["steps"][4]["done"], false);
    state
        .write(
            "deployment",
            &json!({"phase":"running","appId":"11".repeat(20)}),
        )
        .unwrap();
    assert_eq!(
        ui::overview(&sh).unwrap()["deployment"]["needsReconcile"],
        false
    );
    let (args, changes) = ui::action_args(&sh, "stop", &json!({"force":true})).unwrap();
    assert!(changes && args.flag("--force"));
    assert!(ui::action_args(&sh, "resume", &json!({"force":true})).is_err());
    assert!(
        ui::action_args(&sh, "register-hotkey", &json!({"hotkeySs58":"test"}))
            .unwrap()
            .1
    );
}

#[test]
fn overview_reports_registration_uid_zero_timestamp_but_not_signer_or_recovery_secrets() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let state = everyframe_miner::state::State {
        directory: dir.path().join("profile"),
    };
    state.write("registration", &json!({"registered":true,"network":"testnet","hotkey":"public-fixture","uid":0,"checkedAt":123,"hotkeyFile":"secret-signing-path"})).unwrap();
    state
        .write("profile-recovery", &json!({"secret":"never-show"}))
        .unwrap();
    let out = ui::overview(&sh).unwrap();
    assert_eq!(out["registration"]["uid"], 0);
    assert_eq!(out["registration"]["checkedAt"], 123);
    assert_eq!(out["hotkeyAttached"], true);
    assert_eq!(out["recoveryPending"], true);
    assert!(!out.to_string().contains("secret-signing-path"));
    assert!(!out.to_string().contains("never-show"));
    let (args, mutating) = ui::action_args(&sh, "reconnect", &json!({})).unwrap();
    assert_eq!(args.command, "reconnect");
    assert!(mutating);
}

fn saved_keys(sh: &Arc<Shared>, token: &str) -> Vec<String> {
    json_of(&call(sh, "GET", "/api/overview", token, Value::Null))["keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|k| k["saved"] == true)
        .map(|k| k["provider"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn rejected_keys_are_never_saved() {
    let dir = common::tempdir();
    let sh = shared_with(dir.path(), Err("http_401"));
    let token = pair(&sh);
    let r = call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"openrouter","key":"sk-or-test-0123456789abcdef"}),
    );
    assert_eq!(r.status, 400);
    assert_eq!(json_of(&r)["error"]["error"], "credential_rejected");
    assert!(!String::from_utf8_lossy(&r.body).contains("sk-or-test"));
    assert!(saved_keys(&sh, &token).is_empty());
}

#[test]
fn unreachable_checks_still_save_and_say_so() {
    let dir = common::tempdir();
    let sh = shared_with(dir.path(), Err("request_timeout"));
    let token = pair(&sh);
    let r = call(
        &sh,
        "POST",
        "/api/keys",
        &token,
        json!({"provider":"fal","key":"fal-test-key-0123456789abcdef"}),
    );
    assert_eq!(r.status, 200);
    assert_eq!(json_of(&r)["check"]["result"], "unverified");
    assert_eq!(saved_keys(&sh, &token), vec!["fal"]);
    // A saved key can be re-tested without being returned.
    let again = call(
        &sh,
        "POST",
        "/api/keys/check",
        &token,
        json!({"provider":"fal"}),
    );
    assert_eq!(json_of(&again)["check"]["result"], "unverified");
    assert!(!String::from_utf8_lossy(&again.body).contains("fal-test-key"));
    let missing = call(
        &sh,
        "POST",
        "/api/keys/check",
        &token,
        json!({"provider":"luma"}),
    );
    assert_eq!(json_of(&missing)["error"]["error"], "credential_not_saved");
}

#[test]
fn overview_has_no_authorization_expiry_and_reports_the_label() {
    let dir = common::tempdir();
    let sh = shared(dir.path());
    let token = pair(&sh);
    let (args, _) = ui::action_args(&sh, "label", &json!({"name":"studio-worker-1"})).unwrap();
    let (state, _) = ui::execute(&sh, "l1", &args, &[]);
    assert_eq!(state, "done");
    let o = json_of(&call(&sh, "GET", "/api/overview", &token, Value::Null));
    assert_eq!(o["label"], "studio-worker-1");
    assert!(o.get("delegationExpiresAt").is_none());
    let (args, _) = ui::action_args(&sh, "label", &json!({"name":"x".repeat(65)})).unwrap();
    assert_eq!(ui::execute(&sh, "l2", &args, &[]).0, "failed");
}
