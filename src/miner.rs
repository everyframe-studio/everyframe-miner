use crate::{
    Error, Result,
    cloud::{Cloud, compose_hash, encrypt},
    invitation::{self, PROVIDERS, configured},
    matches, n, need,
    network::{Http, Request},
    now,
    protocol::{canonical, id, sha, verified},
    s,
    state::{State, read},
};
use serde_json::{Value, json};
use std::path::Path;
pub const PENDING: &str = "Waiting for automatic workload attestation. No working provider key was released. Hosting continues until stopped. Run everycli status; once online with accepted attestation, run everycli activate.";

fn attestation_ready(status: &Value, at: i64) -> Result<()> {
    let a = &status["attestation"];
    match a["state"].as_str() {
        Some("accepted") => {}
        Some("pending") => return Err(Error("attestation_pending")),
        Some("rejected") => return Err(Error("attestation_rejected")),
        _ => return Err(Error("attestation_unavailable")),
    }
    let verified_at = n(&a["at"]).map_err(|_| Error("attestation_timestamp_invalid"))?;
    need(verified_at <= at + 30000, "attestation_clock_skew")?;
    need(verified_at > at - 600000, "attestation_stale")?;
    need(status["attested"] == true, "attestation_session_not_ready")?;
    need(status["online"] == true, "worker_offline")
}

fn readiness_detail(code: &str) -> String {
    let d = crate::diagnostics::value(code);
    format!(
        "{} {}",
        d["message"].as_str().unwrap(),
        d["next"].as_str().unwrap()
    )
}

pub fn profile_diagnostic(error: &Error) -> Value {
    let (state, message, next) = match error.0 {
        "profile_recovery_pending" => (
            "reconnect_pending",
            "An approved local profile reconnect was interrupted. No cloud mutation was requested by reconnect.",
            "Run everycli reconnect in this same profile to finish the local commit. Keep the profile and recovery journal intact; do not redeploy.",
        ),
        "profile_not_initialized" => (
            "not_initialized",
            "No local miner profile found for the selected network and OS user. Hotkey registration and linking have not been checked.",
            "Connect your hotkey: everycli init --wallet <wallet> --hotkey default. For a new miner, everycli setup guides you through all steps. If you already initialized here, use the original OS user and --state-dir/--network options.",
        ),
        "profile_credentials_missing" => (
            "profile_incomplete",
            "The local miner profile exists, but its credentials file is missing. This does not mean your hotkey is unregistered.",
            "Use the original OS user and profile, or reconnect with everycli init --wallet <wallet> --hotkey default. Preserve your existing profile and deployment records.",
        ),
        "file_permission_denied" | "insecure_file" | "insecure_directory" => (
            "profile_error",
            "The local miner profile cannot be read securely because of its ownership or permissions. Hotkey registration has not been checked.",
            "Use the OS user who initialized this profile. Profile directories must be owner-only (0700), and files owner-only (0600). Do not delete the profile or share credentials.",
        ),
        "invalid_json_file" | "invalid_file" => (
            "profile_error",
            "A local miner profile file is invalid or damaged. Hotkey registration has not been checked.",
            "Preserve the profile and restore a trusted backup, or reconnect using everycli init with your hotkey. Do not delete deployment records or share credentials.",
        ),
        "unsafe_path" => (
            "profile_error",
            "The local miner profile path is unsafe or inaccessible. Hotkey registration has not been checked.",
            "Use an owner-only profile directory without symbolic links; check your OS user and --state-dir option. Do not disable the security checks.",
        ),
        _ => (
            "profile_error",
            "The local miner profile could not be loaded or verified. Hotkey registration has not been checked.",
            "Check the diagnostic code and your OS user, network and profile selection. Preserve the existing profile; do not retry deployment or share credentials.",
        ),
    };
    let mut out = json!({"ok":false,"state":state,"error":error.0,"message":message,"registration":"not_checked","next":next});
    if let Some(crate::ErrorContext::File(path)) = &error.1 {
        out["file"] = json!(path.to_string_lossy());
    }
    out
}

pub fn read_secrets(path: &Path) -> Result<Value> {
    let data =
        String::from_utf8(read(path, true, 64000)?).map_err(|_| Error("invalid_credential"))?;
    let mut result = json!({});
    for line in data.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.is_empty() || line.starts_with('#') {
            continue;
        };
        let Some((k, v)) = line.split_once('=') else {
            return Err(Error("invalid_credential"));
        };
        let k = k.trim().trim_matches('\'');
        if ![
            "MINER_TOKEN",
            "PHALA_CLOUD_API_KEY",
            "DSTACK_DOCKER_USERNAME",
            "DSTACK_DOCKER_PASSWORD",
        ]
        .contains(&k)
            && !PROVIDERS.iter().any(|(_, key)| key == &k)
        {
            continue;
        };
        let v = v.trim();
        let mut parsed = String::new();
        if v.starts_with('"') || v.starts_with('\'') {
            let q = v.as_bytes()[0] as char;
            let mut chars = v[1..].char_indices();
            let mut end = None;
            while let Some((offset, ch)) = chars.next() {
                if ch == q {
                    end = Some(offset + 2);
                    break;
                }
                if ch == '\\' {
                    let (_, escaped) = chars.next().ok_or(Error("invalid_credential"))?;
                    match escaped {
                        '\\' => parsed.push('\\'),
                        ch if ch == q => parsed.push(ch),
                        'n' | 'r' if q == '"' => return Err(Error("invalid_credential")),
                        't' if q == '"' => parsed.push('\t'),
                        'a' if q == '"' => parsed.push('\u{7}'),
                        'b' if q == '"' => parsed.push('\u{8}'),
                        'f' if q == '"' => parsed.push('\u{c}'),
                        'v' if q == '"' => parsed.push('\u{b}'),
                        other => {
                            parsed.push('\\');
                            parsed.push(other);
                        }
                    }
                } else {
                    parsed.push(ch);
                }
            }
            let end = end.ok_or(Error("invalid_credential"))?;
            need(
                v[end..].trim().is_empty() || v[end..].trim().starts_with('#'),
                "invalid_credential",
            )?;
        } else {
            let end = v
                .char_indices()
                .find(|(i, ch)| *ch == '#' && *i > 0 && v[..*i].ends_with(char::is_whitespace))
                .map(|(i, _)| i)
                .unwrap_or(v.len());
            parsed = v[..end].trim_end().to_string();
        };
        let v = parsed;
        need(
            v.len() <= 8000 && !v.contains(['\r', '\n', '\0']),
            "invalid_credential",
        )?;
        if !v.is_empty() {
            result[k] = json!(v)
        }
    }
    Ok(result)
}
pub struct Context {
    pub config: Value,
    pub credentials: Value,
    pub invitation: Value,
    pub deployment: Value,
}
pub struct Miner<'a> {
    pub state: State,
    pub trust: Value,
    pub http: &'a dyn Http,
}
impl Miner<'_> {
    pub fn validate(&self, v: &Value, expired: bool) -> Result<Value> {
        invitation::validate(v, &self.trust, now(), expired)
    }
    pub fn token(creds: &Value, inv: &Value) -> Result<()> {
        if inv["authMode"] == "hotkey-v1" {
            crate::hotkey::validate(&creds["CONSOLE_AUTH"], inv, "console")?;
            crate::hotkey::validate(&creds["WORKER_AUTH"], inv, "worker")?;
            return Ok(());
        }
        let token = s(&creds["MINER_TOKEN"])?;
        need(
            token.len() >= 32 && sha(format!("Bearer {token}")) == inv["tokenHash"],
            "credential_mismatch",
        )
    }
    pub fn load(&self, current: bool) -> Result<Context> {
        self.state.prepare()?;
        need(
            self.state.read("profile-recovery", true)?.is_null(),
            "profile_recovery_pending",
        )?;
        let config = self.state.read("config", false).map_err(|e| {
            if e.0 == "file_not_found" {
                Error("profile_not_initialized").at_file(&self.state.directory.join("config.json"))
            } else {
                e
            }
        })?;
        let credentials = self.state.read("credentials", false).map_err(|e| {
            if e.0 == "file_not_found" {
                Error("profile_credentials_missing")
                    .at_file(&self.state.directory.join("credentials.json"))
            } else {
                e
            }
        })?;
        let invitation = self.validate(&config["invitation"], !current)?;
        Self::token(&credentials, &invitation)?;
        let d = self.state.read("deployment", true)?;
        Ok(Context {
            config,
            credentials,
            invitation,
            deployment: if d.is_null() { json!({}) } else { d },
        })
    }
    pub fn api(&self, c: &Context, action: &str, payload: Value) -> Result<Value> {
        need(
            [
                "status",
                "drain",
                "resume",
                "offer",
                "bind-app",
                "balances",
                "worker-update",
            ]
            .contains(&action),
            "unsupported_action",
        )?;
        let nonce = id();
        let base = s(&self.trust["coordinatorUrl"])?;
        let mut r = Request::get(
            format!("{}/v1/miner/{action}", base.trim_end_matches('/')),
            &["subnet.everyframe.studio"],
        );
        r.limit = 100000;
        let mut auth_body = Value::Null;
        if ["status", "worker-update"].contains(&action) {
            r.url += &format!("?nonce={nonce}")
        } else {
            let mut body = payload;
            body["nonce"] = json!(nonce);
            auth_body = body.clone();
            r = r.json("POST", &body)
        }
        if c.invitation["authMode"] == "hotkey-v1" {
            crate::hotkey::authorize(&mut r, &c.credentials["CONSOLE_AUTH"], &auth_body)?;
        } else {
            r.headers.push((
                "authorization".into(),
                format!("Bearer {}", s(&c.credentials["MINER_TOKEN"])?),
            ));
        }
        let response = self.http.json(r).map_err(|e| {
            if e.0 == "http_401" {
                Error("coordinator_auth_rejected")
            } else {
                e
            }
        })?;
        let out = verified(&response, s(&self.trust["publicKey"])?)?;
        need(
            out["protocol"] == 1
                && out["nonce"] == nonce
                && out["action"] == action
                && out["minerId"] == c.invitation["minerId"]
                && (n(&out["at"])? - now()).abs() < 30000
                && n(&out["activeJobs"])? >= 0
                && out["draining"].is_boolean(),
            "invalid_coordinator_response",
        )?;
        Ok(out)
    }
    fn cloud<'a>(&'a self, c: &Context) -> Result<Cloud<'a>> {
        Cloud::new(self.http, &c.credentials)
    }
    pub fn init(&self, envelope: &Value, creds: &Value) -> Result<Value> {
        let _lock = self.state.lock()?;
        let inv = self.validate(envelope, false)?;
        let registration = self.state.read("registration", true)?;
        if !registration.is_null() {
            need(
                registration["hotkey"] == inv["hotkey"]
                    && registration["network"] == inv["network"],
                "hotkey_does_not_match_deployment",
            )?;
        }
        let mut merged = self.state.read("credentials", true)?;
        if merged.is_null() {
            merged = json!({});
        }
        let object = merged
            .as_object_mut()
            .ok_or(Error("invalid_credentials_file"))?;
        for (k, v) in creds.as_object().ok_or(Error("invalid_credential"))? {
            object.insert(k.clone(), v.clone());
        }
        if inv["authMode"] == "hotkey-v1" {
            object.remove("MINER_TOKEN");
        }
        Self::token(&merged, &inv)?;
        need(
            compose_hash(&inv["request"]["compose_file"])? == inv["composeHash"],
            "compose_hash_mismatch",
        )?;
        let old = self.state.read("config", true)?;
        if !old.is_null() {
            let mut before = self.validate(&old["invitation"], true)?;
            let mut after = inv.clone();
            for v in [&mut before, &mut after] {
                v.as_object_mut().unwrap().remove("issuedAt");
                v.as_object_mut().unwrap().remove("expiresAt");
            }
            need(
                canonical(&before)? == canonical(&after)?,
                "already_initialized",
            )?;
            let d = self.state.read("deployment", true)?;
            need(
                !d["phase"].as_str().unwrap_or("").ends_with("_intent"),
                "reconciliation_required",
            )?
        };
        self.state.write("credentials", &merged)?;
        self.state.write("config",&json!({"version":1,"invitation":envelope,"initializedAt":old.get("initializedAt").cloned().unwrap_or(json!(now()))}))?;
        Ok(
            json!({"state":"configured","minerId":inv["minerId"],"network":inv["network"],"netuid":inv["netuid"],"hotkey":inv["hotkey"],"generationSubmitted":false,"next":"Run everycli doctor, then review hosting with everycli deploy --max-hourly-usd."}),
        )
    }
    pub fn target(&self, c: &Context, new: bool) -> Result<Value> {
        need(!c.deployment["appId"].is_null(), "deployment_not_recorded")?;
        need(
            matches("[0-9a-f]{40}", &c.deployment["appId"]),
            "invalid_deployment_app_id",
        )?;
        let app = s(&c.deployment["appId"])?;
        let info = self.cloud(c)?.info(app)?;
        need(
            info["app_id"] == app && info["name"] == c.invitation["request"]["name"],
            "target_mismatch",
        )?;
        let allowed = info["compose_hash"] == c.invitation["composeHash"]
            || (new
                && !c.deployment["pendingInvitation"].is_null()
                && info["compose_hash"]
                    == self.validate(&c.deployment["pendingInvitation"], true)?["composeHash"]);
        need(allowed, "unapproved_running_compose")?;
        Ok(info)
    }
    pub fn status(&self) -> Result<Value> {
        let c = match self.load(false) {
            Ok(c) => c,
            Err(e) => return Ok(profile_diagnostic(&e)),
        };
        let inv = &c.invitation;
        let d = &c.deployment;
        let mut out = json!({"minerId":inv["minerId"],"network":inv["network"],"netuid":inv["netuid"],"release":inv["release"],"phase":d["phase"].as_str().unwrap_or("not_deployed"),"appId":d["appId"],"invitationExpired":n(&inv["expiresAt"])?<=now()});
        out["label"] = self.state.read("label", true)?["name"].clone();
        out["signedVmName"] = inv["request"]["name"].clone();
        let auth = &c.credentials["WORKER_AUTH"]["certificate"]["value"];
        if !auth.is_null() {
            out["authorization"] = json!({"expiresAt":auth["expiresAt"],"nonExpiring":auth["kind"]=="everyframe-hotkey-delegation-v2","remainingDays":auth["expiresAt"].as_i64().map(|x|(x-now()).max(0)/86400000),"migration":"Legacy expiring delegates require everycli init with the hotkey, then everycli apply-api-keys to install the newly signed worker authorization."});
        }
        match self.api(&c, "status", json!({})) {
            Ok(v) => out["coordinator"] = v,
            Err(e) => {
                let mut diagnostic = crate::diagnostics::value(e.0);
                diagnostic["code"] = json!(e.0);
                out["coordinatorError"] = diagnostic;
            }
        };
        if !d["appId"].is_null() && !c.credentials["PHALA_CLOUD_API_KEY"].is_null() {
            match self.target(&c, true) {
                Ok(v) => {
                    out["cloud"] = json!({"status":v["status"],"composeHash":v["compose_hash"]})
                }
                Err(e) => {
                    out["cloudError"] = json!({"code":e.0,"message":crate::diagnostics::message(e.0).unwrap_or_else(|| e.0.into())})
                }
            }
        }
        let managed_here = d["appId"].is_string() && d["phase"] != "deleted";
        if !managed_here && configured(&c.credentials["PHALA_CLOUD_API_KEY"]) {
            match self.cloud_workloads(&c) {
                Ok(list) => out["cloudWorkloads"] = json!(list),
                Err(e) => out["cloudWorkloadsError"] = json!({"code":e.0}),
            }
        }
        out["next"] = json!(
            "Use everycli doctor for readiness. Reconcile uncertain operations; never blindly repeat a cloud mutation."
        );
        Ok(out)
    }
    /// Phala workloads carrying this miner's signed VM name. A workload listed here
    /// while this profile records none was deployed from another computer or an
    /// earlier setup; it can be deleted with `everycli delete --app-id ID`.
    fn cloud_workloads(&self, c: &Context) -> Result<Vec<Value>> {
        let catalog = self.cloud(c)?.catalog("/cvms/paginated")?;
        Ok(catalog["items"]
            .as_array()
            .ok_or(Error("invalid_cloud_response"))?
            .iter()
            .filter(|v| v["name"] == c.invitation["request"]["name"])
            .filter(|v| matches("[0-9a-f]{40}", &v["app_id"]))
            .take(20)
            .map(|v| {
                json!({"appId":v["app_id"],"name":v["name"],
                    "status":v["status"].as_str().filter(|s| s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')),
                    "sameRelease":v["compose_hash"] == c.invitation["composeHash"]})
            })
            .collect())
    }
    pub fn doctor(&self) -> Result<Value> {
        self.diagnose(false)
    }
    pub fn health(&self) -> Result<Value> {
        self.diagnose(true)
    }
    fn diagnose(&self, health: bool) -> Result<Value> {
        let c = match self.load(false) {
            Ok(c) => c,
            Err(e) => {
                let mut out = profile_diagnostic(&e);
                out["checks"] = json!([{"name":"Local miner profile","ok":false,"detail":e.0,"message":out["message"]}]);
                return Ok(out);
            }
        };
        let inv = &c.invitation;
        let providers = invitation::provider_summary(inv, &c.credentials);
        let mut snapshot = Value::Null;
        let mut checks = vec![];
        let mut add = |name: &str, ok: bool, detail: &str| {
            checks.push(json!({"name":name,"ok":ok,"detail":detail}))
        };
        add(
            "Rust runtime",
            true,
            "Native executable; no Node.js or Python required.",
        );
        add(
            "Profile and permissions",
            true,
            "Signature, token binding, and owner-only files checked.",
        );
        add(
            "Deployment configuration",
            n(&inv["expiresAt"])? > now(),
            "Expired configurations permit diagnosis/stop only; renew with everycli init.",
        );
        add(
            "Phala credential",
            configured(&c.credentials["PHALA_CLOUD_API_KEY"]),
            "Your Phala account pays hosting.",
        );
        add(
            "Wallet enrollment",
            true,
            "Signed enrollment verified; no blockchain transaction is signed.",
        );
        for provider in providers.as_array().unwrap() {
            if provider["configured"] == true {
                add(
                    &format!("Provider: {}", s(&provider["provider"])?),
                    provider["releaseAllows"] == true,
                    invitation::provider_check_detail(provider["releaseAllows"] == true),
                );
            }
        }
        add(
            "Provider credentials",
            providers
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["usable"] == true),
            "Presence only; not a balance check.",
        );
        add(
            "Registry",
            !invitation::registry_required(inv)
                || configured(&c.credentials["DSTACK_DOCKER_PASSWORD"]),
            "Public releases do not send registry credentials.",
        );
        add(
            "Operation journal",
            !c.deployment["phase"]
                .as_str()
                .unwrap_or("")
                .ends_with("_intent"),
            "Unknown operations require reconciliation.",
        );
        match self.api(&c, "status", json!({})) {
            Ok(v) => {
                snapshot = v.clone();
                add(
                    "Coordinator",
                    true,
                    "Signature, nonce, timestamp and miner identity verified.",
                );
                let enabled_detail = if v["enabled"] == true {
                    "Miner is enabled on the coordinator.".into()
                } else {
                    readiness_detail("miner_disabled")
                };
                add("Miner enabled", v["enabled"] == true, &enabled_detail);
                let attestation = attestation_ready(&v, now());
                add(
                    "Workload attestation",
                    attestation.is_ok(),
                    &match attestation {
                        Ok(()) => {
                            "Automatic attestation accepted; session and heartbeat are current."
                                .into()
                        }
                        Err(e) => readiness_detail(e.0),
                    },
                );
                add(
                    "Accepting new work",
                    v["routingEnabled"] == true && v["draining"] == false,
                    if v["routingEnabled"] != true {
                        "Coordinator routing is disabled or unreported. Inspect everycli status."
                    } else if v["draining"] != false {
                        "Miner is draining or paused. Review everycli health before choosing to resume provider-billed work."
                    } else {
                        "Routing enabled and miner is not draining."
                    },
                );
                if v["routingPolicy"] == "discount-lottery-v1" {
                    add(
                        "Current model bid",
                        v["offers"].as_array().is_some_and(|a| {
                            a.iter()
                                .any(|o| o["offered"] == true && o["pricingCurrent"] == true)
                        }),
                        "At least one active offer with current pricing is required. Use everycli offers to review bids and model readiness.",
                    )
                };
                if !c.deployment["appId"].is_null() {
                    add(
                        "Deployment binding",
                        v["appId"] == c.deployment["appId"]
                            && v["composeHash"] == inv["composeHash"],
                        "Exact workload policy required.",
                    )
                }
            }
            Err(e) => add("Coordinator", false, e.0),
        };
        let workload = self.target(&c, false);
        let workload_reason = workload
            .as_ref()
            .err()
            .and_then(|e| crate::diagnostics::message(e.0));
        match workload {
            Ok(v) => add(
                "Phala workload",
                v["status"] == "running",
                "Pinned app/name/compose checked.",
            ),
            Err(e) => add(
                "Phala workload",
                false,
                match e.0 {
                    "deployment_not_recorded"
                        if c.deployment.as_object().is_some_and(|d| d.is_empty()) =>
                    {
                        "No workload deployed in this local profile. For a new miner, next: everycli deploy --max-hourly-usd <limit>. Review the hosting quote; storage costs extra and this is not a total budget. If already deployed elsewhere, use the original management profile; do not redeploy."
                    }
                    "deployment_not_recorded" => {
                        "The local deployment record has no app ID; an interrupted operation may have created a paid workload. Preserve the profile and run everycli reconcile. Do not redeploy."
                    }
                    "invalid_deployment_app_id" => {
                        "The local deployment app ID is invalid. Preserve the profile and restore a trusted deployment record before cloud operations. Do not redeploy."
                    }
                    _ => workload_reason.as_deref().unwrap_or(e.0),
                },
            ),
        };
        if health {
            checks.extend(crate::health::runtime_checks(&snapshot, now()));
            Ok(json!({
                "ok":checks.iter().all(|c|c["ok"]==true),"checks":checks,
                "minerId":inv["minerId"],"network":inv["network"],"netuid":inv["netuid"],
                "checkedAt":now(),"activity":crate::health::activity(&snapshot),
                "generationSubmitted":false,"label":self.state.read("label",true)?["name"]
            }))
        } else {
            Ok(json!({"ok":checks.iter().all(|c|c["ok"]==true),"checks":checks}))
        }
    }
    pub fn providers(&self) -> Result<Value> {
        self.state.prepare()?;
        if self.state.read("config", true)?.is_null() {
            let creds = self.state.read("credentials", true)?;
            return Ok(
                json!({"providers":invitation::provider_summary(&json!({}), &creds),"phalaConfigured":configured(&creds["PHALA_CLOUD_API_KEY"]),"note":"Local credential presence only. Initialize a signed deployment to determine release permissions."}),
            );
        }
        let c = self.load(false)?;
        Ok(
            json!({"providers":invitation::provider_summary(&c.invitation,&c.credentials),"phalaConfigured":configured(&c.credentials["PHALA_CLOUD_API_KEY"]),"note":"Local credential presence only; saved changes are not automatically applied to a running worker. Admission and pricing remain required."}),
        )
    }
    pub fn balances(&self, publish: bool) -> Result<Value> {
        let c = self.load(false)?;
        let local = crate::balances::collect(self.http, &c.credentials);
        let remote_result = if publish {
            need(!local.is_empty(), "no_local_api_keys_to_check")?;
            self.api(
                &c,
                "balances",
                json!({"balances":crate::balances::reports(&local)}),
            )
        } else {
            self.api(&c, "status", json!({}))
        };
        let remote_error = remote_result
            .as_ref()
            .err()
            .map(|e| crate::diagnostics::value(e.0));
        let remote = remote_result.ok();
        let rows = crate::balances::merge(
            &remote
                .as_ref()
                .map(|v| v["balances"].clone())
                .unwrap_or(Value::Null),
            local,
            now(),
        );
        Ok(
            json!({"minerId":c.invitation["minerId"],"balances":rows,"published":publish && remote.is_some(),"publishRequested":publish,"remoteUnavailable":remote.is_none(),"remoteError":remote_error,
            "note":"Account-level USD credit snapshots; not miner profit or spend allowance. Shared account balances must not be added across miners."}),
        )
    }
    pub fn offers(&self) -> Result<Value> {
        if self.state.read("config", true)?.is_null() {
            return self.catalog();
        }
        let c = self.load(false)?;
        let v = self.api(&c, "status", json!({}))?;
        need(v["offers"].is_array(), "offers_not_supported")?;
        Ok(crate::offer_catalog::offers(
            &v,
            &c.invitation,
            &c.credentials,
        ))
    }
    pub fn catalog(&self) -> Result<Value> {
        let nonce = id();
        let mut req = Request::get(
            format!(
                "{}/v1/catalog?nonce={nonce}",
                s(&self.trust["coordinatorUrl"])?.trim_end_matches('/')
            ),
            &["subnet.everyframe.studio"],
        );
        req.limit = 1000000;
        let response = self.http.json(req).map_err(|e| {
            if e.0 == "http_404" {
                Error("public_catalog_unavailable")
            } else {
                e
            }
        })?;
        let out = verified(&response, s(&self.trust["publicKey"])?)?;
        need(
            out["protocol"] == 1
                && out["action"] == "catalog"
                && out["nonce"] == nonce
                && (n(&out["at"])? - now()).abs() < 30000
                && out["chain"] == crate::hotkey::chain(&self.trust)?,
            "invalid_coordinator_response",
        )?;
        let rows = out["models"]
            .as_array()
            .ok_or(Error("invalid_coordinator_response"))?;
        need(rows.len() <= 512, "invalid_coordinator_response")?;
        Ok(
            json!({"discovery":true,"offers":rows,"note":out["note"],"next":"No wallet, provider key or VM needed to view this catalog. Run everycli setup when ready; pricing does not guarantee earnings."}),
        )
    }
    pub fn earnings(&self) -> Result<Value> {
        let c = self.load(false)?;
        let v = self.api(&c, "status", json!({}))?;
        let mut out = v["earnings"].clone();
        need(out.is_object(), "invalid_coordinator_response")?;
        out["minerId"] = v["minerId"].clone();
        out["asOf"] = v["at"].clone();
        Ok(out)
    }
    pub fn offer(
        &self,
        model: &str,
        discount: Option<&str>,
        withdraw: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        if !withdraw {
            invitation::discount(
                discount.ok_or(Error("missing_option").option("--discount-pct"))?,
            )?;
        }
        need(!model.is_empty() && model.len() <= 150, "model_required")?;
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let status = self.api(&c, "status", json!({}))?;
        let entry = status["offers"]
            .as_array()
            .and_then(|a| a.iter().find(|o| o["model"] == model))
            .ok_or(Error("model_not_allowed"))?;
        let bp = if withdraw {
            entry["discountBp"].as_i64().unwrap_or(0)
        } else {
            invitation::discount(discount.ok_or(Error("invalid_discount"))?)?
        };
        if !withdraw {
            let base = n(&entry["baseMinerRewardMicrousd"])?;
            need(
                base > 0 && entry["pricingHash"].is_string(),
                "offer_pricing_unavailable",
            )?;
            let reward =
                (base.checked_mul(10000 - bp).ok_or(Error("invalid_price"))? + 9999) / 10000;
            let scope = if model.starts_with("studio/media/") {
                "example media work-value target; each job is repriced for its settings and references. This is not an actual provider invoice. Review everycli offers --json for the rate schedule"
            } else if model.starts_with("studio/") {
                "five-second reference target; actual job value varies with duration, resolution, aspect and reference media. Review everycli offers --json for the rate schedule"
            } else {
                "job target"
            };
            confirm(&format!(
                "Offer {model} at {}% discount, ${:.6}/{scope}. Provider bills do not decrease; no guaranteed traffic or profit.",
                bp as f64 / 100.,
                reward as f64 / 1e6
            ))?
        } else {
            confirm("Withdraw this offer? Existing assignments retain their locked price.")?
        };
        let out=self.api(&c,"offer",json!({"model":model,"discountBp":bp,"active":!withdraw,"expectedRevision":entry["revision"],"pricingHash":entry["pricingHash"]}))?;
        let saved = out["offers"]
            .as_array()
            .and_then(|a| a.iter().find(|o| o["model"] == model))
            .ok_or(Error("invalid_coordinator_response"))?;
        need(
            n(&saved["revision"])? == n(&entry["revision"])? + 1
                && saved["offered"] == !withdraw
                && saved["discountBp"] == bp,
            "invalid_coordinator_response",
        )?;
        Ok(json!({"minerId":out["minerId"],"routingPolicy":out["routingPolicy"],"offers":[saved]}))
    }
    pub fn hosting(&self, c: &Context, limit: f64) -> Result<Value> {
        need(
            limit.is_finite() && limit > 0. && limit <= 100.,
            "compute_limit_required",
        )?;
        let families = self.cloud(c)?.get("/instance-types")?;
        let offer = families["result"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|f| f["items"].as_array().into_iter().flatten())
            .find(|v| v["id"] == c.invitation["request"]["instance_type"])
            .ok_or(Error("hosting_limit_exceeded"))?;
        let rate = offer["hourly_rate"]
            .as_f64()
            .or_else(|| offer["hourly_rate"].as_str()?.parse().ok())
            .unwrap_or(f64::INFINITY);
        need(
            offer["requires_gpu"] == false && rate.is_finite() && rate >= 0. && rate <= limit,
            "hosting_limit_exceeded",
        )?;
        Ok(offer.clone())
    }
    pub fn ensure_ready(&self, c: &Context, status: &Value) -> Result<()> {
        need(status["enabled"] == true, "miner_disabled")?;
        need(
            status["appId"] == c.deployment["appId"],
            "deployment_app_mismatch",
        )?;
        need(
            status["composeHash"] == c.invitation["composeHash"],
            "deployment_compose_mismatch",
        )?;
        attestation_ready(status, now())
    }
    pub fn drain(&self, c: &Context) -> Result<Value> {
        let status = self.api(c, "drain", json!({}))?;
        need(status["draining"] == true, "drain_not_confirmed")?;
        need(status["activeJobs"] == 0, "draining_in_progress")?;
        Ok(status)
    }
    pub fn deploy(&self, limit: f64, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        let mut c = self.load(true)?;
        let replacing = c.deployment["phase"] == "deleted";
        need(
            c.deployment.as_object().is_some_and(|o| o.is_empty())
                || c.deployment["phase"] == "deleted",
            "deployment_already_recorded",
        )?;
        // Refresh an attached same-hotkey profile once, before any paid action.
        // Never retry deploy itself: a later error may follow a successful POST.
        let status = if c.invitation["publicOnboarding"] == true {
            Some(match self.api(&c, "status", json!({})) {
                Err(e) if e.0 == "coordinator_auth_rejected" => {
                    self.reconnect_locked(None, &|_| Ok(()))?;
                    c = self.load(true)?;
                    self.api(&c, "status", json!({}))?
                }
                other => other?,
            })
        } else {
            None
        };
        let inv = &c.invitation;
        need(
            (inv["appId"].is_null() && inv["previousComposeHash"].is_null())
                || (replacing && inv["publicOnboarding"] == true),
            "initial_invitation_required",
        )?;
        if replacing && inv["publicOnboarding"] == true {
            let status = status
                .as_ref()
                .ok_or(Error("invalid_coordinator_response"))?;
            need(
                status["replacementSupported"] == true,
                "replacement_not_supported",
            )?;
            need(
                status["appId"] == c.deployment["appId"]
                    && status["composeHash"] == inv["composeHash"],
                "deployment_binding_mismatch",
            )?;
            need(status["activeJobs"] == 0, "active_jobs")?;
        }
        let cloud = self.cloud(&c)?;
        let offer = self.hosting(&c, limit)?;
        let images = cloud.catalog("/os-images")?;
        let current = cloud.catalog("/cvms/paginated")?;
        need(
            images["items"].as_array().unwrap().iter().any(|v| {
                v["name"] == inv["request"]["image"]
                    && v["is_dev"] == false
                    && v["requires_gpu"] == false
                    && v["os_image_hash"] == inv["osImageHash"]
            }),
            "os_catalog_changed",
        )?;
        need(
            !current["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["name"] == inv["request"]["name"] && v["status"] != "deleted"),
            "existing_cloud_workload",
        )?;
        need(
            compose_hash(&inv["request"]["compose_file"])? == inv["composeHash"],
            "compose_hash_mismatch",
        )?;
        encrypt(inv, &c.credentials, &"11".repeat(32), false)?;
        confirm(&format!(
            "Create {}: compute {}/hour plus 20GB storage. No automatic total spending cap or shutdown. No video generated by this command.",
            s(&inv["request"]["name"])?,
            offer["hourly_rate"]
        ))?;
        if replacing && inv["publicOnboarding"] == true {
            let status = self.api(&c, "drain", json!({}))?;
            need(
                status["draining"] == true && status["activeJobs"] == 0,
                "active_jobs",
            )?;
        }
        let mut state = json!({"phase":"provision_intent","startedAt":now(),"computeHourlyUsd":offer["hourly_rate"].as_str().map(str::to_string).unwrap_or(offer["hourly_rate"].to_string()),"appId":null});
        if replacing && inv["publicOnboarding"] == true {
            state["replacesAppId"] = c.deployment["appId"].clone();
        }
        self.state.write("deployment", &state)?;
        let p = cloud.mutate("/cvms/provision", "POST", &inv["request"], false)?;
        let mut diagnostic = json!({});
        for k in ["app_id", "compose_hash", "os_image_hash", "instance_type"] {
            if p[k].as_str().is_some_and(|s| s.len() <= 200) {
                diagnostic[k] = p[k].clone()
            }
        }
        state["provisionResponse"] = diagnostic;
        self.state.write("deployment", &state)?;
        need(
            matches("[0-9a-f]{40}", &p["app_id"])
                && p["compose_hash"] == inv["composeHash"]
                && p["os_image_hash"] == inv["osImageHash"]
                && matches("[0-9a-fA-F]{64}", &p["app_env_encrypt_pubkey"])
                && p["kms_info"]["slug"].is_string(),
            "unexpected_provision",
        )?;
        state["phase"] = json!("provisioned");
        state["appId"] = p["app_id"].clone();
        state["encryptionKey"] = p["app_env_encrypt_pubkey"].clone();
        state["kms"] = p["kms_info"]["slug"].clone();
        self.state.write("deployment", &state)?;
        let mut body = encrypt(inv, &c.credentials, s(&state["encryptionKey"])?, false)?;
        body["app_id"] = state["appId"].clone();
        body["compose_hash"] = inv["composeHash"].clone();
        state["phase"] = json!("commit_intent");
        self.state.write("deployment", &state)?;
        let out = cloud.mutate("/cvms", "POST", &body, false)?;
        need(
            out["app_id"] == state["appId"] && out["name"] == inv["request"]["name"],
            "unexpected_commit",
        )?;
        state["phase"] = json!("awaiting_attestation");
        self.state.write("deployment", &state)?;
        self.bind_public_app(&c, &state)?;
        Ok(
            json!({"phase":state["phase"],"appId":state["appId"],"composeHash":inv["composeHash"],"next":PENDING}),
        )
    }
    pub fn activate(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        self.activate_keys(confirm, false)
    }
    fn bind_public_app(&self, c: &Context, deployment: &Value) -> Result<()> {
        if c.invitation["publicOnboarding"] != true {
            return Ok(());
        }
        let mut body =
            json!({"appId":deployment["appId"],"composeHash":c.invitation["composeHash"]});
        if deployment["replacesAppId"].is_string() {
            body["previousAppId"] = deployment["replacesAppId"].clone();
        }
        let out = self.api(c, "bind-app", body)?;
        need(
            out["appId"] == deployment["appId"]
                && out["composeHash"] == c.invitation["composeHash"],
            "deployment_binding_mismatch",
        )?;
        if deployment["replacesAppId"].is_string() {
            let mut updated = self.validate(&out["deployment"], false)?;
            need(
                updated["appId"] == deployment["appId"],
                "deployment_binding_mismatch",
            )?;
            let mut expected = c.invitation.clone();
            for k in ["appId", "issuedAt", "expiresAt"] {
                updated.as_object_mut().unwrap().remove(k);
                expected.as_object_mut().unwrap().remove(k);
            }
            need(updated == expected, "deployment_binding_mismatch")?;
            let mut config = c.config.clone();
            config["invitation"] = out["deployment"].clone();
            self.state.write("config", &config)?;
        }
        Ok(())
    }
    pub fn apply_api_keys(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        self.activate_keys(confirm, true)
    }
    fn activate_keys(&self, confirm: &dyn Fn(&str) -> Result<()>, refresh: bool) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.activate_keys_locked(confirm, refresh)
    }
    pub(crate) fn activate_keys_locked(
        &self,
        confirm: &dyn Fn(&str) -> Result<()>,
        refresh: bool,
    ) -> Result<Value> {
        let c = self.load(true)?;
        need(
            if refresh {
                ["running", "drained", "awaiting_readmission"]
                    .contains(&c.deployment["phase"].as_str().unwrap_or(""))
            } else {
                c.deployment["phase"] == "awaiting_attestation"
            },
            "activation_not_ready",
        )?;
        self.ensure_ready(&c, &self.api(&c, "status", json!({}))?)?;
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        let cloud = self.cloud(&c)?;
        let key = cloud.encryption_key(s(&c.deployment["appId"])?, s(&c.deployment["kms"])?)?;
        need(
            key["public_key"] == c.deployment["encryptionKey"],
            "encryption_key_changed",
        )?;
        let body = encrypt(&c.invitation, &c.credentials, s(&key["public_key"])?, true)?;
        confirm(
            "Release provider keys into this verified workload and restart it? Work remains paused until fresh attestation and everycli resume.",
        )?;
        self.ensure_ready(&c, &self.drain(&c)?)?;
        let mut state = c.deployment.clone();
        state["phase"] = json!("activate_intent");
        state["operationAt"] = json!(now());
        self.state.write("deployment", &state)?;
        let out = cloud.mutate(
            &format!("{}/envs", Cloud::app_path(s(&state["appId"])?)?),
            "PATCH",
            &body,
            false,
        )?;
        need(
            out["status"] == "in_progress" && out["allowed_envs_changed"] == false,
            "unexpected_environment_update",
        )?;
        state["phase"] = json!("awaiting_readmission");
        state["providerConfigured"] = json!(true);
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":state["phase"],"next":"Wait for fresh post-restart TEE admission, then everycli resume. No generation submitted."}),
        )
    }
    pub fn resume(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.resume_locked(confirm)
    }
    pub(crate) fn resume_locked(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let c = self.load(true)?;
        let phase = s(&c.deployment["phase"])?;
        need(
            ["awaiting_readmission", "running", "drained"].contains(&phase),
            "resume_not_ready",
        )?;
        let status = self.api(&c, "status", json!({}))?;
        self.ensure_ready(&c, &status)?;
        if phase == "awaiting_readmission" {
            need(
                n(&status["attestation"]["at"])? > n(&c.deployment["operationAt"])?
                    && n(&status["attestation"]["at"])?
                        > c.deployment["previousAttestationAt"].as_i64().unwrap_or(0),
                "post_restart_attestation_required",
            )?
        };
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        confirm(
            "Accept paid provider work when routing is enabled? Your provider account pays for assignments.",
        )?;
        self.ensure_ready(&c, &self.api(&c, "status", json!({}))?)?;
        let out = self.api(&c, "resume", json!({}))?;
        need(out["draining"] == false, "resume_not_confirmed")?;
        let mut d = c.deployment;
        d["phase"] = json!("running");
        self.state.write("deployment", &d)?;
        Ok(json!({"phase":"running","draining":false,"routingEnabled":out["routingEnabled"]}))
    }
    pub fn stop(&self, drain_only: bool, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        self.stop_mode(drain_only, false, confirm)
    }
    fn cloud_context(&self) -> Result<Context> {
        // Cloud shutdown must survive expired delegates and coordinator outages.
        // Still verify the signed target, local permissions and cloud identity.
        self.state.prepare()?;
        let config = self.state.read("config", false)?;
        let invitation = self.validate(&config["invitation"], true)?;
        Ok(Context {
            config,
            invitation,
            credentials: self.state.read("credentials", false)?,
            // A profile that never deployed has no record; commands that need
            // one fail with deployment_not_recorded below.
            deployment: match self.state.read("deployment", true)? {
                Value::Null => json!({}),
                d => d,
            },
        })
    }
    fn recover_cloud_target(&self, c: &mut Context) -> Result<()> {
        if c.deployment["appId"].is_string() {
            return Ok(());
        }
        need(
            c.deployment["phase"] == "provision_intent",
            "deployment_not_recorded",
        )?;
        let catalog = self.cloud(c)?.catalog("/cvms/paginated")?;
        let matches = catalog["items"]
            .as_array()
            .ok_or(Error("invalid_cloud_response"))?
            .iter()
            .filter(|v| v["name"] == c.invitation["request"]["name"])
            .collect::<Vec<_>>();
        need(
            matches.len() == 1,
            "cloud_target_ambiguous_use_phala_console",
        )?;
        c.deployment["appId"] = matches[0]["app_id"].clone();
        self.target(c, true)?;
        self.state.write("deployment", &c.deployment)
    }
    pub fn stop_mode(
        &self,
        drain_only: bool,
        force: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        let _lock = self.state.lock()?;
        need(!(drain_only && force), "conflicting_stop_options")?;
        let mut c = self.cloud_context()?;
        if !drain_only {
            self.recover_cloud_target(&mut c)?;
        }
        let phase = c.deployment["phase"].as_str().unwrap_or("");
        confirm(if drain_only {
            "Block new assignments while current work finishes?"
        } else if force {
            "Emergency shutdown of the verified VM, even if the coordinator is unreachable or a job is active? Paid in-flight work may be lost. Storage remains billable."
        } else {
            "Drain then gracefully shut down this exact VM? Storage may still cost money. No VM or data deleted."
        })?;
        let status = self.api(&c, "drain", json!({}));
        if !force && status.is_err() {
            return Err(Error("coordinator_unavailable_use_stop_force"));
        }
        let status = status.unwrap_or(Value::Null);
        if !force {
            need(status["draining"] == true, "drain_not_confirmed")?;
        }
        if drain_only || (!force && n(&status["activeJobs"])? > 0) {
            return Ok(
                json!({"phase":"draining","activeJobs":status["activeJobs"],"next":"VM still incurs charges; run stop after active work finishes."}),
            );
        };
        let info = self.target(&c, true)?;
        if ["stopped", "exited"].contains(&s(&info["status"])?) {
            let mut state = c.deployment.clone();
            state["phase"] = json!("stopped");
            self.state.write("deployment", &state)?;
            return Ok(
                json!({"phase":"stopped","next":"Already stopped. Reconcile local state; storage may still be billed."}),
            );
        };
        if ["stopping", "stop_intent"].contains(&phase) {
            return Ok(
                json!({"phase":phase,"cloudStatus":info["status"],"next":"Shutdown already requested; no duplicate request sent. If shutdown is stuck, use the Phala console or everycli delete with the exact app ID."}),
            );
        }
        let cloud = self.cloud(&c)?;
        let mut state = c.deployment.clone();
        state["phase"] = json!("stop_intent");
        state["operationAt"] = json!(now());
        self.state.write("deployment", &state)?;
        cloud.mutate(
            &format!("{}/shutdown", Cloud::app_path(s(&state["appId"])?)?),
            "POST",
            &json!({}),
            true,
        )?;
        state["phase"] = json!("stopping");
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":"stopping","next":"Reconcile shutdown completion; storage may still be billed."}),
        )
    }
    pub fn delete(&self, app_id: &str, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        let mut c = self.cloud_context()?;
        let recorded = c.deployment["appId"].is_string();
        if !recorded && c.deployment["phase"] != "provision_intent" {
            return self.delete_unrecorded(&c, app_id, confirm);
        }
        self.recover_cloud_target(&mut c)?;
        need(c.deployment["appId"] == app_id, "deployment_app_mismatch")?;
        if c.deployment["phase"] == "deleted" {
            return Ok(json!({"phase":"deleted","appId":app_id}));
        }
        let info = match self.deletion_target(&c) {
            Err(e)
                if e.0 == "phala_http_404"
                    && ["delete_intent", "deleting"]
                        .contains(&c.deployment["phase"].as_str().unwrap_or("")) =>
            {
                let mut state = c.deployment.clone();
                state["phase"] = json!("deleted");
                self.state.write("deployment", &state)?;
                return Ok(json!({"phase":"deleted","appId":app_id}));
            }
            other => other?,
        };
        if ["delete_intent", "deleting"].contains(&c.deployment["phase"].as_str().unwrap_or("")) {
            return Ok(
                json!({"phase":c.deployment["phase"],"appId":app_id,"next":"Deletion not yet confirmed; no duplicate request sent. Check the Phala console, or rerun this command to check removal."}),
            );
        }
        confirm(&format!(
            "Permanently DELETE VM {app_id} ({}) and its storage? This is irreversible; active jobs and local VM data will be lost. Other cloud resources and provider subscriptions are not deleted.",
            s(&info["name"])?
        ))?;
        let drained = self.api(&c, "drain", json!({})).is_ok();
        let mut state = c.deployment.clone();
        state["phase"] = json!("delete_intent");
        state["operationAt"] = json!(now());
        self.state.write("deployment", &state)?;
        self.cloud(&c)?
            .request(&Cloud::app_path(app_id)?, "DELETE", None, false, true)?;
        state["phase"] = json!("deleting");
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":"deleting","appId":app_id,"coordinatorDrained":drained,"next":"VM deletion accepted, not yet confirmed. Rerun this command to verify removal and check final storage charges in Phala Billing. Other resources/subscriptions are unchanged; preserve this local management record."}),
        )
    }
    /// Deletes a workload with this miner's signed name that this profile did not
    /// deploy (another computer, or an earlier setup). Journal the exact target
    /// before deletion so a lost response cannot cause another destructive call.
    fn delete_unrecorded(
        &self,
        c: &Context,
        app_id: &str,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        Cloud::app_path(app_id)?;
        let found = self
            .cloud_workloads(c)?
            .into_iter()
            .find(|w| w["appId"] == app_id);
        let Some(workload) = found else {
            // Already gone, or not one of this miner's workloads.
            return Err(Error("cloud_workload_not_found"));
        };
        let info = self.cloud(c)?.info(app_id)?;
        need(
            info["app_id"] == app_id
                && info["name"] == c.invitation["request"]["name"]
                && matches("[0-9a-f]{64}", &info["compose_hash"]),
            "target_mismatch",
        )?;
        if ["deleting", "deleted"].contains(&workload["status"].as_str().unwrap_or("")) {
            self.state.write("deployment", &json!({"phase":"deleting","appId":app_id,"deletionComposeHash":info["compose_hash"],"operationAt":now()}))?;
            return Ok(
                json!({"phase":"deleting","appId":app_id,"recordedHere":false,"next":"Deletion already requested; no duplicate request sent. Check again with everycli status."}),
            );
        }
        confirm(&format!(
            "Permanently DELETE VM {app_id} ({}), which this computer did not deploy? This is irreversible; active jobs and its VM data will be lost. Other cloud resources and provider subscriptions are not deleted.",
            s(&info["name"])?
        ))?;
        let drained = self.api(c, "drain", json!({})).is_ok();
        let mut state = json!({"phase":"delete_intent","appId":app_id,"deletionComposeHash":info["compose_hash"],"operationAt":now()});
        self.state.write("deployment", &state)?;
        self.cloud(c)?
            .request(&Cloud::app_path(app_id)?, "DELETE", None, false, true)?;
        state["phase"] = json!("deleting");
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":"deleting","appId":app_id,"recordedHere":false,"coordinatorDrained":drained,"next":"Deletion journal saved. Rerun everycli delete with this app ID to confirm removal before deploying again."}),
        )
    }
    fn deletion_target(&self, c: &Context) -> Result<Value> {
        if c.deployment["deletionComposeHash"].is_null() {
            return self.target(c, true);
        }
        let info = self.cloud(c)?.info(s(&c.deployment["appId"])?)?;
        need(
            info["app_id"] == c.deployment["appId"]
                && info["name"] == c.invitation["request"]["name"]
                && info["compose_hash"] == c.deployment["deletionComposeHash"],
            "target_mismatch",
        )?;
        Ok(info)
    }
    pub fn label(&self, name: Option<&str>) -> Result<Value> {
        if let Some(name) = name {
            need(
                !name.trim().is_empty()
                    && name.chars().count() <= 64
                    && !name.chars().any(char::is_control),
                "invalid_worker_label",
            )?;
            let _lock = self.state.lock()?;
            self.state.write("label", &json!({"name":name.trim()}))?;
        }
        Ok(
            json!({"label":self.state.read("label",true)?["name"],"scope":"local-profile","signedVmNameChanged":false}),
        )
    }
    pub fn logs(&self) -> Result<Value> {
        let c = self.cloud_context()?;
        self.target(&c, true)?;
        let composition = self.cloud(&c)?.get(&format!(
            "{}/composition",
            Cloud::app_path(s(&c.deployment["appId"])?)?
        ))?;
        let containers = composition["containers"]
            .as_array()
            .ok_or(Error("worker_logs_unavailable"))?;
        let mut events = vec![];
        for container in containers.iter().take(8) {
            let Some(endpoint) = container["log_endpoint"].as_str() else {
                continue;
            };
            let mut url = crate::network::safe_url(
                endpoint,
                &["*.phala.network".into(), "*.phala.com".into()],
            )?;
            url.query_pairs_mut()
                .append_pair("tail", "100")
                .append_pair("text", "true")
                .append_pair("bare", "true");
            let mut request = Request::get(url.to_string(), &["*.phala.network", "*.phala.com"]);
            request.limit = 512000;
            // Never forward the Phala API token to a log host, or expose raw
            // container lines: publish only structured worker diagnostic fields.
            let bytes = self.http.bytes(request)?;
            for line in String::from_utf8_lossy(&bytes).lines().take(200) {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if ![
                    "worker_cycle",
                    "worker_cycle_failed",
                    "generation_failed",
                    "submission_rejected",
                    "submission_uncertain",
                ]
                .iter()
                .any(|e| value["event"] == *e)
                {
                    continue;
                }
                let mut event = json!({"event":value["event"]});
                for key in ["reason", "state", "jobId", "attemptId"] {
                    let field = if value[key].is_null() {
                        &value["result"][key]
                    } else {
                        &value[key]
                    };
                    if crate::matches("[a-zA-Z0-9_-]{1,160}", field) {
                        event[key] = field.clone();
                    }
                }
                events.push(event);
            }
        }
        Ok(
            json!({"appId":c.deployment["appId"],"label":self.state.read("label",true)?["name"],"events":events,"note":"Last 100 lines per container; only structured worker events are shown. No public-log setting was changed. For boot or private logs, use the authenticated Phala console."}),
        )
    }
    pub fn start(&self, limit: f64, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        let c = self.load(true)?;
        need(c.deployment["phase"] == "stopped", "start_not_ready")?;
        let info = self.target(&c, false)?;
        need(
            ["stopped", "exited"].contains(&s(&info["status"])?),
            "cloud_not_stopped",
        )?;
        let rate = self.hosting(&c, limit)?;
        confirm(&format!(
            "Start the existing VM: compute {}/hour plus storage? No new VM; fresh admission required.",
            rate["hourly_rate"]
        ))?;
        self.drain(&c)?;
        let cloud = self.cloud(&c)?;
        let mut state = c.deployment.clone();
        state["phase"] = json!("start_intent");
        state["operationAt"] = json!(now());
        self.state.write("deployment", &state)?;
        cloud.mutate(
            &format!("{}/start", Cloud::app_path(s(&state["appId"])?)?),
            "POST",
            &json!({}),
            true,
        )?;
        state["phase"] = json!("starting");
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":"starting","next":"Reconcile after cloud reports running; obtain fresh admission before resume."}),
        )
    }
    pub fn update(&self, envelope: &Value, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.update_locked(envelope, confirm)
    }
    pub(crate) fn update_locked(
        &self,
        envelope: &Value,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        let c = self.load(false)?;
        let inv = self.validate(envelope, false)?;
        let old = &c.invitation;
        let d = &c.deployment;
        need(
            [
                "running",
                "drained",
                "awaiting_attestation",
                "awaiting_readmission",
            ]
            .contains(&s(&d["phase"])?),
            "update_not_ready",
        )?;
        need(
            [
                "minerId",
                "hotkey",
                "tokenHash",
                "authMode",
                "keyVersion",
                "osImageHash",
            ]
            .iter()
            .all(|k| inv[k] == old[k])
                && inv["appId"] == d["appId"]
                && inv["previousComposeHash"] == old["composeHash"]
                && inv["composeHash"] != inv["previousComposeHash"]
                && inv["request"]["name"] == old["request"]["name"],
            "update_target_mismatch",
        )?;
        need(
            compose_hash(&inv["request"]["compose_file"])? == inv["composeHash"],
            "compose_hash_mismatch",
        )?;
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        let cloud = self.cloud(&c)?;
        let key = cloud.encryption_key(s(&d["appId"])?, s(&d["kms"])?)?;
        need(
            key["public_key"] == d["encryptionKey"],
            "encryption_key_changed",
        )?;
        let mut body = encrypt(&inv, &c.credentials, s(&key["public_key"])?, false)?;
        confirm(
            "Install this signed app-specific release? Drains new work; provider keys disabled until fresh admission/activation.",
        )?;
        self.drain(&c)?;
        let mut state = d.clone();
        state["phase"] = json!("update_provision_intent");
        state["pendingInvitation"] = envelope.clone();
        state["operationAt"] = json!(now());
        self.state.write("deployment", &state)?;
        let path = Cloud::app_path(s(&d["appId"])?)?;
        // Phala otherwise retains the old allowed_envs even when the signed
        // compose changes it, yielding a different hash (e.g. adding Vivix).
        // This is a request option, NOT part of the signed compose/hash.
        let mut provision = inv["request"]["compose_file"].clone();
        provision["update_env_vars"] = json!(true);
        let result = cloud.mutate(
            &format!("{path}/compose_file/provision"),
            "POST",
            &provision,
            false,
        )?;
        // Store only bounded public diagnostics, never raw provider responses.
        state["updateProvisionResponse"] = json!({
            "expectedComposeHash":inv["composeHash"],
            "returnedComposeHash":result["compose_hash"].as_str()
                .filter(|v| v.len()==64 && v.bytes().all(|b| b.is_ascii_hexdigit())),
            "updateEnvVars":true
        });
        self.state.write("deployment", &state)?;
        need(
            result["compose_hash"] == inv["composeHash"],
            "update_compose_mismatch",
        )?;
        state["phase"] = json!("update_commit_intent");
        self.state.write("deployment", &state)?;
        body["compose_hash"] = inv["composeHash"].clone();
        body["update_env_vars"] = json!(true);
        cloud.mutate(&format!("{path}/compose_file"), "PATCH", &body, true)?;
        state["phase"] = json!("updating");
        self.state.write("deployment", &state)?;
        Ok(
            json!({"phase":"updating","next":"Reconcile completed update, request new exact-policy admission, then activate."}),
        )
    }
    pub fn reconcile(&self) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.reconcile_locked()
    }
    pub(crate) fn reconcile_locked(&self) -> Result<Value> {
        let c = self.load(false)?;
        need(
            c.deployment["appId"].is_string(),
            "manual_reconciliation_required",
        )?;
        let info = self.target(&c, true)?;
        let phase = s(&c.deployment["phase"])?;
        let status = s(&info["status"])?;
        if phase == "update_provision_intent" {
            // The provision endpoint validates/caches a candidate only. The
            // separate PATCH that restarts the VM is preceded by a durable
            // update_commit_intent write. Thus this phase cannot contain a
            // CLI-issued commit, even after a lost provision response in 0.3.7.
            // Never infer that a commit is safe from elapsed time or retry it.
            let inv = self.validate(&c.deployment["pendingInvitation"], true)?;
            need(
                inv["appId"] == c.deployment["appId"]
                    && inv["previousComposeHash"] == c.invitation["composeHash"]
                    && inv["request"]["name"] == c.invitation["request"]["name"]
                    && [
                        "minerId",
                        "hotkey",
                        "tokenHash",
                        "authMode",
                        "keyVersion",
                        "osImageHash",
                    ]
                    .iter()
                    .all(|k| inv[k] == c.invitation[k])
                    && status == "running"
                    && info["compose_hash"] == c.invitation["composeHash"],
                "update_provision_state_changed",
            )?;
            let mut d = c.deployment.clone();
            d["phase"] = json!(if d["providerConfigured"] == true {
                "drained"
            } else {
                "awaiting_attestation"
            });
            d["provisionReconciledAt"] = json!(now());
            d.as_object_mut().unwrap().remove("pendingInvitation");
            self.state.write("deployment", &d)?;
            return Ok(
                json!({"phase":d["phase"],"resolved":true,"cloudStatus":status,
                "next":"The existing worker is unchanged; preparation was not committed. Run everycli worker-update to continue, or everycli stop to shut down. No cloud mutation or resume was performed."}),
            );
        }
        if status == "running"
            && ["commit_intent", "provisioned", "awaiting_attestation"].contains(&phase)
        {
            self.bind_public_app(&c, &c.deployment)?;
        }
        let mut d = c.deployment.clone();
        let next = if ["stop_intent", "stopping", "stopped"].contains(&phase)
            && ["stopped", "exited"].contains(&status)
        {
            Some("stopped")
        } else if ["commit_intent", "provisioned"].contains(&phase) && status == "running" {
            Some("awaiting_attestation")
        } else if ["start_intent", "starting"].contains(&phase) && status == "running" {
            Some(if d["providerConfigured"] == true {
                "awaiting_readmission"
            } else {
                "awaiting_attestation"
            })
        } else if ["update_commit_intent", "updating"].contains(&phase)
            && !d["pendingInvitation"].is_null()
            && status == "running"
        {
            let inv = self.validate(&d["pendingInvitation"], true)?;
            need(
                info["compose_hash"] == inv["composeHash"],
                "update_not_observed",
            )?;
            let mut config = c.config;
            config["invitation"] = d["pendingInvitation"].clone();
            self.state.write("config", &config)?;
            d.as_object_mut().unwrap().remove("pendingInvitation");
            d["providerConfigured"] = json!(false);
            Some("awaiting_attestation")
        } else {
            None
        };
        if let Some(phase) = next {
            d["phase"] = json!(phase);
            self.state.write("deployment", &d)?;
            return Ok(
                json!({"phase":phase,"next":"Fresh TEE admission required before resume; stopped storage may still be billed."}),
            );
        };
        Ok(
            json!({"phase":phase,"cloudStatus":status,"resolved":false,"next":"No conclusive state change. Do not retry uncertain cloud mutations; ask the operator to reconcile."}),
        )
    }
}
