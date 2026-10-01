mod common;

use everyframe_miner::update;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::{Command, Output},
};

struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = common::tempdir();
        let p = dir.path();
        fs::create_dir(p.join("mock-bin")).unwrap();
        // Downloads are mocked, but hashing, execution, permissions, locking,
        // replacement, and cleanup use the actual installer and native CLI.
        fs::write(
            p.join("mock-bin/curl"),
            r#"#!/bin/sh
set -eu
[ "${EVERYCLI_TEST_FAIL:-0}" = 0 ] || exit 22
url=''
dest=''
retry=''
connect_timeout=''
retry_limit=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    https://github.com/everyframe-studios/everyframe-miner/releases/download/v*) url=$1 ;;
    -o) shift; dest=$1 ;;
    --retry) shift; retry=$1 ;;
    --connect-timeout) shift; connect_timeout=$1 ;;
    --retry-max-time) shift; retry_limit=$1 ;;
  esac
  shift
done
[ -n "$url" ] && [ -n "$dest" ]
[ "$retry" = 3 ] && [ "$connect_timeout" = 30 ] && [ "$retry_limit" = 240 ]
case "$url" in
  *.sha256) cp "$EVERYCLI_TEST_SUM" "$dest" ;;
  *) cp "$EVERYCLI_TEST_BINARY" "$dest" ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(p.join("mock-bin/curl"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            p.join("installer.sh"),
            include_str!("../scripts/everycli-installer.sh")
                .replace("@VERSION@", concat!("v", env!("CARGO_PKG_VERSION"))),
        )
        .unwrap();
        let binary = env!("CARGO_BIN_EXE_everycli");
        let asset = update::asset(std::env::consts::OS, std::env::consts::ARCH).unwrap();
        fs::write(
            p.join("checksum"),
            format!(
                "{}  {asset}\n",
                hex::encode(Sha256::digest(fs::read(binary).unwrap()))
            ),
        )
        .unwrap();
        Self { dir }
    }
    fn installed(&self) -> std::path::PathBuf {
        self.dir.path().join("cargo space'quoted/bin/everycli")
    }
    fn run(&self, fail: bool) -> Output {
        self.run_path(fail, false, false)
    }
    fn run_path(&self, fail: bool, on_path: bool, modify: bool) -> Output {
        Command::new("sh")
            .arg(self.dir.path().join("installer.sh"))
            .env(
                "PATH",
                format!(
                    "{}:{}:{}",
                    self.dir.path().join("mock-bin").display(),
                    if on_path {
                        self.installed().parent().unwrap().display().to_string()
                    } else {
                        "/nonexistent-test-bin".into()
                    },
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("CARGO_HOME", self.dir.path().join("cargo space'quoted"))
            .env("HOME", self.dir.path())
            .env("EVERYCLI_NO_MODIFY_PATH", if modify { "0" } else { "1" })
            .env("EVERYCLI_TEST_BINARY", env!("CARGO_BIN_EXE_everycli"))
            .env("EVERYCLI_TEST_SUM", self.dir.path().join("checksum"))
            .env("EVERYCLI_TEST_FAIL", if fail { "1" } else { "0" })
            .output()
            .unwrap()
    }
}

#[test]
fn installer_only_requests_path_setup_when_needed_and_does_not_duplicate_path() {
    let f = Fixture::new();
    for modify in [true, false] {
        let out = f.run_path(false, true, modify);
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains("run:\n"));
        assert!(!text.contains("terminal"));
        assert!(!text.contains("Add "));
    }
    let out = f.run_path(false, false, true);
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("To use everycli in this terminal")
    );
    let bin = f
        .installed()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let out = Command::new("sh")
        .args(["-c", ". \"$1\"; . \"$1\"; printf '%s' \"$PATH\"", "test"])
        .arg(f.dir.path().join("cargo space'quoted/everycli-env"))
        .env("PATH", format!("{bin}:/usr/bin:/bin"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{bin}:/usr/bin:/bin")
    );
    let profile = fs::read_to_string(f.dir.path().join(".zshrc")).unwrap();
    assert_eq!(profile.matches("# Everyframe CLI").count(), 1);
}

#[test]
fn installer_installs_and_reinstalls_verified_native_cli_without_rust() {
    let f = Fixture::new();
    for _ in 0..2 {
        let out = f.run(false);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            Command::new(f.installed())
                .arg("--version")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            !f.installed()
                .parent()
                .unwrap()
                .join(".everycli-update.lock")
                .exists()
        );
    }
}

#[test]
fn installer_failure_keeps_existing_binary_and_removes_lock() {
    let f = Fixture::new();
    assert!(f.run(false).status.success());
    let before = fs::read(f.installed()).unwrap();
    assert!(!f.run(true).status.success());
    let asset = update::asset(std::env::consts::OS, std::env::consts::ARCH).unwrap();
    fs::write(
        f.dir.path().join("checksum"),
        format!("{}  {asset}\n", "0".repeat(64)),
    )
    .unwrap();
    assert!(!f.run(false).status.success());
    assert_eq!(fs::read(f.installed()).unwrap(), before);
    assert!(
        !f.installed()
            .parent()
            .unwrap()
            .join(".everycli-update.lock")
            .exists()
    );
}

#[test]
fn installer_refuses_symlink_and_unversioned_template() {
    let f = Fixture::new();
    fs::create_dir_all(f.installed().parent().unwrap()).unwrap();
    let other = f.dir.path().join("other");
    fs::write(&other, b"untouched").unwrap();
    symlink(&other, f.installed()).unwrap();
    assert!(!f.run(false).status.success());
    assert_eq!(fs::read(other).unwrap(), b"untouched");
    fs::write(
        f.dir.path().join("installer.sh"),
        include_str!("../scripts/everycli-installer.sh"),
    )
    .unwrap();
    assert!(!f.run(false).status.success());
}

#[test]
fn installer_quotes_shell_paths_without_expansion() {
    let line = include_str!("../scripts/everycli-installer.sh")
        .lines()
        .find(|l| l.trim_start().starts_with("quote() {"))
        .unwrap();
    let script = format!(
        "{line}\nquoted=$(quote \"$1\")\neval \"decoded=$quoted\"\nprintf '%s' \"$decoded\"\n"
    );
    let path = "/tmp/path with 'quote' and $(false) and $dollar";
    let out = Command::new("sh")
        .args(["-c", &script, "test", path])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), path);
}
