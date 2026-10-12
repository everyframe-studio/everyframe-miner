//! Public CLI releases are separate from signed worker releases and credentials.
use crate::{Error, Result, need};
use reqwest::{blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};

const REPO: &str = "https://github.com/everyframe-studio/everyframe-miner";
const API: &str = "https://api.github.com/repos/everyframe-studio/everyframe-miner/releases/latest";

#[derive(Debug, PartialEq)]
enum ProgressEvent {
    Stage(String),
    Download { received: u64, total: Option<u64> },
}

struct Progress<W: Write> {
    writer: W,
    enabled: bool,
    terminal: bool,
    active_line: bool,
    last_step: Option<u64>,
}
impl<W: Write> Progress<W> {
    fn new(writer: W, enabled: bool, terminal: bool) -> Self {
        Self {
            writer,
            enabled,
            terminal,
            active_line: false,
            last_step: None,
        }
    }
    fn finish(&mut self) {
        if self.active_line {
            let _ = writeln!(self.writer);
            let _ = self.writer.flush();
            self.active_line = false;
        }
    }
    fn event(&mut self, event: ProgressEvent) {
        if !self.enabled {
            return;
        }
        match event {
            ProgressEvent::Stage(message) => {
                self.finish();
                self.last_step = None;
                let _ = writeln!(self.writer, "{message}");
            }
            ProgressEvent::Download { received, total } => {
                let total = total.filter(|n| *n > 0);
                let percent = total.map(|n| ((received as u128 * 100) / n as u128).min(100) as u64);
                // In a terminal update at each percent; redirected logs get
                // coarse milestones, with no carriage returns or ANSI escapes.
                let step = percent
                    .map(|p| if self.terminal { p } else { p / 10 })
                    .unwrap_or(received / (1024 * 1024));
                if self.last_step == Some(step) {
                    return;
                }
                self.last_step = Some(step);
                let mib = received as f64 / (1024. * 1024.);
                let message = match (percent, total) {
                    (Some(p), Some(n)) => format!(
                        "Downloading: {p:3}% ({mib:.2} / {:.2} MiB)",
                        n as f64 / (1024. * 1024.)
                    ),
                    _ => format!("Downloading: {mib:.2} MiB (size unknown)"),
                };
                if self.terminal {
                    let _ = write!(self.writer, "\r{message:<72}");
                    self.active_line = true;
                } else {
                    let _ = writeln!(self.writer, "{message}");
                }
            }
        }
        let _ = self.writer.flush();
    }
}

fn read_download(
    mut reader: impl Read,
    total: Option<u64>,
    limit: u64,
    progress: &mut dyn FnMut(ProgressEvent),
) -> Result<Vec<u8>> {
    need(total.is_none_or(|n| n <= limit), "release_asset_too_large")?;
    let mut bytes = vec![];
    let mut buffer = [0u8; 32 * 1024];
    progress(ProgressEvent::Download { received: 0, total });
    loop {
        let allowed = limit
            .saturating_sub(bytes.len() as u64)
            .saturating_add(1)
            .min(buffer.len() as u64) as usize;
        let n = match reader.read(&mut buffer[..allowed]) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error("release_download_failed")),
        };
        if n == 0 {
            break;
        }
        need(
            bytes.len() as u64 + n as u64 <= limit,
            "release_asset_too_large",
        )?;
        bytes.extend_from_slice(&buffer[..n]);
        progress(ProgressEvent::Download {
            received: bytes.len() as u64,
            total,
        });
    }
    need(
        total.is_none_or(|n| n == bytes.len() as u64),
        "release_download_failed",
    )?;
    Ok(bytes)
}

pub fn version(tag: &str) -> Result<(u64, u64, u64)> {
    need(
        crate::matches(
            r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)",
            &json!(tag),
        ),
        "invalid_release_version",
    )?;
    let parts = tag[1..]
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error("invalid_release_version"))?;
    Ok((parts[0], parts[1], parts[2]))
}

pub fn asset(os: &str, arch: &str) -> Result<String> {
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return Err(Error("unsupported_update_platform")),
    };
    Ok(format!("everycli-{target}"))
}

pub fn trusted_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}

fn download(
    client: &Client,
    url: &str,
    limit: u64,
    progress: &mut dyn FnMut(ProgressEvent),
) -> Result<Vec<u8>> {
    let parsed = url::Url::parse(url).map_err(|_| Error("invalid_release_url"))?;
    need(trusted_url(&parsed), "invalid_release_url")?;
    let response = client
        .get(parsed)
        .send()
        .map_err(|_| Error("release_download_failed"))?;
    if response.status().as_u16() == 404 {
        return Err(Error("public_release_not_available"));
    }
    need(response.status().is_success(), "release_download_failed")?;
    let total = response.content_length();
    read_download(response, total, limit, progress)
}

pub fn verify(bytes: &[u8], checksum: &str, name: &str) -> Result<()> {
    let fields: Vec<_> = checksum.split_whitespace().collect();
    need(
        fields.len() == 2
            && fields[1] == name
            && fields[0].len() == 64
            && fields[0].bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid_release_checksum",
    )?;
    need(
        hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(fields[0]),
        "release_checksum_mismatch",
    )
}

struct Staging(std::path::PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        // Only our fixed-name staged file and lock directory; never recursive.
        let _ = fs::remove_file(self.0.join("everycli"));
        let _ = fs::remove_dir(&self.0);
    }
}

pub fn replace_binary(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or(Error("invalid_install_path"))?;
    let dir = fs::symlink_metadata(parent).map_err(|_| Error("install_directory_unavailable"))?;
    let old = fs::symlink_metadata(target).map_err(|_| Error("installed_binary_unavailable"))?;
    let uid = unsafe { libc::geteuid() };
    need(
        dir.is_dir()
            && dir.uid() == uid
            && dir.mode() & 0o022 == 0
            && old.is_file()
            && old.uid() == uid
            && old.mode() & 0o6022 == 0,
        "unsafe_install_permissions",
    )?;
    let lock = parent.join(".everycli-update.lock");
    fs::create_dir(&lock).map_err(|_| Error("update_locked_or_directory_not_writable"))?;
    let staging = Staging(lock);
    fs::set_permissions(&staging.0, fs::Permissions::from_mode(0o700))
        .map_err(|_| Error("update_write_failed"))?;
    let staged = staging.0.join("everycli");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&staged)
        .map_err(|_| Error("update_write_failed"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error("update_write_failed"))?;
    file.set_permissions(fs::Permissions::from_mode(0o755))
        .map_err(|_| Error("update_write_failed"))?;
    let current = fs::symlink_metadata(target).map_err(|_| Error("installed_binary_changed"))?;
    need(
        current.dev() == old.dev() && current.ino() == old.ino() && current.is_file(),
        "installed_binary_changed",
    )?;
    fs::rename(&staged, target).map_err(|_| Error("update_replace_failed"))?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error("update_directory_sync_failed"))?;
    Ok(())
}

pub fn run(check_only: bool, json_mode: bool) -> Result<Value> {
    let stderr = io::stderr();
    let mut progress = Progress::new(stderr.lock(), !json_mode, stderr.is_terminal());
    let result = run_with_progress(check_only, &mut |event| progress.event(event));
    // End an in-place line even if a download failed. JSON mode emits nothing
    // here; main_entry owns its single structured success/error response.
    progress.finish();
    result
}

fn run_with_progress(check_only: bool, progress: &mut dyn FnMut(ProgressEvent)) -> Result<Value> {
    progress(ProgressEvent::Stage("Checking for updates…".into()));
    let client = Client::builder()
        .no_proxy()
        .https_only(true)
        .user_agent(concat!("everycli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !trusted_url(attempt.url()) {
                attempt.error("untrusted release redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| Error("update_client_failed"))?;
    let release: Value = serde_json::from_slice(&download(&client, API, 1024 * 1024, &mut |_| {})?)
        .map_err(|_| Error("invalid_release_metadata"))?;
    need(
        release["draft"] == false && release["prerelease"] == false,
        "unstable_release",
    )?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or(Error("invalid_release_version"))?;
    let newer = version(tag)? > version(concat!("v", env!("CARGO_PKG_VERSION")))?;
    if check_only || !newer {
        return Ok(
            json!({"current":env!("CARGO_PKG_VERSION"),"latest":tag,"updateAvailable":newer,"updated":false}),
        );
    }
    let name = asset(std::env::consts::OS, std::env::consts::ARCH)?;
    let base = format!("{REPO}/releases/download/{tag}/{name}");
    progress(ProgressEvent::Stage(format!(
        "Update available: {} → {}",
        env!("CARGO_PKG_VERSION"),
        tag.trim_start_matches('v')
    )));
    progress(ProgressEvent::Stage("Fetching release checksum…".into()));
    let checksum = download(&client, &format!("{base}.sha256"), 1024, &mut |_| {})?;
    progress(ProgressEvent::Stage(format!(
        "Downloading everycli {} ({name})…",
        tag.trim_start_matches('v')
    )));
    let bytes = download(&client, &base, 64 * 1024 * 1024, progress)?;
    progress(ProgressEvent::Stage(format!(
        "Download complete: {:.2} MiB. Verifying checksum…",
        bytes.len() as f64 / (1024. * 1024.)
    )));
    verify(
        &bytes,
        std::str::from_utf8(&checksum).map_err(|_| Error("invalid_release_checksum"))?,
        &name,
    )?;
    need(!bytes.is_empty(), "empty_release_binary")?;
    let target = std::env::current_exe().map_err(|_| Error("installed_binary_unavailable"))?;
    progress(ProgressEvent::Stage("Installing update…".into()));
    replace_binary(&target, &bytes)?;
    Ok(
        json!({"previous":env!("CARGO_PKG_VERSION"),"version":tag,"updated":true,"message":"CLI updated. Miner profiles and deployed workers are unchanged."}),
    )
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn streaming_reports_real_bytes_and_handles_unknown_sizes() {
        let input = vec![42; 100_000];
        for total in [Some(input.len() as u64), None] {
            let mut events = vec![];
            let bytes =
                read_download(input.as_slice(), total, 200_000, &mut |e| events.push(e)).unwrap();
            assert_eq!(bytes, input);
            assert_eq!(
                events.first(),
                Some(&ProgressEvent::Download { received: 0, total })
            );
            assert_eq!(
                events.last(),
                Some(&ProgressEvent::Download {
                    received: 100_000,
                    total
                })
            );
            assert!(events.len() > 3);
        }
    }

    #[test]
    fn streaming_rejects_oversize_truncated_and_failed_reads() {
        for (input, total, limit, code) in [
            (b"1234".as_slice(), Some(4), 3, "release_asset_too_large"),
            (b"1234".as_slice(), None, 3, "release_asset_too_large"),
            (b"12".as_slice(), Some(4), 4, "release_download_failed"),
        ] {
            assert_eq!(
                read_download(input, total, limit, &mut |_| {})
                    .unwrap_err()
                    .0,
                code
            );
        }
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("synthetic private URL must not escape"))
            }
        }
        let error = read_download(Broken, None, 100, &mut |_| {}).unwrap_err();
        assert_eq!(error.0, "release_download_failed");
        assert!(!error.to_string().contains("private URL"));
    }

    #[test]
    fn human_progress_is_readable_and_json_mode_is_completely_silent() {
        for terminal in [false, true] {
            for enabled in [false, true] {
                let mut progress = Progress::new(Vec::new(), enabled, terminal);
                progress.event(ProgressEvent::Stage("Checking for updates…".into()));
                for received in [0, 1, 10, 50, 100] {
                    progress.event(ProgressEvent::Download {
                        received,
                        total: Some(100),
                    });
                }
                progress.event(ProgressEvent::Stage("Verifying checksum…".into()));
                progress.event(ProgressEvent::Stage("Installing update…".into()));
                progress.finish();
                let output = String::from_utf8(progress.writer).unwrap();
                if !enabled {
                    assert!(output.is_empty());
                    continue;
                }
                assert!(output.contains("Checking for updates") && output.contains("100%"));
                assert!(
                    output.contains("Verifying checksum") && output.contains("Installing update")
                );
                assert_eq!(output.contains('\r'), terminal);
                assert!(!output.contains('{') && !output.contains('\x1b'));
            }
        }
    }

    #[test]
    fn unknown_size_never_invents_percent_and_failed_terminal_line_is_finished() {
        let mut progress = Progress::new(Vec::new(), true, true);
        progress.event(ProgressEvent::Download {
            received: 2 * 1024 * 1024,
            total: None,
        });
        progress.finish();
        let output = String::from_utf8(progress.writer).unwrap();
        assert!(output.contains("2.00 MiB (size unknown)"));
        assert!(!output.contains('%'));
        assert!(output.ends_with('\n'));
    }
}
