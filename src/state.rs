use crate::{Error, Result, need, now, protocol::id};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};
fn uid() -> u32 {
    unsafe { libc::getuid() }
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| Error("unsafe_path"))?
            .join(path)
    };
    let mut result = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(c),
        }
    }
    Ok(result)
}
pub fn no_symlink(path: &Path) -> Result<()> {
    for p in absolute(path)?.ancestors() {
        match fs::symlink_metadata(p) {
            Ok(m) => need(!m.file_type().is_symlink(), "unsafe_path")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error("unsafe_path")),
        }
    }
    Ok(())
}
pub fn read(path: &Path, secret: bool, limit: usize) -> Result<Vec<u8>> {
    no_symlink(path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error("file_unavailable"))?;
    let m = f.metadata().map_err(|_| Error("invalid_file"))?;
    need(m.is_file() && m.len() <= limit as u64, "invalid_file")?;
    if secret {
        need(m.uid() == uid() && m.mode() & 0o077 == 0, "insecure_file")?
    };
    let mut out = Vec::new();
    f.take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| Error("invalid_file"))?;
    need(out.len() <= limit, "invalid_file")?;
    Ok(out)
}
pub fn read_json(path: &Path, secret: bool) -> Result<Value> {
    serde_json::from_slice(&read(path, secret, 200000)?).map_err(|_| Error("invalid_json_file"))
}
pub struct State {
    pub directory: PathBuf,
}
impl State {
    pub fn new(network: &str, explicit: Option<&str>) -> Result<Self> {
        let home = std::env::var("HOME").map_err(|_| Error("home_required"))?;
        let path = explicit.map(str::to_string).unwrap_or_else(|| {
            if network == "mainnet" {
                format!("{home}/.config/everycli-mainnet117")
            } else {
                std::env::var("EVERYCLI_DIR").unwrap_or(format!("{home}/.config/everycli"))
            }
        });
        Ok(Self {
            directory: absolute(Path::new(&path))?,
        })
    }
    pub fn prepare(&self) -> Result<()> {
        need(
            self.directory != Path::new("/")
                && std::env::var("HOME").is_ok_and(|h| self.directory != Path::new(&h)),
            "unsafe_state_directory",
        )?;
        no_symlink(&self.directory)?;
        if !self.directory.exists() {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)
                .map_err(|_| Error("state_create_failed"))?
        };
        let m = fs::symlink_metadata(&self.directory).map_err(|_| Error("insecure_directory"))?;
        need(
            m.is_dir() && m.uid() == uid() && m.mode() & 0o077 == 0,
            "insecure_directory",
        )
    }
    pub fn path(&self, name: &str) -> Result<PathBuf> {
        need(
            ["config", "credentials", "deployment"].contains(&name),
            "invalid_state_file",
        )?;
        Ok(self.directory.join(format!("{name}.json")))
    }
    pub fn read(&self, name: &str, optional: bool) -> Result<Value> {
        let p = self.path(name)?;
        no_symlink(&p)?;
        if optional && !p.exists() {
            return Ok(Value::Null);
        }
        read_json(&p, true)
    }
    pub fn write(&self, name: &str, v: &Value) -> Result<()> {
        self.prepare()?;
        let dest = self.path(name)?;
        no_symlink(&dest)?;
        let temp = self.directory.join(format!("{name}.{}.tmp", id()));
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| Error("state_write_failed"))?;
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| Error("state_write_failed"))?;
        let data = serde_json::to_vec_pretty(v).map_err(|_| Error("invalid_state"))?;
        f.write_all(&data)
            .and_then(|_| f.write_all(b"\n"))
            .and_then(|_| f.sync_all())
            .map_err(|_| Error("state_write_failed"))?;
        fs::rename(temp, dest).map_err(|_| Error("state_write_failed"))?;
        File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error("state_write_failed"))
    }
    pub fn lock(&self) -> Result<Lock> {
        self.prepare()?;
        let path = self.directory.join("operation.lock");
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| Error("operation_locked"))?;
        let inode = f.metadata().map_err(|_| Error("lock_failed"))?.ino();
        f.write_all(
            json!({"pid":std::process::id(),"at":now()})
                .to_string()
                .as_bytes(),
        )
        .and_then(|_| f.sync_all())
        .map_err(|_| Error("lock_failed"))?;
        Ok(Lock {
            path,
            inode,
            _file: f,
        })
    }
}
pub struct Lock {
    path: PathBuf,
    inode: u64,
    _file: File,
}
impl Drop for Lock {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
