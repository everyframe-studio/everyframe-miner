//! Public CLI releases are separate from signed worker releases and credentials.
use crate::{Error, Result, need};
use reqwest::{blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};

const REPO: &str = "https://github.com/everyframe-studios/everyframe-miner";
const API: &str =
    "https://api.github.com/repos/everyframe-studios/everyframe-miner/releases/latest";

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

fn download(client: &Client, url: &str, limit: u64) -> Result<Vec<u8>> {
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
    need(
        response.content_length().is_none_or(|n| n <= limit),
        "release_asset_too_large",
    )?;
    let mut bytes = vec![];
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error("release_download_failed"))?;
    need(bytes.len() as u64 <= limit, "release_asset_too_large")?;
    Ok(bytes)
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

pub fn run(check_only: bool) -> Result<Value> {
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
    let release: Value = serde_json::from_slice(&download(&client, API, 1024 * 1024)?)
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
    let checksum = download(&client, &format!("{base}.sha256"), 1024)?;
    let bytes = download(&client, &base, 64 * 1024 * 1024)?;
    verify(
        &bytes,
        std::str::from_utf8(&checksum).map_err(|_| Error("invalid_release_checksum"))?,
        &name,
    )?;
    need(!bytes.is_empty(), "empty_release_binary")?;
    let target = std::env::current_exe().map_err(|_| Error("installed_binary_unavailable"))?;
    replace_binary(&target, &bytes)?;
    Ok(
        json!({"previous":env!("CARGO_PKG_VERSION"),"version":tag,"updated":true,"message":"CLI updated. Miner profiles and deployed workers are unchanged."}),
    )
}
