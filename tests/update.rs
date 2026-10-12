mod common;

use everyframe_miner::{cli, update};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[test]
fn human_release_labels_use_a_consistent_v_prefix() {
    for value in ["0.3.2", "v0.3.2", "vv0.3.2"] {
        assert_eq!(update::display_version(value), "v0.3.2");
    }
    assert_eq!(update::display_version(""), "—");
    assert_eq!(update::display_version("—"), "—");
    let updater = include_str!("../src/update.rs");
    assert!(updater.contains("display_version(env!(\"CARGO_PKG_VERSION\"))"));
    assert_eq!(updater.matches("display_version(tag)").count(), 2);
}

#[test]
fn public_downloads_use_the_current_github_organization() {
    let repo = "https://github.com/everyframe-studio/everyframe-miner";
    let updater = include_str!("../src/update.rs");
    let installer = include_str!("../scripts/everycli-installer.sh");
    let readme = include_str!("../README.md");
    assert!(updater.contains(repo));
    assert!(updater.contains(
        "https://api.github.com/repos/everyframe-studio/everyframe-miner/releases/latest"
    ));
    assert!(installer.contains(&format!("{repo}/releases/download/$version")));
    assert!(readme.contains(&format!(
        "{repo}/releases/latest/download/everycli-installer.sh"
    )));
    for text in [updater, installer, readme] {
        assert!(!text.contains("everyframe-studios/"));
    }
}

#[test]
fn cli_update_is_separate_from_worker_update() {
    let parse = |a: &[&str]| cli::parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(parse(&["update"]).unwrap().command, "self-update");
    assert!(
        parse(&["update", "--check", "--json"])
            .unwrap()
            .flag("--check")
    );
    assert_eq!(
        parse(&["miner", "update", "--release", "release.json"])
            .unwrap()
            .command,
        "update"
    );
    assert!(
        parse(&["worker-update", "--check"])
            .unwrap()
            .flag("--check")
    );
    assert!(
        parse(&["miner", "update", "--check"])
            .unwrap()
            .flag("--check")
    );
    assert_eq!(parse(&["worker-update"]).unwrap().command, "update");
    for a in [
        vec!["update", "--state-dir", "/private"],
        vec!["update", "--release", "file"],
    ] {
        assert!(parse(&a).is_err());
    }
}

#[test]
fn update_release_versions_platforms_and_urls_are_fenced() {
    assert!(update::version("v0.10.0").unwrap() > update::version("v0.9.9").unwrap());
    for v in [
        "latest",
        "v1.2.3/evil",
        "v1.2.3-rc1",
        "v01.2.3",
        "1.2.3",
        "v999999999999999999999999.0.0",
    ] {
        assert!(update::version(v).is_err());
    }
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
    ] {
        assert!(update::asset(os, arch).is_ok());
    }
    assert!(update::asset("windows", "x86_64").is_err());
    for u in [
        "https://github.com/x",
        "https://release-assets.githubusercontent.com/x?a=b",
    ] {
        assert!(update::trusted_url(&u.parse().unwrap()));
    }
    for u in [
        "http://github.com/x",
        "https://github.com.evil.test/x",
        "https://github.com:444/x",
        "https://secret@github.com/x",
        "https://127.0.0.1/x",
        "https://github.com/x#y",
    ] {
        assert!(!update::trusted_url(&u.parse().unwrap()));
    }
}

#[test]
fn checksum_rejects_tampering_and_wrong_asset() {
    let bytes = b"synthetic executable bytes";
    let sum = format!("{}  everycli-test\n", hex::encode(Sha256::digest(bytes)));
    assert!(update::verify(bytes, &sum, "everycli-test").is_ok());
    assert!(update::verify(b"tampered", &sum, "everycli-test").is_err());
    assert!(update::verify(bytes, &sum, "everycli-other").is_err());
    assert!(update::verify(bytes, &format!("{sum}{sum}"), "everycli-test").is_err());
}

#[test]
fn replacement_is_atomic_and_preserves_profiles() {
    let tmp = common::tempdir();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let binary = tmp.path().join("everycli");
    fs::write(&binary, b"old").unwrap();
    fs::write(
        tmp.path().join("credentials.json"),
        b"untouched synthetic fixture",
    )
    .unwrap();
    update::replace_binary(&binary, b"new").unwrap();
    assert_eq!(fs::read(&binary).unwrap(), b"new");
    assert_eq!(
        fs::read(tmp.path().join("credentials.json")).unwrap(),
        b"untouched synthetic fixture"
    );
    assert!(!tmp.path().join(".everycli-update.lock").exists());
    fs::create_dir(tmp.path().join(".everycli-update.lock")).unwrap();
    assert!(update::replace_binary(&binary, b"blocked").is_err());
    assert_eq!(fs::read(&binary).unwrap(), b"new");
}

#[test]
fn replacement_refuses_symlinks_and_writable_install_dirs() {
    let tmp = common::tempdir();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let binary = tmp.path().join("real");
    fs::write(&binary, b"old").unwrap();
    let link = tmp.path().join("everycli");
    symlink(&binary, &link).unwrap();
    assert!(update::replace_binary(&link, b"new").is_err());
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(update::replace_binary(&binary, b"new").is_err());
    assert_eq!(fs::read(&binary).unwrap(), b"old");
}
