use everyframe_miner::{cli, invitation, updater_vm};
use serde_json::json;

#[test]
fn docker_default_remains_worker_and_updater_is_explicit() {
    let dockerfile = include_str!("../Dockerfile");
    let stages: Vec<_> = dockerfile
        .lines()
        .filter(|line| line.starts_with("FROM "))
        .collect();
    assert!(stages.last().unwrap().ends_with(" AS worker"));
    assert!(stages.iter().any(|line| line.ends_with(" AS updater")));
    assert_eq!(
        dockerfile
            .matches("COPY LICENSE LICENSE-NOTICE.md /usr/share/doc/everyframe/")
            .count(),
        3
    );
}

#[test]
fn docker_context_includes_every_embedded_registry_but_excludes_private_state() {
    let ignore = include_str!("../.dockerignore");
    assert_eq!(ignore.lines().next(), Some("**"));
    for license in ["!LICENSE", "!LICENSE-NOTICE.md"] {
        assert!(ignore.lines().any(|line| line == license));
    }
    for file in [
        "models.json",
        "provider-expansion.json",
        "provider-model-status.json",
        "studio-video.json",
        "studio-media.json",
        "mainnet.json",
        "testnet.json",
        "release.json",
    ] {
        assert!(ignore.lines().any(|l| l == format!("!config/{file}")));
    }
    for forbidden in ["!.env", "!data/", "!profile/", "!wallets/", "!config/**"] {
        assert!(!ignore.lines().any(|l| l == forbidden));
    }
}

#[test]
fn isolated_compose_is_exact_and_has_no_shared_management_credentials() {
    let image = format!(
        "ghcr.io/everyframebuilder/updater@sha256:{}",
        "c".repeat(64)
    );
    let docker = format!(
        "services:\n  worker:\n    image: ghcr.io/everyframebuilder/worker@sha256:{}\n{}",
        "a".repeat(64),
        updater_vm::suffix(&image)
    );
    assert_eq!(
        updater_vm::image(&json!({"docker_compose_file":docker})).unwrap(),
        image
    );
    for bad in [
        docker.replace(
            "  worker:",
            "  worker:\n    environment:\n      LEAK: ${EVERYFRAME_UPDATER_SEED}",
        ),
        docker.replace("read_only: true", "read_only: false"),
        docker.replace(
            "updater-state:/state",
            "/var/run/docker.sock:/var/run/docker.sock",
        ),
    ] {
        assert!(updater_vm::image(&json!({"docker_compose_file":bad})).is_err());
    }
}

#[test]
fn capsule_exports_only_permitted_provider_keys_and_delegates() {
    let inv = json!({"request":{"compose_file":{"allowed_envs":["MINER_ID","MINER_AUTH","FAL_KEY","EVERYFRAME_UPDATER_SEED"]}}});
    let credentials = json!({"PHALA_CLOUD_API_KEY":"workspace-token","CONSOLE_AUTH":{"certificate":"console"},"WORKER_AUTH":{"certificate":"worker"},
  "FAL_KEY":"generation-key","FAL_ADMIN_KEY":"billing-secret","UPDATER_SEED":"recursive","seedPhrase":"never-export","OPENROUTER_API_KEY":"not-permitted"});
    let selected = updater_vm::permitted_credentials(&credentials, &inv);
    assert_eq!(selected.as_object().unwrap().len(), 4);
    assert_eq!(selected["FAL_KEY"], "generation-key");
    for forbidden in [
        "billing-secret",
        "recursive",
        "never-export",
        "not-permitted",
    ] {
        assert!(!selected.to_string().contains(forbidden));
    }
    assert!(invitation::valid_environment(
        &inv["request"]["compose_file"]["allowed_envs"]
    ));
}

#[test]
fn worker_auto_command_is_distinct_opt_in_and_rejects_conflicting_flags() {
    for flags in [vec![], vec!["--enable"], vec!["--disable"]] {
        let mut raw = vec!["worker-auto-update".to_string()];
        raw.extend(flags.into_iter().map(str::to_string));
        assert_eq!(cli::parse(&raw).unwrap().command, "worker-auto-update");
    }
    assert!(
        cli::parse(&["worker-auto-update", "--enable", "--disable"].map(str::to_string)).is_err()
    );
    assert!(
        cli::parse(&["worker-auto-update", "--max-hourly-usd", "10"].map(str::to_string)).is_err()
    );
}
