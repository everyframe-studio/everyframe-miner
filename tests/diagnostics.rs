use everyframe_miner::{
    Error, Result, balances, cli,
    cloud::Cloud,
    diagnostics, display,
    network::{Http, Request},
};
use serde_json::{Value, json};

struct Failure(&'static str);
impl Http for Failure {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert_eq!(r.method, "GET");
        Err(Error(self.0))
    }
}

#[test]
fn http_reasons_preserve_status_and_phala_scope_without_remote_text() {
    for status in [
        400, 401, 402, 403, 404, 405, 408, 409, 422, 429, 500, 502, 503, 504,
    ] {
        let e = diagnostics::http_error(status);
        assert_eq!(e.0, format!("http_{status}"));
        let d = diagnostics::value(e.0);
        assert!(
            d["message"]
                .as_str()
                .unwrap()
                .contains(&format!("HTTP {status}"))
        );
        let http = Failure(e.0);
        let cloud = Cloud::new(&http, &json!({"PHALA_CLOUD_API_KEY":"synthetic-secret"})).unwrap();
        let e = cloud.get("/instance-types").unwrap_err();
        assert_eq!(e.0, format!("phala_http_{status}"));
        let d = diagnostics::value(e.0);
        assert!(d["message"].as_str().unwrap().starts_with("Phala:"));
        assert!(!d.to_string().contains("synthetic-secret"));
    }
    for (status, code) in [
        (418, "http_client_error"),
        (501, "http_server_error"),
        (302, "http_unexpected_status"),
    ] {
        assert_eq!(diagnostics::http_error(status).0, code);
    }
    assert_eq!(
        diagnostics::value("raw-secret-token")["code"],
        "request_failed"
    );
    assert!(
        !diagnostics::value("raw-secret-token")
            .to_string()
            .contains("raw-secret")
    );
}

#[test]
fn balance_failures_have_safe_reasons_actions_and_nonzero_readiness() {
    for (key, provider, setting) in [
        ("FAL_KEY", "fal", "fal-billing"),
        ("PHALA_CLOUD_API_KEY", "phala", "phala"),
        ("OPENROUTER_API_KEY", "openrouter", "openrouter-billing"),
    ] {
        for code in [
            "http_401",
            "http_403",
            "http_429",
            "http_503",
            "dns_failed",
            "request_timeout",
            "connection_failed",
            "invalid_balance",
        ] {
            let rows = balances::collect(&Failure(code), &json!({key:"synthetic-secret"}));
            assert_eq!(rows[0]["diagnostic"]["code"], code);
            let report = json!(balances::reports(&rows));
            assert!(!report.to_string().contains("diagnostic"));
            let rows = balances::merge(&Value::Null, rows, everyframe_miner::now());
            assert_eq!(rows[0]["diagnostic"]["code"], code);
            let out = json!({"balances":rows});
            let shown = display::render("balances", &out);
            assert!(shown.contains(&format!("{provider} [{code}]")));
            assert!(!shown.contains("synthetic-secret"));
            if matches!(code, "http_401" | "http_403") {
                assert!(shown.contains(&format!("everycli set-api-keys --provider {setting}")));
            }
            assert!(cli::balance_check_failed(&out));
        }
    }
}

#[test]
fn snapshots_and_unknown_failures_never_invent_or_echo_reasons() {
    let now = everyframe_miner::now();
    let remote = json!([{"provider":"fal","currency":"USD","status":"unavailable","checkedAt":now,"diagnostic":{"code":"http_401","message":"raw-secret","next":"raw-secret"}}]);
    let rows = balances::merge(&remote, vec![], now);
    assert_eq!(
        rows[0]["diagnostic"]["code"],
        "balance_snapshot_reason_unavailable"
    );
    assert!(!json!(rows).to_string().contains("raw-secret"));
    let local = balances::collect(
        &Failure("raw-secret"),
        &json!({"FAL_KEY":"synthetic-secret"}),
    );
    let rows = balances::merge(&remote, local, now);
    assert_eq!(rows[0]["diagnostic"]["code"], "request_failed");
    assert_eq!(rows[0]["source"], "local");
    assert!(!json!(rows).to_string().contains("raw-secret"));
    let out = json!({"balances":[],"remoteUnavailable":true,"remoteError":{"code":"http_401","message":"raw-secret"}});
    let shown = display::render("balances", &out);
    assert!(shown.contains("Coordinator [http_401]"));
    assert!(shown.contains("No balance rows"));
    assert!(!shown.contains("raw-secret"));
    assert!(cli::balance_check_failed(&out));
    assert!(!cli::balance_check_failed(
        &json!({"balances":[{"status":"ok","stale":true}]})
    ));
    assert!(!cli::balance_check_failed(
        &json!({"balances":[{"status":"unsupported"}]})
    ));
    assert!(cli::balance_check_failed(
        &json!({"balances":[{"status":"ok"}],"publishRequested":true,"published":false})
    ));
}

#[test]
fn transport_errors_are_classified_without_exposing_request_urls() {
    use std::{net::TcpListener, time::Duration};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((_stream, _)) => {
                    std::thread::sleep(Duration::from_millis(500));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "local fixture connection timed out"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("local fixture failed: {e}"),
            }
        }
    });
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(150))
        .build()
        .unwrap();
    let e = client
        .get(format!("http://{addr}/synthetic-secret"))
        .send()
        .unwrap_err();
    assert_eq!(diagnostics::transport_error(&e).0, "request_timeout");
    thread.join().unwrap();
    let e = client
        .get(format!("http://{addr}/synthetic-secret"))
        .send()
        .unwrap_err();
    assert_eq!(diagnostics::transport_error(&e).0, "connection_failed");
    assert!(
        !diagnostics::value(diagnostics::transport_error(&e).0)
            .to_string()
            .contains("synthetic-secret")
    );
}
