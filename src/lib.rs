pub mod cli;
pub mod cloud;
pub mod hotkey;
pub mod invitation;
pub mod miner;
pub mod models;
pub mod network;
pub mod protocol;
pub mod providers;
pub mod state;
pub mod update;
pub mod worker;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub &'static str);
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
