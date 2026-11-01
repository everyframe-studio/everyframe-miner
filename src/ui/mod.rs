//! `everycli ui`: a local browser front end for setup and status.
//!
//! It drives the same `cli::run_with` dispatcher as the terminal, so the UI can
//! never do anything the CLI would refuse, and it never stores anything itself.
//!
//! Security model:
//! - Binds 127.0.0.1 only. Miners on a remote server use an SSH tunnel.
//! - Every /api request needs the session token in `x-everycli-token`. The page
//!   gets it by redeeming the single-use pairing code in the printed link, so a
//!   link that later shows up in a process list or shell history is useless.
//! - The Host header must be a loopback name (DNS rebinding), and state-changing
//!   requests need a loopback Origin plus a JSON body. No CORS headers are sent.
//! - Key values travel browser → this process → the owner-only credentials file.
//!   No response, job log or terminal line ever contains one.
//! - Paid steps keep the CLI's own question. The browser shows that exact text,
//!   and the command runs again only with that text's hash approved, so a quote
//!   that changed in between is asked again rather than silently accepted.
use crate::{
    Error, Result,
    cli::{self, Args},
    invitation,
    key_check::{self, Verdict},
    need,
    network::{Http, PublicHttp},
    now, onboarding, protocol,
    state::State,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const INDEX: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");
const APP_CSS: &str = include_str!("assets/app.css");
const ICON: &str = include_str!("assets/icon.svg");

pub const DEFAULT_PORT: u16 = 47117;
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const MAX_JOBS: usize = 200;

/// UI action → (CLI command words, extra flags, changes state or spends money).
/// Anything not listed here cannot be reached from the browser.
const ACTIONS: &[(&str, &[&str], &[&str], bool)] = &[
    ("status", &["status"], &[], false),
    ("health", &["health"], &[], false),
    ("doctor", &["doctor"], &[], false),
    ("providers", &["providers"], &[], false),
    ("balances", &["balances"], &[], false),
    ("offers", &["offers"], &[], false),
    ("earnings", &["earnings"], &[], false),
    ("jobs", &["jobs"], &[], false),
    ("register-hotkey", &["register-hotkey"], &[], true),
    (
        "worker-update-check",
        &["worker-update"],
        &["--check"],
        false,
    ),
    ("cli-update-check", &["update"], &["--check"], false),
    ("init", &["init"], &[], true),
    ("reconnect", &["reconnect"], &[], true),
    ("offer", &["offer"], &[], true),
    ("offer-batch", &["offer-batch"], &[], true),
    ("deploy", &["deploy"], &[], true),
    ("start", &["start"], &[], true),
    ("activate", &["activate"], &[], true),
    ("apply-api-keys", &["apply-api-keys"], &[], true),
    ("resume", &["resume"], &[], true),
    ("stop", &["stop"], &[], true),
    ("label", &["label"], &[], true),
    ("delete", &["delete"], &[], true),
    ("logs", &["logs"], &[], false),
    ("worker-update", &["worker-update"], &[], true),
    (
        "worker-update-abort",
        &["worker-update"],
        &["--abort"],
        true,
    ),
    ("worker-auto-status", &["worker-auto-update"], &[], false),
    (
        "worker-auto-enable",
        &["worker-auto-update"],
        &["--enable"],
        true,
    ),
    (
        "worker-auto-disable",
        &["worker-auto-update"],
        &["--disable"],
        true,
    ),
    ("reconcile", &["reconcile"], &[], true),
    ("remove-api-key", &["remove-api-key"], &[], true),
    ("balances-publish", &["balances"], &["--publish"], true),
];
/// Browser parameter → CLI option. Values are passed through `cli::parse`, which
/// applies the same per-command option allow-list as the terminal.
const PARAMS: &[(&str, &str)] = &[
    ("wallet", "--wallet"),
    ("hotkey", "--hotkey"),
    ("hotkeyFile", "--hotkey-file"),
    ("hotkeySs58", "--hotkey-ss58"),
    ("model", "--model"),
    ("models", "--models"),
    ("discountPct", "--discount-pct"),
    ("maxHourlyUsd", "--max-hourly-usd"),
    ("provider", "--provider"),
    ("name", "--name"),
    ("appId", "--app-id"),
    ("limit", "--limit"),
    ("cursor", "--cursor"),
    ("status", "--status"),
];
const FLAG_PARAMS: &[(&str, &str)] = &[
    ("withdraw", "--withdraw"),
    ("drainOnly", "--drain-only"),
    ("force", "--force"),
];

struct Job {
    action: String,
    request_hash: String,
    state: &'static str,
    log: Vec<String>,
    result: Value,
    started: i64,
}
pub struct Shared {
    token: String,
    pairing: Mutex<String>,
    pub network: String,
    pub state_dir: Option<String>,
    port: u16,
    jobs: Mutex<HashMap<String, Job>>,
    busy: Mutex<Option<String>>,
    http: Box<dyn Http + Send + Sync>,
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}
fn reply(status: u16, v: Value) -> Response {
    Response {
        status,
        content_type: "application/json",
        body: v.to_string().into_bytes(),
    }
}
fn asset(content_type: &'static str, body: &str) -> Response {
    Response {
        status: 200,
        content_type,
        body: body.as_bytes().to_vec(),
    }
}
fn error(status: u16, code: &str, message: &str) -> Response {
    reply(status, json!({"error":{"code":code,"message":message}}))
}
fn hex_id<const N: usize>() -> String {
    hex::encode(protocol::random::<N>())
}
pub fn question_hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl Shared {
    pub fn new(network: &str, state_dir: Option<&str>, port: u16) -> Self {
        Self {
            token: hex_id::<32>(),
            pairing: Mutex::new(hex_id::<16>()),
            network: network.into(),
            state_dir: state_dir.map(str::to_string),
            port,
            jobs: Mutex::new(HashMap::new()),
            busy: Mutex::new(None),
            http: Box::new(PublicHttp),
        }
    }
    /// Replaces the client used for API key checks (tests use a fixed answer).
    pub fn with_http(mut self, http: Box<dyn Http + Send + Sync>) -> Self {
        self.http = http;
        self
    }
    pub fn pairing_link(&self) -> String {
        format!(
            "http://127.0.0.1:{}/#pair={}",
            self.port,
            self.pairing.lock().unwrap()
        )
    }
    #[doc(hidden)]
    pub fn pairing_code_for_tests(&self) -> String {
        self.pairing.lock().unwrap().clone()
    }
    fn state(&self) -> Result<State> {
        State::new(&self.network, self.state_dir.as_deref())
    }
}

/// `host` or `host:port` where host is a loopback name.
fn loopback(authority: &str) -> bool {
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => authority,
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// Request checks shared by every route; pure so it can be tested directly.
pub fn admit(sh: &Shared, r: &Request) -> std::result::Result<(), Response> {
    let host = r.headers.get("host").map(String::as_str).unwrap_or("");
    if !loopback(host) {
        return Err(error(
            421,
            "ui_host_rejected",
            "Open the UI through 127.0.0.1 or localhost.",
        ));
    }
    if !r.path.starts_with("/api/") {
        return Ok(());
    }
    if let Some(site) = r.headers.get("sec-fetch-site")
        && !["same-origin", "none"].contains(&site.as_str())
    {
        return Err(error(403, "ui_cross_site", "Cross-site request refused."));
    }
    if r.method != "GET" {
        let origin = r.headers.get("origin").map(String::as_str).unwrap_or("");
        let local_origin = origin.strip_prefix("http://").is_some_and(loopback);
        let json_body = r
            .headers
            .get("content-type")
            .is_some_and(|t| t.starts_with("application/json"));
        if !local_origin || !json_body {
            return Err(error(403, "ui_origin_rejected", "Request refused."));
        }
    }
    if r.path == "/api/pair" {
        return Ok(());
    }
    let token = r
        .headers
        .get("x-everycli-token")
        .map(String::as_str)
        .unwrap_or("");
    if !same(token, &sh.token) {
        return Err(error(
            401,
            "ui_not_paired",
            "This page is not connected. Open the newest link printed by everycli ui.",
        ));
    }
    Ok(())
}

pub fn handle(sh: &Arc<Shared>, r: &Request) -> Response {
    if let Err(denied) = admit(sh, r) {
        return denied;
    }
    let body = || serde_json::from_slice::<Value>(&r.body).unwrap_or(Value::Null);
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => asset("text/html; charset=utf-8", INDEX),
        ("GET", "/app.js") => asset("text/javascript; charset=utf-8", APP_JS),
        ("GET", "/app.css") => asset("text/css; charset=utf-8", APP_CSS),
        ("GET", "/icon.svg") => asset("image/svg+xml", ICON),
        ("POST", "/api/pair") => pair(sh, &body()),
        ("GET", "/api/session") => reply(200, session(sh)),
        ("GET", "/api/overview") => match overview(sh) {
            Ok(v) => reply(200, v),
            Err(e) => reply(
                500,
                json!({"error":crate::cli_errors::diagnostic(&e, None)}),
            ),
        },
        ("GET", "/api/wallets") => reply(200, wallets()),
        ("GET", "/api/help") => reply(200, json!({"codes":crate::diagnostics::catalog()})),
        ("POST", "/api/keys") => save_key(sh, body()),
        ("POST", "/api/keys/check") => check_saved_key(sh, &body()),
        ("POST", "/api/run") => run(sh, &body()),
        ("GET", path) if path.starts_with("/api/jobs/") => job(sh, &path["/api/jobs/".len()..]),
        _ => error(404, "not_found", "Not found."),
    }
}

fn pair(sh: &Shared, body: &Value) -> Response {
    let mut code = sh.pairing.lock().unwrap();
    if !body["code"].as_str().is_some_and(|c| same(c, &code)) {
        return error(
            403,
            "ui_pairing_used",
            "This link was already used or is out of date. Open the newest link printed by everycli ui.",
        );
    }
    // Single use: the next tab or browser needs the next link.
    *code = hex_id::<16>();
    eprintln!(
        "Browser connected. To connect another tab or browser, open: http://127.0.0.1:{}/#pair={}",
        sh.port, code
    );
    reply(200, json!({"token":sh.token}))
}

fn session(sh: &Shared) -> Value {
    let netuid = if sh.network == "testnet" { 566 } else { 117 };
    json!({"version":env!("CARGO_PKG_VERSION"),"network":sh.network,"netuid":netuid,
        "stateDir":sh.state().ok().map(|s| s.directory.display().to_string()),
        "busy":*sh.busy.lock().unwrap()})
}

/// Local-only snapshot (no network): which setup steps are done and which keys
/// are saved. Never includes key values.
pub fn overview(sh: &Shared) -> Result<Value> {
    let state = sh.state()?;
    state.prepare()?;
    let registration = state.read("registration", true)?;
    let config = state.read("config", true)?;
    let credentials = state.read("credentials", true)?;
    let deployment = state.read("deployment", true)?;
    let label = state.read("label", true)?;
    let saved = |key: &str| invitation::configured(&credentials[key]);
    let keys: Vec<Value> = onboarding::credential_providers()
        .into_iter()
        .filter_map(|p| {
            let key = onboarding::credential_key(p).ok()?;
            let kind = if p == "phala" {
                "hosting"
            } else if crate::balances::BILLING_KEYS.iter().any(|(b, _)| *b == p) {
                "billing"
            } else {
                "generation"
            };
            Some(json!({"provider":p,"kind":kind,"saved":saved(key)}))
        })
        .collect();
    let generation = keys
        .iter()
        .any(|k| k["kind"] == "generation" && k["saved"] == true);
    let profile = if config.is_null() {
        Value::Null
    } else {
        let trust = invitation::trust(&sh.network)?;
        match invitation::validate(&config["invitation"], &trust, now(), true) {
            Ok(inv) => {
                json!({"minerId":inv["minerId"],"hotkey":inv["hotkey"],"release":inv["release"],
                "workerName":inv["request"]["name"],"configurationExpiresAt":inv["expiresAt"]})
            }
            Err(e) => json!({"error":e.0}),
        }
    };
    let phase = deployment["phase"].as_str().unwrap_or("not_deployed");
    // A deleted worker no longer counts; deploy can create a new one.
    let deployed = deployment["appId"].is_string() && phase != "deleted";
    let activated = [
        "awaiting_readmission",
        "running",
        "drained",
        "draining",
        "stopping",
        "stopped",
        "starting",
        "updating",
    ]
    .contains(&phase);
    let steps = json!([
        {"id":"hotkey","done":registration["registered"] == true || !config.is_null()},
        {"id":"phala","done":saved("PHALA_CLOUD_API_KEY")},
        {"id":"providers","done":generation},
        {"id":"init","done":!config.is_null()},
        {"id":"deploy","done":deployed && !phase.ends_with("_intent")},
        {"id":"activate","done":activated},
        {"id":"offers","done":Value::Null},
        {"id":"resume","done":phase == "running"},
    ]);
    Ok(json!({
        "network":sh.network,
        "registration": if registration.is_null() { Value::Null } else {
            json!({"hotkey":registration["hotkey"],"registered":registration["registered"],"network":registration["network"],"wallet":registration["wallet"],"uid":registration["uid"],"checkedAt":registration["checkedAt"]})
        },
        // Expose only whether a saved signer exists, never its private path.
        "hotkeyAttached": registration["hotkeyFile"].is_string()
            && (profile.is_null() || registration["hotkey"] == profile["hotkey"]),
        "initialized":!config.is_null(),"profile":profile,
        "deployment":{"phase":phase,"appId":if phase == "deleted" { Value::Null } else { deployment["appId"].clone() },
            "computeHourlyUsd":deployment["computeHourlyUsd"],
            // Deletion is confirmed by re-running delete, not by reconcile.
            "needsReconcile":(phase.ends_with("_intent") && phase != "delete_intent") || ["provisioned","starting","stopping","updating"].contains(&phase)},
        "label":label["name"],
        "recoveryPending":!state.read("profile-recovery", true)?.is_null(),
        "keys":keys,"steps":steps,"checkedAt":now()
    }))
}

/// Wallet and hotkey names under ~/.bittensor/wallets with their public address.
/// Secret material is read only to find the public address, never returned.
fn names(dir: &std::path::Path) -> Vec<String> {
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 80
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| ok(n))
        .collect();
    v.sort();
    v.truncate(100);
    v
}

pub fn wallets() -> Value {
    let Ok(home) = std::env::var("HOME") else {
        return json!({"wallets":[],"root":Value::Null});
    };
    let root = PathBuf::from(home).join(".bittensor/wallets");
    let mut out = vec![];
    for wallet in names(&root) {
        let mut hotkeys = vec![];
        for hotkey in names(&root.join(&wallet).join("hotkeys")) {
            let path = root.join(&wallet).join("hotkeys").join(&hotkey);
            match onboarding::file_address(&path) {
                Ok(address) => hotkeys.push(json!({"name":hotkey,"address":address})),
                Err(e) => hotkeys.push(json!({"name":hotkey,"error":e.0})),
            }
        }
        out.push(json!({"name":wallet,"hotkeys":hotkeys}));
    }
    json!({"wallets":out,"root":root.display().to_string()})
}

fn save_key(sh: &Shared, mut body: Value) -> Response {
    let provider = body["provider"].as_str().unwrap_or("").to_string();
    let key = zeroize::Zeroizing::new(body["key"].as_str().unwrap_or("").trim().to_string());
    if let Some(k) = body.get_mut("key") {
        *k = Value::Null;
    }
    let result = (|| {
        let name = onboarding::credential_key(&provider)?;
        need(!key.is_empty(), "empty_credential")?;
        need(key.len() >= 10, "credential_too_short")?;
        need(
            key.len() <= 8000 && !key.contains(['\r', '\n', '\0']),
            "invalid_credential",
        )?;
        Ok(name)
    })();
    let name = match result {
        Ok(name) => name,
        Err(e) => {
            return reply(
                400,
                json!({"error":crate::cli_errors::diagnostic(&e, None)}),
            );
        }
    };
    // Ask the provider first; a key it refuses is never saved.
    let (verdict, reason) = key_check::check(sh.http.as_ref(), &provider, &key);
    if verdict == Verdict::Rejected {
        return reply(400, json!({"error":rejected(&provider, reason)}));
    }
    let result = (|| {
        let state = sh.state()?;
        state.prepare()?;
        let mut update = serde_json::Map::new();
        update.insert(name.to_string(), json!(key.as_str()));
        onboarding::save_keys(&state, &Value::Object(update), None)
    })();
    match result {
        Ok(v) => reply(
            200,
            json!({"result":v,"check":key_check::verdict_json(verdict, reason)}),
        ),
        Err(e) => reply(
            400,
            json!({"error":crate::cli_errors::diagnostic(&e, None)}),
        ),
    }
}

fn rejected(provider: &str, reason: &str) -> Value {
    let (message, next) = if reason == "key_not_admin" {
        (
            "fal accepted this key, but it is not an admin key, so it can't read your balance.",
            "Create an ADMIN key in the fal dashboard for balance checks. Your generation key stays as it is.",
        )
    } else {
        (
            "The provider rejected this key, so it was not saved.",
            "Copy the whole key again from the provider's dashboard. A revoked key, a key from a different product, or a key missing permissions is refused.",
        )
    };
    json!({"ok":false,"error":"credential_rejected","provider":provider,"message":message,"next":next})
}

/// Re-checks a saved key without returning it.
fn check_saved_key(sh: &Shared, body: &Value) -> Response {
    let provider = body["provider"].as_str().unwrap_or("");
    let found = (|| {
        let name = onboarding::credential_key(provider)?;
        let credentials = sh.state()?.read("credentials", true)?;
        let key = credentials[name]
            .as_str()
            .filter(|k| invitation::configured(&json!(k)))
            .ok_or(Error("credential_not_saved"))?;
        Ok::<_, Error>(zeroize::Zeroizing::new(key.to_string()))
    })();
    match found {
        Ok(key) => {
            let (verdict, reason) = key_check::check(sh.http.as_ref(), provider, &key);
            if verdict == Verdict::Rejected {
                let mut e = rejected(provider, reason);
                e["message"] = json!("The provider rejected the saved key.");
                return reply(
                    200,
                    json!({"check":key_check::verdict_json(verdict, reason),"error":e}),
                );
            }
            reply(
                200,
                json!({"check":key_check::verdict_json(verdict, reason)}),
            )
        }
        Err(e) => reply(
            400,
            json!({"error":crate::cli_errors::diagnostic(&e, None)}),
        ),
    }
}

/// Build CLI arguments for a UI action through the CLI's own parser.
pub fn action_args(sh: &Shared, action: &str, params: &Value) -> Result<(Args, bool)> {
    let (_, words, flags, changes) = ACTIONS
        .iter()
        .find(|(a, ..)| *a == action)
        .ok_or(Error("ui_unknown_action"))?;
    let mut argv: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    argv.extend(flags.iter().map(|f| f.to_string()));
    // The CLI self-update check is profile-independent and takes no profile options.
    if words != &["update"] {
        argv.extend(["--network".into(), sh.network.clone()]);
        if let Some(dir) = &sh.state_dir {
            argv.extend(["--state-dir".into(), dir.clone()]);
        }
    }
    if let Some(object) = params.as_object() {
        for (name, value) in object {
            if let Some((_, option)) = PARAMS.iter().find(|(p, _)| p == name) {
                let text = match value {
                    Value::String(s) => s.trim().to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::Null => continue,
                    _ => return Err(Error("invalid_option_value").option(option)),
                };
                if text.is_empty() {
                    continue;
                }
                argv.extend([option.to_string(), text]);
            } else if let Some((_, flag)) = FLAG_PARAMS.iter().find(|(p, _)| p == name) {
                if value == &json!(true) {
                    argv.push(flag.to_string());
                }
            } else {
                return Err(Error("invalid_option").option(name));
            }
        }
    }
    let args = cli::parse(&argv)?;
    cli::validate_args(&args)?;
    Ok((args, *changes))
}

fn run(sh: &Arc<Shared>, body: &Value) -> Response {
    let action = body["action"].as_str().unwrap_or("").to_string();
    let (args, changes) = match action_args(sh, &action, &body["params"]) {
        Ok(v) => v,
        Err(e) => {
            return reply(
                400,
                json!({"error":crate::cli_errors::diagnostic(&e, None)}),
            );
        }
    };
    let approvals: Vec<String> = body["approvals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .take(8)
        .collect();
    // A browser chooses its operation ID before submission, so a lost HTTP
    // response can be reconciled using GET without resubmitting a mutation.
    let id = match body.get("requestId") {
        Some(Value::String(id)) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => {
            id.clone()
        }
        Some(_) => return error(400, "ui_invalid_request_id", "Invalid operation ID."),
        None => hex_id::<16>(),
    };
    let request_hash = hex::encode(Sha256::digest(
        json!({"action": action, "params": body["params"], "approvals": approvals})
            .to_string()
            .as_bytes(),
    ));
    let mut jobs = sh.jobs.lock().unwrap();
    if let Some(previous) = jobs.get(&id) {
        return if previous.request_hash == request_hash {
            reply(202, json!({"job":id}))
        } else {
            error(
                409,
                "ui_request_id_conflict",
                "This operation ID was already used for a different request.",
            )
        };
    }
    if changes {
        let mut busy = sh.busy.lock().unwrap();
        if let Some(running) = busy.as_ref() {
            return error(
                409,
                "ui_operation_in_progress",
                &format!("Wait for '{running}' to finish before starting another change."),
            );
        }
        *busy = Some(action.clone());
    }
    {
        if jobs.len() >= MAX_JOBS {
            let oldest: Vec<String> = {
                let mut finished: Vec<_> = jobs
                    .iter()
                    .filter(|(_, j)| j.state != "running")
                    .map(|(k, j)| (j.started, k.clone()))
                    .collect();
                finished.sort();
                finished
                    .into_iter()
                    .take(MAX_JOBS / 2)
                    .map(|(_, k)| k)
                    .collect()
            };
            for k in oldest {
                jobs.remove(&k);
            }
        }
        jobs.insert(
            id.clone(),
            Job {
                action: action.clone(),
                request_hash,
                state: "running",
                log: vec![],
                result: Value::Null,
                started: now(),
            },
        );
    }
    drop(jobs);
    let shared = Arc::clone(sh);
    let job_id = id.clone();
    std::thread::spawn(move || {
        let (state, result) = execute(&shared, &job_id, &args, &approvals);
        if let Some(job) = shared.jobs.lock().unwrap().get_mut(&job_id) {
            job.state = state;
            job.result = result;
        }
        if changes {
            *shared.busy.lock().unwrap() = None;
        }
    });
    reply(202, json!({"job":id}))
}

/// Runs one CLI command for the browser. A question the browser has not approved
/// stops the command before it spends and hands the exact text back.
pub fn execute(
    sh: &Shared,
    job_id: &str,
    args: &Args,
    approvals: &[String],
) -> (&'static str, Value) {
    let log = |line: String| {
        if let Some(job) = sh.jobs.lock().unwrap().get_mut(job_id)
            && job.log.len() < 500
        {
            job.log.push(line);
        }
    };
    let pending: Mutex<Option<(String, String)>> = Mutex::new(None);
    let confirm = |text: &str| {
        let hash = question_hash(text);
        if approvals.iter().any(|a| same(a, &hash)) {
            log(format!("Approved: {text}"));
            Ok(())
        } else {
            *pending.lock().unwrap() = Some((text.to_string(), hash));
            Err(Error("ui_confirmation_required"))
        }
    };
    let progress = |message: &str| log(message.to_string());
    match cli::run_with(args, &PublicHttp, &confirm, &progress) {
        Err(e) if e.0 == "ui_confirmation_required" => {
            let (text, hash) = pending.lock().unwrap().take().unwrap_or_default();
            ("confirm", json!({"question":text,"hash":hash}))
        }
        Ok(value) => {
            let failed = cli::result_failed(&args.command, &value);
            (
                if failed { "attention" } else { "done" },
                json!({"value":value,"text":crate::display::render(&args.command, &value)}),
            )
        }
        Err(e) => (
            "failed",
            json!({"error":crate::cli_errors::diagnostic(&e, Some(args))}),
        ),
    }
}

fn job(sh: &Shared, id: &str) -> Response {
    match sh.jobs.lock().unwrap().get(id) {
        Some(j) => reply(
            200,
            json!({"id":id,"action":j.action,"state":j.state,"log":j.log,"result":j.result,"startedAt":j.started}),
        ),
        None => error(404, "ui_job_not_found", "Unknown job."),
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEADER {
            return Ok(None);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_string();
    let target = first.next().unwrap_or("");
    let path = target.split(['?', '#']).next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Ok(None);
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

fn write_response(stream: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let reason = match r.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        421 => "Misdirected Request",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

fn serve_connection(sh: &Arc<Shared>, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let response = match read_request(&mut stream) {
        Ok(Some(r)) => handle(sh, &r),
        _ => error(400, "bad_request", "Bad request."),
    };
    let _ = write_response(&mut stream, &response);
}

fn open_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn serve(args: &Args) -> Result<Value> {
    let network = args.get("--network").unwrap_or("mainnet");
    invitation::trust(network)?;
    State::new(network, args.get("--state-dir"))?.prepare()?;
    let wanted: Option<u16> = args.get("--port").and_then(|p| p.parse().ok());
    let candidates: Vec<u16> = match wanted {
        Some(p) => vec![p],
        None => (DEFAULT_PORT..DEFAULT_PORT + 10).chain([0]).collect(),
    };
    let listener = candidates
        .iter()
        .find_map(|p| TcpListener::bind(("127.0.0.1", *p)).ok())
        .ok_or(Error("ui_port_unavailable").option("--port"))?;
    let port = listener
        .local_addr()
        .map_err(|_| Error("ui_port_unavailable"))?
        .port();
    let shared = Arc::new(Shared::new(network, args.get("--state-dir"), port));
    let link = shared.pairing_link();
    let remote =
        std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    eprintln!(
        "EveryFrame miner UI ({network}) is running on this computer only.\n\n  Open: {link}\n"
    );
    if remote {
        eprintln!(
            "You are on a remote server. On your own computer, run:\n  ssh -N -L {port}:127.0.0.1:{port} <user>@<this-server>\nthen open the link above in your browser.\n"
        );
    }
    eprintln!(
        "The link works once. Opening it prints a fresh link for another tab.\nPress Ctrl-C to stop the UI. Your worker keeps running; nothing is stopped by closing the UI.\n"
    );
    if !args.flag("--no-open") && !remote && std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        open_browser(&link);
    }
    // UI is long-running: the normal command-exit update hook is never reached.
    crate::auto_update::maybe_launch(args);
    for stream in listener.incoming().flatten() {
        let sh = Arc::clone(&shared);
        std::thread::spawn(move || serve_connection(&sh, stream));
    }
    Ok(json!({"stopped":true}))
}
