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
        if self.session.is_null()
            || n(&self.session["expires"])? < self.runtime.now() + models::max_job_ms() + 20000
        {
            self.enroll()?
        };
        let (result, nonce) = self.call(
            "claim",
            json!({"models":self.provider.models(),"durationPolicy":1}),
        )?;
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
            reference = match self.provider.submit(&work["spec"]) {
                Ok(r) => json!(r),
                Err(_) => {
                    self.call("unknown", json!({"attemptId":attempt}))?;
                    return Ok(json!({"state":"unknown"}));
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
                Err(_) => {
                    self.runtime.sleep(1000);
                    continue;
                }
            };
            if status == "FAILED" {
                self.call("failed", json!({"attemptId":attempt}))?;
                return Ok(json!({"state":"failed"}));
            };
            if status != "COMPLETED" {
                self.runtime.sleep(1000);
                continue;
            };
            let output = match self.provider.result(s(&reference)?, &work["spec"]) {
                Ok(v) => v,
                Err(_) => {
                    self.runtime.sleep(1000);
                    continue;
                }
            };
            let value = json!({"protocol":1,"sessionId":self.session["sessionId"],"minerId":self.miner_id,"jobId":work["jobId"],"attemptId":attempt,"inputHash":work["inputHash"],"grantNonce":grant,"providerRef":reference,"outputHash":protocol::sha(&output.media),"bytes":output.media.len(),"providerResponseHash":output.response_hash});
            let receipt = protocol::signed(&value, &self.keys.signing)?;
            let payload = json!({"receipt":receipt,"video":protocol::b64(&output.media)});
            while self.runtime.now() < deadline {
                if let Ok((out, _)) = self.call("complete", payload.clone())
                    && out["outputHash"] == value["outputHash"]
                    && out["jobId"] == value["jobId"]
                {
                    return Ok(
                        json!({"state":"accepted","jobId":work["jobId"],"outputHash":value["outputHash"]}),
                    );
                }
                self.runtime.sleep(1000)
            }
            return Ok(json!({"state":"reconcile"}));
        }
        Ok(json!({"state":"deadline_exceeded"}))
    }
}
pub struct LiveRuntime {
    pub base: String,
    pub token: String,
    pub auth: Option<Value>,
    pub http: crate::network::PublicHttp,
}
impl Runtime for LiveRuntime {
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
        r.timeout = 30;
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
    let mut w = Worker {
        miner_id: miner,
        coordinator_key: s(&release["coordinatorSigningKey"])?.into(),
        chain,
        keys: Keys::default(),
        session: Value::Null,
        runtime: LiveRuntime {
            base: base.to_string(),
            token,
            auth,
            http: crate::network::PublicHttp,
        },
        provider: crate::providers::Router::new(&http, credentials),
    };
    while !stop.load(Ordering::Relaxed) {
        match w.step() {
            Ok(v) => println!("{}", json!({"event":"worker_cycle","state":v["state"]})),
            Err(_) => eprintln!("worker_cycle_failed"),
        };
        if !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2))
        }
    }
    Ok(())
}
