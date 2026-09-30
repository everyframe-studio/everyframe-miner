mod common;

use everyframe_miner::{
    cli, invitation,
    network::{public_address, safe_url},
    state::{self, State},
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[test]
fn network_destination_policy() {
    let hosts = vec!["api.example.org".into(), "*.media.example.org".into()];
    for u in [
        "https://api.example.org/x",
        "https://v1.media.example.org/x",
    ] {
        assert!(safe_url(u, &hosts).is_ok());
    }
    for u in [
        "http://api.example.org",
        "https://api.example.org:8080",
        "https://a:b@api.example.org",
        "https://api.example.org/#secret",
        "https://api.example.org.evil.com",
        "https://media.example.org",
        "https://127.0.0.1",
    ] {
        assert!(safe_url(u, &hosts).is_err(), "{u}");
    }
    for ip in [
        "127.0.0.1",
        "10.0.0.2",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "224.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(!public_address(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
        assert!(public_address(ip.parse().unwrap()));
    }
}
#[test]
fn private_state_and_exclusive_lock() {
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    state.prepare().unwrap();
    assert_eq!(
        fs::metadata(&state.directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    state
        .write("credentials", &json!({"test":"synthetic"}))
        .unwrap();
    assert_eq!(
        fs::metadata(state.path("credentials").unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        state.read("credentials", false).unwrap()["test"],
        "synthetic"
    );
    let lock = state.lock().unwrap();
    assert!(state.lock().is_err());
    drop(lock);
    assert!(state.lock().is_ok());
    assert!(state.path("../../secret").is_err());
    fs::set_permissions(
        state.path("credentials").unwrap(),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(state.read("credentials", false).is_err());
}
#[test]
fn symlinks_and_insecure_directories_are_rejected() {
    let tmp = common::tempdir();
    let target = tmp.path().join("real");
    fs::write(&target, b"{}").unwrap();
    let link = tmp.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(state::read(&link, false, 100).is_err());
    let dir = tmp.path().join("insecure");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(State { directory: dir }.prepare().is_err());
    assert!(state::read(&target, false, 1).is_err());
}
#[test]
fn provider_environment_and_discount_validation() {
    assert!(invitation::valid_environment(&json!([
        "MINER_ID",
        "MINER_TOKEN",
        "FAL_KEY"
    ])));
    for keys in [
        json!(["MINER_ID", "MINER_TOKEN", "FAL_KEY", "LD_PRELOAD"]),
        json!(["MINER_ID", "MINER_TOKEN", "FAL_KEY", "FAL_KEY"]),
        json!([
            "MINER_ID",
            "MINER_TOKEN",
            "FAL_KEY",
            "DSTACK_DOCKER_PASSWORD"
        ]),
    ] {
        assert!(!invitation::valid_environment(&keys));
    }
    for (v, n) in [("0", 0), ("0.01", 1), ("1.5", 150), ("99.90", 9990)] {
        assert_eq!(invitation::discount(v).unwrap(), n);
    }
    for v in ["100", "99.91", "-1", "NaN", "1.001", "01", "1e1", ""] {
        assert!(invitation::discount(v).is_err(), "{v}");
    }
    assert!(!invitation::configured(&json!(invitation::DISABLED)));
}
#[test]
fn all_cli_commands_and_scoped_options_parse() {
    for command in [
        "init",
        "doctor",
        "status",
        "providers",
        "offers",
        "offer",
        "earnings",
        "deploy",
        "activate",
        "resume",
        "update",
        "stop",
        "start",
        "reconcile",
    ] {
        let args = ["miner", command, "--network", "testnet", "--json"].map(String::from);
        assert_eq!(cli::parse(&args).unwrap().command, command);
    }
    for args in [
        vec!["miner", "status", "--withdraw"],
        vec!["miner", "offer", "--withdraw", "--discount-pct", "10"],
        vec!["miner", "deploy", "--api-key", "DO_NOT_PRINT"],
        vec!["miner", "unknown"],
    ] {
        assert!(cli::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
    }
}
#[test]
fn binaries_work_without_any_runtime_or_credentials() {
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args(["--version", "--json"])
        .output()
        .unwrap();
    assert!(cli.status.success());
    assert!(String::from_utf8_lossy(&cli.stdout).contains("rust"));
    let worker = std::process::Command::new(env!("CARGO_BIN_EXE_everyframe-worker"))
        .env_clear()
        .output()
        .unwrap();
    assert!(!worker.status.success());
    assert!(String::from_utf8_lossy(&worker.stderr).contains("real_dstack_socket_required"));
}

#[test]
fn credential_file_parsing_never_expands_or_executes_values() {
    use everyframe_miner::miner::read_secrets;
    let dir = common::tempdir();
    let path = dir.path().join("synthetic.env");
    fs::write(&path,"export FAL_KEY=\"literal-${HOME}-$(whoami)-\\\"quote\\\"\" # comment\nMINIMAX_API_KEY='literal-${TOKEN}'\nGEMINI_API_KEY=some-token\t# ignored comment\nIGNORED=unused\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let values = read_secrets(&path).unwrap();
    assert_eq!(values["FAL_KEY"], "literal-${HOME}-$(whoami)-\"quote\"");
    assert_eq!(values["MINIMAX_API_KEY"], "literal-${TOKEN}");
    assert_eq!(values["GEMINI_API_KEY"], "some-token");
    assert!(values.get("IGNORED").is_none());
    for text in [
        "FAL_KEY=\"bad\\nvalue\"",
        "FAL_KEY=\"unclosed",
        "FAL_KEY=a\0b",
    ] {
        fs::write(&path, text).unwrap();
        assert!(read_secrets(&path).is_err());
    }
}

#[test]
fn real_transport_refuses_private_ip_before_connecting() {
    use everyframe_miner::network::{Http, PublicHttp, Request};
    let request = Request::get("https://127.0.0.1/", &["127.0.0.1"]);
    assert_eq!(
        PublicHttp.bytes(request).unwrap_err().0,
        "private_destination"
    );
}

#[test]
fn cli_errors_do_not_echo_unknown_secret_arguments() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_everycli"))
        .args([
            "miner",
            "status",
            "--secret-synthetic-DO-NOT-ECHO",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DO-NOT-ECHO"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("DO-NOT-ECHO"));
}

#[test]
fn fixtures_resolve_os_temporary_root_alias_without_relaxing_state_checks() {
    let base = common::tempdir();
    let physical = base.path().join("physical-temp-root");
    fs::create_dir(&physical).unwrap();
    let alias = base.path().join("os-temp-alias");
    symlink(&physical, &alias).unwrap();
    // Reproduce the old failure on every OS, including Linux CI.
    assert_eq!(
        state::no_symlink(&alias.join("profile")).unwrap_err().0,
        "unsafe_path"
    );
    let fixture = common::tempdir_in(&alias);
    assert_eq!(fixture.path(), fs::canonicalize(fixture.path()).unwrap());
    let profile = State {
        directory: fixture.path().join("profile"),
    };
    profile
        .write("credentials", &json!({"synthetic":"fixture"}))
        .unwrap();
    assert_eq!(
        profile.read("credentials", false).unwrap()["synthetic"],
        "fixture"
    );
    let lock = profile.lock().unwrap();
    assert!(profile.lock().is_err());
    drop(lock);
}

#[test]
fn credential_ancestor_symlinks_remain_rejected() {
    let base = common::tempdir();
    let real = base.path().join("real");
    fs::create_dir(&real).unwrap();
    let secret = real.join("credentials.json");
    fs::write(&secret, b"{}").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(state::read_json(&secret, true).is_ok());
    let alias = base.path().join("user-link");
    symlink(&real, &alias).unwrap();
    assert_eq!(
        state::read_json(&alias.join("credentials.json"), true)
            .unwrap_err()
            .0,
        "unsafe_path"
    );
    assert!(
        State {
            directory: alias.join("profile")
        }
        .prepare()
        .is_err()
    );
    assert!(!real.join("profile").exists());
}
