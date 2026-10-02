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

fn profile_diagnostic(error: &Error) -> Value {
    let (state, message, next) = match error.0 {
        "profile_not_initialized" => (
            "not_initialized",
            "No local miner profile found for the selected network and OS user. Hotkey registration and linking have not been checked.",
            "Connect your hotkey: everycli init --wallet <wallet> --hotkey default. If you already initialized here, use the original OS user and --state-dir/--network options.",
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
    json!({"ok":false,"state":state,"error":error.0,"message":message,"registration":"not_checked","next":next})
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
        let config = self.state.read("config", false).map_err(|e| {
            if e.0 == "file_not_found" {
                Error("profile_not_initialized")
            } else {
                e
            }
        })?;
        let credentials = self.state.read("credentials", false).map_err(|e| {
            if e.0 == "file_not_found" {
                Error("profile_credentials_missing")
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
            ["status", "drain", "resume", "offer", "bind-app", "balances"].contains(&action),
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
        if action == "status" {
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
        let out = verified(&self.http.json(r)?, s(&self.trust["publicKey"])?)?;
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
        match self.api(&c, "status", json!({})) {
            Ok(v) => out["coordinator"] = v,
            Err(e) => out["coordinatorError"] = json!({"code":e.0,"message":e.0}),
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
        out["next"] = json!(
            "Use everycli doctor for readiness. Reconcile uncertain operations; never blindly repeat a cloud mutation."
        );
        Ok(out)
    }
    pub fn doctor(&self) -> Result<Value> {
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
                    "Only provider keys supported by the signed release are sent. Use everycli offers to see available models.",
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
                    "Routing can intentionally be disabled.",
                );
                if v["routingPolicy"] == "discount-lottery-v1" {
                    add(
                        "Current model bid",
                        v["offers"].as_array().is_some_and(|a| {
                            a.iter()
                                .any(|o| o["offered"] == true && o["pricingCurrent"] == true)
                        }),
                        "Declare a current model offer.",
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
        Ok(json!({"ok":checks.iter().all(|c|c["ok"]==true),"checks":checks}))
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
        let c = self.load(false)?;
        let v = self.api(&c, "status", json!({}))?;
        need(v["offers"].is_array(), "offers_not_supported")?;
        Ok(json!({"minerId":v["minerId"],"routingPolicy":v["routingPolicy"],"offers":v["offers"]}))
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
            confirm(&format!(
                "Offer {model} at {}% discount, ${:.6}/job target. Provider bills do not decrease; no guaranteed traffic or profit.",
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
        let c = self.load(true)?;
        let inv = &c.invitation;
        need(
            c.deployment.as_object().is_some_and(|o| o.is_empty()),
            "deployment_already_recorded",
        )?;
        need(
            inv["appId"].is_null() && inv["previousComposeHash"].is_null(),
            "initial_invitation_required",
        )?;
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
                .any(|v| v["name"] == inv["request"]["name"]),
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
        let mut state = json!({"phase":"provision_intent","startedAt":now(),"computeHourlyUsd":offer["hourly_rate"].as_str().map(str::to_string).unwrap_or(offer["hourly_rate"].to_string()),"appId":null});
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
        let out = self.api(
            c,
            "bind-app",
            json!({"appId":deployment["appId"],"composeHash":c.invitation["composeHash"]}),
        )?;
        need(
            out["appId"] == deployment["appId"]
                && out["composeHash"] == c.invitation["composeHash"],
            "deployment_binding_mismatch",
        )
    }
    pub fn apply_api_keys(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        self.activate_keys(confirm, true)
    }
    fn activate_keys(&self, confirm: &dyn Fn(&str) -> Result<()>, refresh: bool) -> Result<Value> {
        let _lock = self.state.lock()?;
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
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let phase = c.deployment["phase"].as_str().unwrap_or("");
        need(!phase.ends_with("_intent"), "reconciliation_required")?;
        if phase == "stopping" {
            return Ok(
                json!({"phase":"stopping","next":"Shutdown already requested. Reconcile; no second request sent."}),
            );
        };
        confirm(if drain_only {
            "Block new assignments while current work finishes?"
        } else {
            "Drain then gracefully shut down this exact VM? Storage may still cost money. No VM or data deleted."
        })?;
        let status = self.api(&c, "drain", json!({}))?;
        need(status["draining"] == true, "drain_not_confirmed")?;
        if drain_only || n(&status["activeJobs"])? > 0 {
            return Ok(
                json!({"phase":"draining","activeJobs":status["activeJobs"],"next":"VM still incurs charges; run stop after active work finishes."}),
            );
        };
        let info = self.target(&c, true)?;
        if ["stopped", "exited"].contains(&s(&info["status"])?) {
            return Ok(
                json!({"phase":"stopped","next":"Already stopped. Reconcile local state; storage may still be billed."}),
            );
        };
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
        let c = self.load(false)?;
        let inv = self.validate(envelope, false)?;
        let old = &c.invitation;
        let d = &c.deployment;
        need(
            ["running", "awaiting_attestation", "awaiting_readmission"].contains(&s(&d["phase"])?),
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
        let result = cloud.mutate(
            &format!("{path}/compose_file/provision"),
            "POST",
            &inv["request"]["compose_file"],
            false,
        )?;
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
        let c = self.load(false)?;
        need(
            c.deployment["appId"].is_string(),
            "manual_reconciliation_required",
        )?;
        let info = self.target(&c, true)?;
        let phase = s(&c.deployment["phase"])?;
        let status = s(&info["status"])?;
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
