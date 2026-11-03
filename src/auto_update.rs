//! Best-effort native CLI updates. Never loads a miner profile or provider key.
use crate::{Error, Result, cli::Args, need, now, state::State};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::IsTerminal,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const INTERVAL_MS: i64 = 6 * 60 * 60 * 1000;
const INTERNAL: &str = "__everycli_background_update";

fn state() -> Result<State> {
    let root = std::env::var_os("HOME").ok_or(Error("home_required"))?;
    Ok(State {
        directory: PathBuf::from(root).join(".config/everycli-updater"),
    })
}

// Only native, owner-managed installation paths: the official installer's
// `${CARGO_HOME:-~/.cargo}/bin` and `~/.local/bin`. Homebrew/Nix/system packages,
// build directories and custom symlink launchers are never rewritten. A binary in
// one of these paths that `cargo install` put there is excluded by `cargo_managed`.
pub fn native_path(target: &Path, user_home: &Path, cargo_home: Option<&Path>) -> bool {
    let cargo = cargo_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| user_home.join(".cargo"));
    cargo.is_absolute()
        && user_home.is_absolute()
        && [
            cargo.join("bin/everycli"),
            user_home.join(".local/bin/everycli"),
        ]
        .contains(&target.to_path_buf())
}

/// True when the executable is a `cargo install` (source) build: cargo's own
/// install records beside its bin directory (`<root>/.crates.toml` or
/// `<root>/.crates2.json`, for `~/.cargo`, `$CARGO_HOME` or any `--root`) list
/// an `everycli` binary. Such builds are never replaced by `everycli update` or
/// auto-update; the user reruns `cargo install`. The official installer writes
/// no cargo records, so its `~/.cargo/bin` installs keep updating.
pub fn cargo_managed(target: &Path) -> bool {
    let Some(bin) = target.parent() else {
        return false;
    };
    let Some(root) = bin.parent() else {
        return false;
    };
    bin.file_name() == Some("bin".as_ref())
        && [".crates.toml", ".crates2.json"].iter().any(|name| {
            let path = root.join(name);
            // Bounded read of cargo's metadata; never follows a symlinked record.
            fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 4 << 20)
                && fs::read(&path)
                    .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("\"everycli\""))
        })
}
fn eligible() -> bool {
    if !crate::update::official_build() {
        return false;
    }
    let Ok(target) = std::env::current_exe() else {
        return false;
    };
    let Some(user_home) = std::env::var_os("HOME") else {
        return false;
    };
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from);
    if !native_path(&target, Path::new(&user_home), cargo_home.as_deref())
        || cargo_managed(&target)
        || crate::state::no_symlink(&target).is_err()
    {
        return false;
    }
    let uid = unsafe { libc::geteuid() };
    target
        .parent()
        .and_then(|p| fs::symlink_metadata(p).ok())
        .is_some_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o022 == 0)
        && fs::symlink_metadata(target)
            .is_ok_and(|m| m.is_file() && m.uid() == uid && m.mode() & 0o6022 == 0)
}
fn env_disabled() -> bool {
    std::env::var("EVERYCLI_DISABLE_AUTO_UPDATE").as_deref() == Ok("1")
}
fn configured(s: &State) -> Result<bool> {
    let v = s.read("auto-update-settings", true)?;
    if v.is_null() {
        return Ok(true);
    }
    need(v["enabled"].is_boolean(), "invalid_auto_update_settings")?;
    Ok(v["enabled"] == true)
}
pub fn due(last: Option<i64>, at: i64) -> bool {
    last.is_none_or(|last| at < last || at.saturating_sub(last) >= INTERVAL_MS)
}

// Advisory locks are released by the kernel even on a crash or hard kill.
struct UpdateLock(File);
impl Drop for UpdateLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn lock(s: &State) -> Result<UpdateLock> {
    s.prepare()?;
    let p = s.directory.join("auto-update.lock");
    crate::state::no_symlink(&p)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p)
        .map_err(|_| Error("auto_update_locked"))?;
    let m = f.metadata().map_err(|_| Error("auto_update_locked"))?;
    need(
        m.is_file() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
        "insecure_file",
    )?;
    need(
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "auto_update_locked",
    )?;
    Ok(UpdateLock(f))
}

pub fn settings(enable: bool, disable: bool) -> Result<Value> {
    need(!(enable && disable), "conflicting_auto_update_options")?;
    let s = state()?;
    if enable || disable {
        s.write("auto-update-settings", &json!({"enabled":enable}))?;
    }
    let enabled = configured(&s)?;
    Ok(
        json!({"enabled":enabled,"effective":enabled && !env_disabled() && eligible(),
        "environmentDisabled":env_disabled(),"nativeInstallation":eligible(),
        "cargoInstall":std::env::current_exe().is_ok_and(|p| cargo_managed(&p)),"intervalHours":6,
        "lastAttempt":s.read("auto-update-result",true)?,
        "lastUpdate":s.read("last-successful-update",true)?,
        "currentRelease":crate::upgrade_notes::current_release(env!("CARGO_PKG_VERSION")),
        "note":"Interactive native CLI only; updates take effect on the next invocation. JSON, CI, help and version commands do not start background updates. Miner workers, keys and offers are unchanged."}),
    )
}

fn remember_at(s: &State, value: &Value, shown: bool) -> Result<()> {
    if value["updated"] != true {
        return Ok(());
    }
    let _guard = s.lock()?;
    let previous = s.read("last-successful-update", true)?;
    let old = previous["result"]["version"]
        .as_str()
        .and_then(|v| crate::update::version(&crate::update::display_version(v)).ok());
    let new = value["version"]
        .as_str()
        .and_then(|v| crate::update::version(&crate::update::display_version(v)).ok());
    if old.zip(new).is_some_and(|(old, new)| old > new) {
        return Ok(());
    }
    s.write(
        "last-successful-update",
        &json!({"recordedAt":now(),"notificationShown":shown,"result":value}),
    )
}
pub(crate) fn remember_update(value: &Value, shown: bool) -> Result<()> {
    remember_at(&state()?, value, shown)
}

fn take_update_notice(s: &State) -> Result<Option<String>> {
    let _guard = s.lock()?;
    let mut receipt = s.read("last-successful-update", true)?;
    if receipt["result"]["updated"] != true || receipt["notificationShown"] == true {
        return Ok(None);
    }
    let Some(installed) = receipt["result"]["version"]
        .as_str()
        .and_then(|v| crate::update::version(&crate::update::display_version(v)).ok())
    else {
        return Ok(None);
    };
    if installed > crate::update::version(concat!("v", env!("CARGO_PKG_VERSION")))? {
        return Ok(None);
    }
    let result = &receipt["result"];
    let notice = format!(
        "CLI updated: {} → {}.\n{}\nReview later: everycli auto-update\n",
        crate::update::display_version(result["previous"].as_str().unwrap_or("—")),
        crate::update::display_version(result["version"].as_str().unwrap_or("—")),
        crate::upgrade_notes::render(&result["upgrade"])
    );
    receipt["notificationShown"] = json!(true);
    s.write("last-successful-update", &receipt)?;
    Ok(Some(notice))
}

pub fn internal(raw: &[String]) -> bool {
    raw.len() == 1 && raw[0] == INTERNAL
}

fn perform(s: &State, updater: impl FnOnce() -> Result<Value>) -> Result<()> {
    let _lock = lock(s)?;
    if !configured(s)? || env_disabled() {
        return Ok(());
    }
    let at = now();
    s.write(
        "auto-update-result",
        &json!({"checkedAt":at,"state":"checking"}),
    )?;
    let result = match updater() {
        Ok(v) => {
            let _ = remember_at(s, &v, false);
            json!({"checkedAt":at,"finishedAt":now(),"state":if v["updated"]==true {"updated"} else {"current"},"result":v})
        }
        Err(e) => json!({"checkedAt":at,"finishedAt":now(),"state":"failed","code":e.0}),
    };
    s.write("auto-update-result", &result)
}
pub fn background_entry() -> i32 {
    if !eligible() || env_disabled() {
        return 0;
    }
    let result = (|| {
        let s = state()?;
        perform(&s, || {
            crate::update::background(&|| !env_disabled() && configured(&s).unwrap_or(false))
        })
    })();
    if result.is_ok() { 0 } else { 2 }
}

pub fn maybe_launch(args: &Args) {
    if args.flag("--json")
        || args.flag("--help")
        || args.flag("--version")
        || args.command.is_empty()
        || ["self-update", "auto-update"].contains(&args.command.as_str())
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
        || std::env::var_os("CI").is_some()
        || env_disabled()
        || !eligible()
    {
        return;
    }
    let _ = (|| -> Result<()> {
        let s = state()?;
        if let Some(notice) = take_update_notice(&s)? {
            eprintln!("{notice}");
        }
        if !configured(&s)? {
            return Ok(());
        }
        {
            let _lock = lock(&s)?;
            let last = s.read("auto-update-result", true)?;
            if !due(last["checkedAt"].as_i64(), now()) {
                return Ok(());
            }
            // Reserve this interval before spawn: concurrent invocations cannot
            // start a herd of downloaders. Failed checks also back off six hours.
            s.write(
                "auto-update-result",
                &json!({"checkedAt":now(),"state":"scheduled"}),
            )?;
        }
        let executable =
            std::env::current_exe().map_err(|_| Error("installed_binary_unavailable"))?;
        let mut command = Command::new(executable);
        command
            .arg(INTERNAL)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for k in ["HOME", "CARGO_HOME"] {
            if let Some(v) = std::env::var_os(k) {
                command.env(k, v);
            }
        }
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        if command.spawn().is_err() {
            s.write(
                "auto-update-result",
                &json!({"checkedAt":now(),"state":"failed","code":"auto_update_spawn_failed"}),
            )?;
            return Ok(());
        }
        eprintln!(
            "Checking CLI updates in the background. Status: everycli auto-update. Disable: everycli auto-update --disable."
        );
        Ok(())
    })(); // Updates must never change the user's command result or exit status.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_update_history_survives_later_checks_and_notifies_once() {
        let dir = tempfile::tempdir().unwrap();
        let s = State {
            directory: fs::canonicalize(dir.path()).unwrap().join("updater"),
        };
        let v = json!({"updated":true,"previous":"0.9.3","version":"v0.9.5",
            "upgrade":crate::upgrade_notes::bundled("0.9.3","v0.9.5")});
        remember_at(&s, &v, false).unwrap();
        s.write(
            "auto-update-result",
            &json!({"state":"current","result":{"updated":false}}),
        )
        .unwrap();
        assert_eq!(
            s.read("last-successful-update", false).unwrap()["result"],
            v
        );
        let notice = take_update_notice(&s).unwrap().unwrap();
        assert!(notice.contains("BFL") && notice.contains("everycli worker-update"));
        assert!(take_update_notice(&s).unwrap().is_none());
        assert_eq!(
            s.read("last-successful-update", false).unwrap()["result"],
            v
        );
        remember_at(&s, &json!({"updated":true,"version":"v0.9.2"}), false).unwrap();
        assert_eq!(
            s.read("last-successful-update", false).unwrap()["result"],
            v
        );
    }

    #[test]
    fn a_receipt_for_a_future_invocation_is_not_announced_by_the_old_cli() {
        let dir = tempfile::tempdir().unwrap();
        let s = State {
            directory: fs::canonicalize(dir.path()).unwrap().join("updater"),
        };
        remember_at(
            &s,
            &json!({"updated":true,"previous":"0.9.6","version":"v99.0.0"}),
            false,
        )
        .unwrap();
        assert!(take_update_notice(&s).unwrap().is_none());
        assert_eq!(
            s.read("last-successful-update", false).unwrap()["notificationShown"],
            false
        );
    }
    #[test]
    fn native_paths_exclude_builds_packages_and_relative_overrides() {
        let h = Path::new("/home/miner");
        // Installer location; `cargo_managed` separately excludes cargo builds.
        assert!(native_path(
            Path::new("/home/miner/.cargo/bin/everycli"),
            h,
            None
        ));
        assert!(native_path(
            Path::new("/custom/bin/everycli"),
            h,
            Some(Path::new("/custom"))
        ));
        for p in [
            "/project/target/release/everycli",
            "/opt/homebrew/bin/everycli",
            "/usr/bin/everycli",
            "/home/miner/.cargo/bin/other",
        ] {
            assert!(!native_path(Path::new(p), h, None));
        }
        assert!(!native_path(
            Path::new("relative/bin/everycli"),
            h,
            Some(Path::new("relative"))
        ));
    }
    #[test]
    fn cargo_install_builds_in_any_install_root_are_not_native_updates() {
        let dir = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(dir.path()).unwrap();
        let cargo = home.join(".cargo");
        let target = cargo.join("bin/everycli");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"binary").unwrap();
        // Installer-placed binary: no cargo records.
        assert!(native_path(&target, &home, None) && !cargo_managed(&target));
        // Unrelated cargo installs do not make everycli a cargo build.
        fs::write(
            cargo.join(".crates.toml"),
            "[v1]\n\"ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rg\"]\n",
        )
        .unwrap();
        assert!(!cargo_managed(&target));
        for (file, text) in [
            (
                ".crates.toml",
                "[v1]\n\"everyframe-miner 0.5.8 (git+https://github.com/everyframe-studio/everyframe-miner#b1c20c7)\" = [\"everycli\"]\n",
            ),
            (
                ".crates2.json",
                r#"{"installs":{"everyframe-miner 0.5.8 (path+file:///src)":{"bins":["everycli"]}}}"#,
            ),
        ] {
            let _ = fs::remove_file(cargo.join(".crates.toml"));
            let _ = fs::remove_file(cargo.join(".crates2.json"));
            fs::write(cargo.join(file), text).unwrap();
            assert!(cargo_managed(&target), "{file}");
        }
        // `cargo install --root ~/.local` records beside ~/.local/bin.
        let local = home.join(".local/bin/everycli");
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        fs::write(&local, b"binary").unwrap();
        assert!(!cargo_managed(&local));
        fs::write(
            home.join(".local/.crates2.json"),
            r#"{"installs":{"everyframe-miner 0.6.0":{"bins":["everycli"]}}}"#,
        )
        .unwrap();
        assert!(cargo_managed(&local));
        assert!(!cargo_managed(Path::new("/everycli")));
    }
    #[test]
    fn interval_handles_first_run_failure_backoff_and_clock_change() {
        assert!(due(None, 100));
        assert!(!due(Some(100), 101));
        assert!(due(Some(100), 100 + INTERVAL_MS));
        assert!(due(Some(100), 99));
    }
    #[test]
    fn disabled_and_locked_skip_updater_and_crashed_locks_do_not_stick() {
        let dir = tempfile::tempdir().unwrap();
        let s = State {
            // macOS exposes TMPDIR through /var -> /private/var. Resolve
            // this test-owned directory, not a user's credential/state path.
            directory: fs::canonicalize(dir.path()).unwrap().join("cache"),
        };
        s.write("auto-update-settings", &json!({"enabled":false}))
            .unwrap();
        perform(&s, || panic!("disabled")).unwrap();
        s.write("auto-update-settings", &json!({"enabled":true}))
            .unwrap();
        let held = lock(&s).unwrap();
        assert!(perform(&s, || panic!("locked")).is_err());
        drop(held);
        perform(&s, || Ok(json!({"updated":true,"version":"v9.0.0"}))).unwrap();
        assert_eq!(
            s.read("auto-update-result", false).unwrap()["state"],
            "updated"
        );
        perform(&s, || Err(Error("release_checksum_mismatch"))).unwrap();
        let v = s.read("auto-update-result", false).unwrap();
        assert_eq!(v["state"], "failed");
        assert_eq!(v["code"], "release_checksum_mismatch");
    }
}
