pub mod auto_update;
pub mod balances;
pub mod cli;
pub mod cli_errors;
pub mod cli_help;
pub mod cloud;
pub mod diagnostics;
pub mod display;
pub mod health;
pub mod hotkey;
pub mod invitation;
pub mod job_history;
pub mod key_check;
pub mod miner;
pub mod model_availability;
pub mod models;
pub mod network;
pub mod offer_catalog;
pub mod onboarding;
pub mod protocol;
pub mod providers;
pub mod reconnect;
pub mod state;
pub mod studio_media;
pub mod studio_video;
pub mod ui;
pub mod update;
pub mod worker;
pub mod worker_balances;
pub mod worker_update;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorContext {
    File(std::path::PathBuf),
    Option(String),
    Command(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub &'static str, pub Option<ErrorContext>);
pub type Error = Failure;
// Keep the existing code-only constructor; context is explicit and local, never
// copied from a provider response or credential value.
#[allow(non_snake_case)]
pub fn Error(code: &'static str) -> Error {
    Failure(code, None)
}
impl Failure {
    pub fn at_file(mut self, path: &std::path::Path) -> Self {
        self.1 = Some(ErrorContext::File(path.to_path_buf()));
        self
    }
    pub fn option(mut self, option: &str) -> Self {
        self.1 = Some(ErrorContext::Option(option.into()));
        self
    }
    pub fn command(mut self, command: &str) -> Self {
        self.1 = Some(ErrorContext::Command(command.into()));
        self
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Error {}
pub fn need(ok: bool, code: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(Error(code)) }
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn s(v: &serde_json::Value) -> Result<&str> {
    v.as_str().ok_or(Error("invalid_string"))
}
pub fn n(v: &serde_json::Value) -> Result<i64> {
    v.as_i64()
        .filter(|n| n.unsigned_abs() <= 9007199254740991)
        .ok_or(Error("invalid_integer"))
}
pub fn matches(pattern: &str, v: &serde_json::Value) -> bool {
    v.as_str().is_some_and(|s| {
        regex::Regex::new(&format!("^(?:{pattern})$")).is_ok_and(|r| r.is_match(s))
    })
}
pub mod submission;
