mod common;

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
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
    let new = b"#!/bin/sh\nprintf '%s\\n' '{\"version\":\"0.4.2\"}'\n";
    update::replace_binary(&binary, new).unwrap();
    assert_eq!(fs::read(&binary).unwrap(), new);
    assert_eq!(
        fs::read(tmp.path().join("everycli.rollback")).unwrap(),
        b"old"
    );
    assert!(update::replace_binary(&binary, b"not executable").is_err());
    assert_eq!(fs::read(&binary).unwrap(), new);
    assert_eq!(
        fs::read(tmp.path().join("credentials.json")).unwrap(),
        b"untouched synthetic fixture"
    );
    let lock_state = everyframe_miner::state::State {
        directory: tmp.path().join(".everycli-update.lock"),
    };
    let _held = lock_state.lock().unwrap();
    assert!(update::replace_binary(&binary, b"blocked").is_err());
    assert_eq!(fs::read(&binary).unwrap(), new);
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

// A throwaway key generated per test run; the real release key is never used.
fn throwaway_key() -> SigningKey {
    SigningKey::generate(&mut rand::rngs::OsRng)
}

fn sign(key: &SigningKey, tag: &str, asset: &str, sha256_hex: &str) -> Vec<u8> {
    let signature = key.sign(&update::signed_message(tag, asset, sha256_hex));
    format!("{}\n", STANDARD.encode(signature.to_bytes())).into_bytes()
}

#[test]
fn signed_message_has_the_documented_canonical_format() {
    let sha = "ab".repeat(32);
    assert_eq!(
        update::signed_message("v0.6.1", "everycli-x86_64-unknown-linux-gnu", &sha),
        format!("everycli-release-v1\nv0.6.1\neverycli-x86_64-unknown-linux-gnu\n{sha}\n")
            .into_bytes()
    );
    let documented = "everycli-release-v1\\n<tag>\\n<asset-name>\\n<sha256-hex-lowercase>\\n";
    for doc in [
        include_str!("../RELEASING.md"),
        include_str!("../SECURITY.md"),
    ] {
        assert!(doc.contains(documented));
        assert!(doc.contains(update::RELEASE_SIGNING_PUBLIC_KEY_HEX));
    }
}

#[test]
fn release_signature_accepts_only_the_exact_tag_asset_and_hash() {
    let key = throwaway_key();
    let public = key.verifying_key();
    let tag = "v0.6.1";
    let asset = "everycli-x86_64-unknown-linux-gnu";
    let sha = hex::encode(Sha256::digest(b"synthetic release binary"));
    let sig = sign(&key, tag, asset, &sha);
    let check = |tag: &str, asset: &str, sha: &str, sig: Option<&[u8]>| {
        update::verify_signature_with_key(&public, tag, asset, sha, sig).map_err(|e| e.0)
    };
    assert_eq!(check(tag, asset, &sha, Some(&sig)), Ok(()));
    // Surrounding whitespace in the .sig file is tolerated.
    let padded = format!("  {}\r\n\n", String::from_utf8(sig.clone()).unwrap().trim());
    assert_eq!(check(tag, asset, &sha, Some(padded.as_bytes())), Ok(()));

    let invalid = Err("update_signature_invalid");
    // Replaying an older signed binary as a newer version.
    assert_eq!(check("v0.6.2", asset, &sha, Some(&sig)), invalid);
    // Another platform's binary.
    assert_eq!(
        check(tag, "everycli-aarch64-apple-darwin", &sha, Some(&sig)),
        invalid
    );
    // Different (tampered) content.
    let other = hex::encode(Sha256::digest(b"tampered release binary"));
    assert_eq!(check(tag, asset, &other, Some(&sig)), invalid);
    assert_eq!(check(tag, asset, &sha.to_uppercase(), Some(&sig)), invalid);
    // Tampered signature bytes.
    let mut raw = STANDARD
        .decode(String::from_utf8(sig.clone()).unwrap().trim())
        .unwrap();
    raw[10] ^= 1;
    let tampered = STANDARD.encode(&raw);
    assert_eq!(check(tag, asset, &sha, Some(tampered.as_bytes())), invalid);
    // Signed by a different key.
    let foreign = sign(&throwaway_key(), tag, asset, &sha);
    assert_eq!(check(tag, asset, &sha, Some(&foreign)), invalid);
    // Malformed signature files.
    for bad in [
        b"".as_slice(),
        b"not base64!",
        STANDARD.encode([0u8; 63]).as_bytes(),
        STANDARD.encode([0u8; 65]).as_bytes(),
        &[0xff, 0xfe],
    ] {
        assert_eq!(check(tag, asset, &sha, Some(bad)), invalid);
    }
    // Missing .sig is refused, never treated as "unsigned is fine".
    assert_eq!(
        check(tag, asset, &sha, None),
        Err("update_signature_missing")
    );
}

#[test]
fn production_verification_uses_the_embedded_release_key() {
    let embedded = update::release_verifying_key().unwrap();
    assert_eq!(
        hex::encode(embedded.as_bytes()),
        update::RELEASE_SIGNING_PUBLIC_KEY_HEX
    );
    let bytes = b"synthetic release binary";
    let tag = "v0.6.1";
    let asset = "everycli-x86_64-unknown-linux-gnu";
    let sha = hex::encode(Sha256::digest(bytes));
    // A throwaway key's valid signature is refused by the production check.
    let sig = sign(&throwaway_key(), tag, asset, &sha);
    assert_eq!(
        update::verify_release_signature(tag, asset, bytes, Some(&sig))
            .unwrap_err()
            .0,
        "update_signature_invalid"
    );
    assert_eq!(
        update::verify_release_signature(tag, asset, bytes, None)
            .unwrap_err()
            .0,
        "update_signature_missing"
    );
}

#[test]
fn installer_and_workflow_pin_the_same_release_key() {
    // The installer embeds the key as an SPKI PEM: fixed Ed25519 prefix + raw key.
    let installer = include_str!("../scripts/everycli-installer.sh");
    let start = installer
        .find("release_key='-----BEGIN PUBLIC KEY-----\n")
        .unwrap();
    let body = installer[start..].lines().nth(1).unwrap();
    let der = STANDARD.decode(body).unwrap();
    assert_eq!(hex::encode(&der[..12]), "302a300506032b6570032100");
    assert_eq!(
        hex::encode(&der[12..]),
        update::RELEASE_SIGNING_PUBLIC_KEY_HEX
    );
    assert!(installer.contains(update::RELEASE_SIGNING_PUBLIC_KEY_HEX));
    assert!(installer.contains(
        "printf 'everycli-release-v1\\n%s\\n%s\\n%s\\n' \"$version\" \"$asset\" \"$actual\""
    ));

    let workflow = include_str!("../.github/workflows/release.yml");
    assert!(workflow.contains("secrets.EVERYCLI_RELEASE_SIGNING_KEY"));
    assert!(workflow.contains("environment: release"));
    assert!(workflow.contains("pkeyutl -sign -inkey \"$work/key.pem\" -rawin"));
    assert!(workflow.contains(
        "printf 'everycli-release-v1\\n%s\\n%s\\n%s\\n' \"$RELEASE_TAG\" \"$asset\" \"$digest\""
    ));
    assert!(workflow.contains("pub const RELEASE_SIGNING_PUBLIC_KEY_HEX"));
    assert!(workflow.contains("--latest=\"$latest\""));
    assert!(!workflow.contains("--latest\n"));
    assert!(!workflow.contains("set -x"));
}

#[test]
fn updater_verifies_the_signature_before_running_or_installing_the_download() {
    let updater = include_str!("../src/update.rs");
    let body = &updater[updater.find("fn run_with_progress").unwrap()..];
    let sig = body.find("{base}.sig").unwrap();
    let verify = body
        .find("verify_release_signature(tag, &name, &bytes, signature.as_deref())?")
        .unwrap();
    let install = body.find("replace_binary_checked(").unwrap();
    assert!(sig < verify && verify < install);
    // Only a 404 means "missing"; other download failures stay failures.
    assert!(body.contains("Err(e) if e.0 == \"public_release_not_available\" => None"));
    let cargo = body.find("\"cargo_install_update_disabled\"").unwrap();
    assert!(cargo < body.find("Client::builder()").unwrap());
}
