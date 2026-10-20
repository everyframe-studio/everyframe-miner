use crate::{
    Error, Result, models, n, need,
    network::{Http, Request},
    protocol::{self, Keys},
    providers::Provider,
    s,
};
use serde_json::{Value, json};
use std::{
    io::Read,
    os::unix::fs::FileTypeExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub trait Runtime {
    fn post(&self, path: &str, body: &Value) -> Result<Value>;
    fn quote(&self, data: &[u8]) -> Result<Value>;
    fn balances_enabled(&self) -> bool {
        false
    }
    fn balance_session(&self, _session: &Value, _keys: &Keys) {}
    fn now(&self) -> i64 {
        crate::now()
    }
    fn sleep(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms))
    }
}
pub struct Worker<R, P> {
    pub miner_id: String,
    pub coordinator_key: String,
    pub chain: Value,
    pub keys: Keys,
    pub session: Value,
    pub runtime: R,
    pub provider: P,
    /// Providers whose account refused a job for billing or credentials, and
    /// until when (ms) this worker stops offering their models. Without it a
    /// worker with an empty balance takes, and loses, one job after another.
    pub provider_pause: std::collections::HashMap<String, i64>,
}
/// Upload time for the finished output: three minutes, plus a second per
/// 250 KB above that (assumes at least 2 Mbit/s up), capped at ten minutes.
/// A fixed 180 s timed out on long 1080p clips after the provider was paid.
pub fn complete_timeout_secs(encoded_bytes: usize) -> u64 {
    (120 + encoded_bytes as u64 / 250_000).clamp(180, 600)
}
/// How long a billing or credential refusal keeps a provider's models off the claim.
pub const PROVIDER_PAUSE_MS: i64 = 15 * 60 * 1000;
/// Refusals that mean the account itself can't pay or authenticate right now.
pub fn pauses_provider(category: &str) -> bool {
    matches!(
        category,
        "provider_billing_required" | "provider_auth_rejected" | "provider_key_required"
    )
}
/// The coordinator refused the uploaded output itself (validation). The same
/// bytes will be refused again, so retrying only burns the job's deadline.
pub fn output_refused(code: &str) -> bool {
    matches!(code, "http_400" | "http_413" | "http_422")
}
impl<R: Runtime, P: Provider> Worker<R, P> {
    pub fn enroll(&mut self) -> Result<()> {
        let challenge = protocol::verified(
            &self.runtime.post("/v1/challenge", &json!({}))?,
            &self.coordinator_key,
        )?;
        if self.chain["network"] == "finney" || challenge.get("chain").is_some() {
            need(challenge["chain"] == self.chain, "challenge_chain_mismatch")?
        };
        need(
            challenge["minerId"] == self.miner_id && n(&challenge["expires"])? > self.runtime.now(),
            "invalid_coordinator_challenge",
        )?;
        let mut binding = json!({"protocol":1,"minerId":self.miner_id,"challengeId":challenge["id"],"nonce":challenge["nonce"],"signingKey":self.keys.signing_key(),"encryptionKey":self.keys.encryption_key()});
        if self.chain["network"] == "finney" {
            binding["chain"] = self.chain.clone()
        };
        let evidence = self.runtime.quote(&protocol::report_data(&binding)?)?;
        let session=protocol::verified(&self.runtime.post("/v1/attest",&json!({"challengeId":challenge["id"],"signingKey":binding["signingKey"],"encryptionKey":binding["encryptionKey"],"evidence":evidence}))?,&self.coordinator_key)?;
        need(
            session["challengeId"] == challenge["id"]
                && session["signingKey"] == self.keys.signing_key()
                && n(&session["expires"])? > self.runtime.now(),
            "invalid_session",
        )?;
        self.session = session;
        Ok(())
    }
    pub fn call(&self, action: &str, payload: Value) -> Result<(Value, String)> {
        let nonce = protocol::id();
        let envelope = protocol::signed(
            &json!({"sessionId":self.session["sessionId"],"nonce":nonce,"time":self.runtime.now(),"action":action,"payload":payload}),
            &self.keys.signing,
        )?;
        let response = protocol::verified(
            &self.runtime.post("/v1/action", &envelope)?,
            &self.coordinator_key,
        )?;
        need(
            response["sessionId"] == self.session["sessionId"] && response["requestNonce"] == nonce,
            "response_replayed",
        )?;
        Ok((response["result"].clone(), nonce))
    }
    pub fn claim(&mut self) -> Result<Value> {
        // Refresh before enrollment/claim, never during a paid generation.
        let availability = self.provider.refresh_availability(self.runtime.now());
        if self.session.is_null()
            || n(&self.session["expires"])? < self.runtime.now() + models::max_job_ms() + 20000
        {
            self.enroll()?
        };
        let now = self.runtime.now();
        self.provider_pause.retain(|_, until| *until > now);
        let models: Vec<String> = self
            .provider
            .models()
            .into_iter()
            .filter(|id| {
                let provider = models::info(id)
                    .ok()
                    .and_then(|m| m["provider"].as_str())
                    .unwrap_or("");
                !self.provider_pause.contains_key(provider)
            })
            .collect();
        let mut capabilities = json!({"models":models,"durationPolicy":1});
        if let Some(rows) = availability {
            capabilities["modelAvailability"] = rows;
        }
        if self.runtime.balances_enabled() {
            capabilities["balancePolicy"] = json!(1);
        }
        let (result, nonce) = self.call("claim", capabilities)?;
        if self.runtime.balances_enabled() {
            self.runtime.balance_session(&self.session, &self.keys);
        }
        if result["work"].is_null() {
            return Ok(Value::Null);
        };
        let work = protocol::unseal(
            &result["work"],
            &self.keys.encryption,
            &format!("{}:{nonce}", s(&self.session["sessionId"])?),
        )?;
        models::check(&work["spec"])?;
        need(
            work["minerId"] == self.miner_id
                && protocol::digest(&work["spec"])? == work["inputHash"]
                && n(&work["deadline"])? > self.runtime.now(),
            "invalid_work",
        )?;
        Ok(work)
    }
    pub fn step(&mut self) -> Result<Value> {
        let work = self.claim()?;
        if work.is_null() {
            return Ok(json!({"state":"idle"}));
        };
        let attempt = &work["attemptId"];
        if work["state"] == "starting" {
            self.call("unknown", json!({"attemptId":attempt}))?;
            return Ok(json!({"state":"unknown"}));
        };
        if work["state"] == "unknown" {
            return Ok(json!({"state":"unknown"}));
        };
        let mut reference = work["providerRef"].clone();
        let mut grant = work["grantNonce"].clone();
        let deadline = n(&work["deadline"])?;
        if work["state"] == "offered" {
            let (start, _) = self.call("start", json!({"attemptId":attempt}))?;
            if start["permitted"] != true {
                return Ok(json!({"state":"reconcile"}));
            };
            need(
                start["attemptId"] == *attempt && start["inputHash"] == work["inputHash"],
                "invalid_start_grant",
            )?;
            grant = start["grantNonce"].clone();
            let submission = if self.runtime.now() >= deadline {
                Err(Error("submission_deadline_elapsed"))
            } else {
                self.provider.submit_before(&work["spec"], deadline)
            };
            reference = match submission {
                Ok(r) => json!(r),
                Err(error) => {
                    let rejected = crate::submission::rejected(error.0);
                    let reason = rejected.unwrap_or_else(|| crate::submission::uncertain(error.0));
                    let action = if rejected.is_some() {
                        "submission_rejected"
                    } else {
                        "submission_uncertain"
                    };
                    // Only fixed codes and public job/attempt identifiers; no keys,
                    // prompts, upstream bodies or credential-bearing URLs.
                    eprintln!(
                        "{}",
                        json!({"event":action,"jobId":work["jobId"],"attemptId":attempt,"reason":reason})
                    );
                    self.pause_provider_for(&work["spec"], reason);
                    while self.runtime.now() < deadline {
                        if self
                            .call(action, json!({"attemptId":attempt,"reason":reason}))
                            .is_ok()
                        {
                            return Ok(
                                json!({"state":if rejected.is_some(){"rejected"}else{"unknown"},"reason":reason}),
                            );
                        }
                        self.runtime.sleep(1000);
                    }
                    return Ok(json!({"state":"reconcile","reason":reason}));
                }
            };
            let mut recorded = false;
            while self.runtime.now() < deadline {
                if self
                    .call(
                        "submitted",
                        json!({"attemptId":attempt,"grantNonce":grant,"providerRef":reference}),
                    )
                    .is_ok()
                {
                    recorded = true;
                    break;
                };
                self.runtime.sleep(1000)
            }
            if !recorded {
                return Ok(json!({"state":"reconcile"}));
            }
        }
        let mut heartbeat = self.runtime.now();
        while self.runtime.now() < deadline {
            if self.runtime.now() - heartbeat > 15000 && self.call("heartbeat", json!({})).is_ok() {
                heartbeat = self.runtime.now()
            };
            let status = match self.provider.poll(s(&reference)?, &work["spec"]) {
                Ok(v) => v,
                Err(e) => {
                    if crate::submission::transient_poll(e.0) {
                        self.runtime.sleep(3000);
                        continue;
                    }
                    return self.fail_generation(&work, e.0, deadline);
                }
            };
            if status == "FAILED" {
                return self.fail_generation(&work, "provider_generation_failed", deadline);
            };
            if status != "COMPLETED" {
                self.runtime.sleep(1000);
                continue;
            };
            let output = match self.provider.result(s(&reference)?, &work["spec"]) {
                Ok(v) => v,
                Err(e) => {
                    if crate::submission::transient_poll(e.0) {
                        self.runtime.sleep(3000);
                        continue;
                    }
                    return self.fail_generation(&work, e.0, deadline);
                }
            };
            let value = json!({"protocol":1,"sessionId":self.session["sessionId"],"minerId":self.miner_id,"jobId":work["jobId"],"attemptId":attempt,"inputHash":work["inputHash"],"grantNonce":grant,"providerRef":reference,"outputHash":protocol::sha(&output.media),"bytes":output.media.len(),"providerResponseHash":output.response_hash});
            let receipt = protocol::signed(&value, &self.keys.signing)?;
            let payload = json!({"receipt":receipt,"video":protocol::b64(&output.media)});
            let mut refused = 0;
            while self.runtime.now() < deadline {
                match self.call("complete", payload.clone()) {
                    Ok((out, _))
                        if out["outputHash"] == value["outputHash"]
                            && out["jobId"] == value["jobId"] =>
                    {
                        return Ok(
                            json!({"state":"accepted","jobId":work["jobId"],"outputHash":value["outputHash"]}),
                        );
                    }
                    // Refused twice in a row: the output itself failed validation.
                    // Report it now so the job can move on, instead of resending
                    // the same bytes until the deadline.
                    Err(e) if output_refused(e.0) => {
                        refused += 1;
                        if refused >= 2 {
                            eprintln!(
                                "{}",
                                json!({"event":"output_refused","jobId":work["jobId"],"attemptId":attempt,"reason":e.0})
                            );
                            return self.fail_generation(
                                &work,
                                "output_refused_by_coordinator",
                                deadline,
                            );
                        }
                    }
                    // The attempt is no longer ours to complete (already accepted
                    // after a lost reply, reassigned, or the session ended).
                    Err(e) if matches!(e.0, "http_401" | "http_409") => {
                        return Ok(json!({"state":"reconcile","reason":e.0}));
                    }
                    _ => refused = 0,
                }
                self.runtime.sleep(1000)
            }
            return Ok(json!({"state":"reconcile"}));
        }
        Ok(json!({"state":"deadline_exceeded"}))
    }
    fn pause_provider_for(&mut self, spec: &Value, category: &str) {
        if !pauses_provider(category) {
            return;
        }
        if let Some(provider) = spec["model"]
            .as_str()
            .and_then(|id| models::info(id).ok())
            .and_then(|m| m["provider"].as_str())
        {
            eprintln!(
                "{}",
                json!({"event":"provider_paused","provider":provider,"reason":category,"minutes":PROVIDER_PAUSE_MS/60000})
            );
            self.provider_pause
                .insert(provider.into(), self.runtime.now() + PROVIDER_PAUSE_MS);
        }
    }
    fn fail_generation(&mut self, work: &Value, reason: &str, deadline: i64) -> Result<Value> {
        eprintln!(
            "{}",
            json!({"event":"generation_failed","jobId":work["jobId"],"attemptId":work["attemptId"],"reason":reason})
        );
        // Never downgrade to a reasonless report: transport errors (or a generic
        // HTTP 400) do not prove legacy compatibility, and losing a moderation
        // category can trigger an inappropriate paid fallback. An incompatible
        // coordinator must reconcile rather than accept a different outcome.
        let category = report_category(reason);
        self.pause_provider_for(&work["spec"], category);
        // Never repeat the provider POST or keep fetching a terminal result.
        while self.runtime.now() < deadline {
            if self
                .call(
                    "failed",
                    json!({"attemptId":work["attemptId"],"reason":category}),
                )
                .is_ok()
            {
                return Ok(json!({"state":"failed","reason":reason}));
            }
            self.runtime.sleep(3000);
        }
        Ok(json!({"state":"reconcile","reason":reason}))
    }
}
pub struct LiveRuntime {
    pub base: String,
    pub token: String,
    pub auth: Option<Value>,
    pub http: crate::network::PublicHttp,
    pub balance_session: Option<crate::worker_balances::SharedSession>,
}
impl Runtime for LiveRuntime {
    fn balances_enabled(&self) -> bool {
        self.balance_session.is_some()
    }
    fn balance_session(&self, session: &Value, keys: &Keys) {
        if let Some(shared) = &self.balance_session
            && let Ok(mut state) = shared.try_lock()
            && let (Some(id), Some(expires)) =
                (session["sessionId"].as_str(), session["expires"].as_i64())
        {
            *state = Some(crate::worker_balances::BalanceSession {
                id: id.into(),
                expires,
                signing: keys.signing.clone(),
            });
        }
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        need(
            ["/v1/challenge", "/v1/attest", "/v1/action"].contains(&path),
            "invalid_worker_path",
        )?;
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let host = url::Url::parse(&self.base)
            .map_err(|_| Error("invalid_release_url"))?
            .host_str()
            .ok_or(Error("invalid_release_url"))?
            .to_string();
        let mut r = Request::get(url, &[&host]).json("POST", body);
        r.limit = 300000;
        r.timeout = if body["value"]["action"] == "balances" {
            5
        } else if body["value"]["action"] == "complete" {
            complete_timeout_secs(
                body["value"]["payload"]["video"]
                    .as_str()
                    .map_or(0, str::len),
            )
        } else {
            30
        };
        if let Some(auth) = &self.auth {
            crate::hotkey::authorize(&mut r, auth, body)?;
        } else {
            r.headers
                .push(("authorization".into(), format!("Bearer {}", self.token)));
        }
        if let Some(session) = body["value"]["sessionId"].as_str() {
            r.headers.push(("x-session-id".into(), session.into()))
        };
        self.http.json(r)
    }
    fn quote(&self, data: &[u8]) -> Result<Value> {
        need(data.len() == 64, "invalid_report_data")?;
        let socket = "/var/run/dstack.sock";
        need(
            std::fs::metadata(socket).is_ok_and(|m| m.file_type().is_socket()),
            "real_dstack_socket_required",
        )?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .unix_socket(socket)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| Error("attestation_transport_failed"))?;
        let r = client
            .post("http://localhost/GetQuote")
            .json(&json!({"report_data":hex::encode(data)}))
            .send()
            .map_err(|_| Error("attestation_failed"))?;
        need(r.status().is_success(), "attestation_failed")?;
        let mut b = vec![];
        r.take(2_000_001)
            .read_to_end(&mut b)
            .map_err(|_| Error("attestation_failed"))?;
        need(b.len() <= 2_000_000, "attestation_response_too_large")?;
        let v: Value = serde_json::from_slice(&b).map_err(|_| Error("invalid_quote"))?;
        need(
            v["quote"].is_string() && v.get("event_log").is_some(),
            "invalid_quote",
        )?;
        Ok(json!({"quote":v["quote"],"event_log":v["event_log"]}))
    }
}
pub fn validate_release(release: &Value) -> Result<(url::Url, Value)> {
    need(
        release["release"] != "UNCONFIGURED",
        "release_not_configured",
    )?;
    protocol::public(s(&release["coordinatorSigningKey"])?, true)?;
    let base = url::Url::parse(s(&release["coordinatorUrl"])?)
        .map_err(|_| Error("invalid_release_url"))?;
    need(
        base.scheme() == "https"
            && ["/", "/mainnet/"].contains(&base.path())
            && base.query().is_none()
            && base.fragment().is_none()
            && base.username().is_empty()
            && base.password().is_none()
            && base.port_or_known_default() == Some(443),
        "invalid_release_url",
    )?;
    let chain = release["chain"].clone();
    if !chain.is_null() {
        let config: Value = serde_json::from_str(if chain["network"] == "finney" {
            include_str!("../config/mainnet.json")
        } else {
            include_str!("../config/testnet.json")
        })
        .map_err(|_| Error("invalid_release_chain"))?;
        need(
            chain["network"] == config["network"]
                && chain["netuid"] == config["netuid"]
                && chain["genesis"] == config["genesisHash"],
            "invalid_release_chain",
        )?
    };
    need(
        (chain["network"] == "finney") == (base.path() == "/mainnet/"),
        "release_endpoint_chain_mismatch",
    )?;
    Ok((base, chain))
}

pub fn run_live() -> Result<()> {
    let release: Value = serde_json::from_str(include_str!("../config/release.json"))
        .map_err(|_| Error("invalid_release"))?;
    let (base, chain) = validate_release(&release)?;
    for k in [
        "NODE_OPTIONS",
        "NODE_EXTRA_CA_CERTS",
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "SSLKEYLOGFILE",
        "DSTACK_SIMULATOR_ENDPOINT",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
    ] {
        need(
            std::env::var(k).unwrap_or_default().is_empty(),
            "unsafe_runtime_environment",
        )?
    }
    need(std::env::args().len() == 1, "unsafe_worker_arguments")?;
    need(
        std::fs::metadata("/var/run/dstack.sock").is_ok_and(|m| m.file_type().is_socket()),
        "real_dstack_socket_required",
    )?;
    let miner = std::env::var("MINER_ID").map_err(|_| Error("miner_id_required"))?;
    need(
        crate::matches("[a-zA-Z0-9_-]{1,64}", &json!(miner)),
        "miner_id_required",
    )?;
    let auth = std::env::var("MINER_AUTH")
        .ok()
        .map(|raw| serde_json::from_str::<Value>(&raw).map_err(|_| Error("invalid_worker_auth")))
        .transpose()?;
    let token = if auth.is_none() {
        let t = std::env::var("MINER_TOKEN").map_err(|_| Error("hotkey_worker_auth_required"))?;
        need(t.len() >= 32, "hotkey_worker_auth_required")?;
        t
    } else {
        String::new()
    };
    if let Some(a) = &auth {
        let c = &a["certificate"]["value"];
        need(
            c["minerId"] == miner
                && c["scope"] == "worker"
                && c["chain"] == chain
                && c["audience"] == base.to_string(),
            "wrong_worker_auth_identity",
        )?;
    }
    let mut credentials = json!({});
    for (_, key) in crate::invitation::PROVIDERS {
        if let Ok(v) = std::env::var(key) {
            credentials[key] = json!(v)
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())
            .map_err(|_| Error("signal_handler_failed"))?;
    }
    let http = crate::network::PublicHttp;
    let balance_session = Arc::new(std::sync::Mutex::new(None));
    let balance_runtime = LiveRuntime {
        base: base.to_string(),
        token: token.clone(),
        auth: auth.clone(),
        http: crate::network::PublicHttp,
        balance_session: None,
    };
    // A separately bounded thread: no new credentials, provider access, or
    // publishing authority beyond the exact admitted workload/session.
    let reporting = crate::worker_balances::start(
        credentials.clone(),
        balance_runtime,
        balance_session.clone(),
        s(&release["coordinatorSigningKey"])?.into(),
        stop.clone(),
    )
    .is_ok();
    if !reporting {
        eprintln!("balance_reporter_unavailable");
    }
    let mut w = Worker {
        miner_id: miner,
        coordinator_key: s(&release["coordinatorSigningKey"])?.into(),
        chain,
        keys: Keys::default(),
        session: Value::Null,
        provider_pause: Default::default(),
        runtime: LiveRuntime {
            base: base.to_string(),
            token,
            auth,
            http: crate::network::PublicHttp,
            balance_session: reporting.then_some(balance_session),
        },
        provider: crate::providers::Router::new(&http, credentials),
    };
    let mut last_log = String::new();
    let mut last_log_at = 0;
    while !stop.load(Ordering::Relaxed) {
        match w.step() {
            Ok(v) => {
                let signature = v.to_string();
                if signature != last_log || crate::now() - last_log_at >= 60000 {
                    println!("{}", json!({"event":"worker_cycle","result":v}));
                    last_log = signature;
                    last_log_at = crate::now();
                }
            }
            Err(e) => {
                if last_log != e.0 || crate::now() - last_log_at >= 60000 {
                    eprintln!("{}", json!({"event":"worker_cycle_failed","reason":e.0}));
                    last_log = e.0.into();
                    last_log_at = crate::now();
                }
            }
        };
        if !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2))
        }
    }
    Ok(())
}

/// Maps an internal failure code to a category the coordinator accepts.
pub fn report_category(reason: &str) -> &'static str {
    match reason {
        "provider_content_rejected" | "vivix_content_rejected" => "provider_content_rejected",
        "provider_invalid_request"
        | "vivix_invalid_parameters"
        | "http_400"
        | "http_403"
        | "http_422" => "provider_invalid_request",
        "provider_billing_required"
        | "vivix_billing_required"
        | "fal_billing_required"
        | "http_402" => "provider_billing_required",
        "provider_timeout" => "provider_timeout",
        "provider_unavailable" | "vivix_unavailable" | "provider_model_unavailable" => {
            "provider_unavailable"
        }
        r if r.starts_with("provider_no_media")
            || r.starts_with("output_")
            || matches!(r, "invalid_media" | "invalid_media_size") =>
        {
            "output_invalid"
        }
        _ => "provider_generation_failed",
    }
}
