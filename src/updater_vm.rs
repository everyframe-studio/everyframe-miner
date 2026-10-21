//! Isolated in-VM supervisor. No Docker socket, inbound server or wallet secret.
//! Its image remains pinned while the generation image changes.
use crate::{
    Error, Result, invitation, miner::Miner, need, network::PublicHttp, now, s, state::State,
};
use serde_json::{Value, json};

// Below Linux's per-argument/environment-string ceiling, including its name.
pub const MAX_SEED_BYTES: usize = 100_000;

pub fn suffix(image: &str) -> String {
    format!(
        "  updater:\n    image: {image}\n    restart: unless-stopped\n    read_only: true\n    user: \"0:0\"\n    cap_drop: [ALL]\n    security_opt: [no-new-privileges:true]\n    pids_limit: 32\n    mem_limit: 192m\n    cpus: 0.25\n    environment:\n      HOME: /state\n      EVERYFRAME_UPDATER_SEED: ${{EVERYFRAME_UPDATER_SEED}}\n    volumes:\n      - updater-state:/state\n    tmpfs:\n      - /tmp:size=16m,mode=1777\nvolumes:\n  updater-state:\n"
    )
}
pub fn image(compose: &Value) -> Result<String> {
    let docker = s(&compose["docker_compose_file"])?;
    let re =
        regex::Regex::new(r"  updater:\n    image: (ghcr\.io/[a-z0-9/_-]+@sha256:[0-9a-f]{64})\n")
            .unwrap();
    let m = re
        .captures(docker)
        .ok_or(Error("worker_auto_update_bootstrap_required"))?;
    let image = m[1].to_string();
    let tail = suffix(&image);
    let prefix = docker
        .strip_suffix(&tail)
        .ok_or(Error("updater_not_isolated"))?;
    need(
        ![
            "EVERYFRAME_UPDATER",
            "updater-state",
            "docker.sock",
            "  updater:",
        ]
        .iter()
        .any(|s| prefix.contains(s)),
        "updater_not_isolated",
    )?;
    Ok(image)
}
pub fn permitted_credentials(c: &Value, inv: &Value) -> Value {
    let mut out = json!({});
    for k in ["PHALA_CLOUD_API_KEY", "CONSOLE_AUTH", "WORKER_AUTH"] {
        if !c[k].is_null() {
            out[k] = c[k].clone();
        }
    }
    for k in invitation::invited_keys(inv) {
        if !c[&k].is_null() {
            out[&k] = c[&k].clone();
        }
    }
    out
}

impl Miner<'_> {
    pub fn enroll_updater(&self, confirm: &dyn Fn(&str) -> Result<()>) -> Result<Value> {
        let _lock = self.state.lock()?;
        let current = self.load(false)?;
        let status = self.api(&current, "status", json!({}))?;
        let saved_seed: Value = serde_json::from_str(
            current.credentials["UPDATER_SEED"]
                .as_str()
                .unwrap_or("null"),
        )
        .map_err(|_| Error("invalid_updater_seed"))?;
        // Re-enabling the same private controller does not require the laptop
        // profile to have followed every automatic image update.
        if !saved_seed.is_null()
            && saved_seed["settings"]["capsuleId"] == status["automaticUpdates"]["capsuleId"]
            && saved_seed["settings"]["appId"] == status["appId"]
            && status["automaticUpdates"]["report"]["composeHash"] == status["composeHash"]
            && saved_seed["credentials"]
                == permitted_credentials(&current.credentials, &current.invitation)
        {
            confirm(crate::worker_auto_update::CONSENT)?;
            self.api(
                &current,
                "worker-auto-update-policy",
                json!({"appId":status["appId"],"enabled":true}),
            )?;
            return Ok(
                json!({"ok":true,"enabled":true,"state":"enabled","next":"Existing in-VM updater re-enabled. No credentials or journals were replaced."}),
            );
        }
        let c = self.load(true)?;
        let flow = self.state.read("worker-update", true)?;
        need(
            (flow.is_null() || flow["stage"] == "complete")
                && c.deployment["pendingInvitation"].is_null()
                && c.deployment["abandonedInvitation"].is_null(),
            "manual_worker_update_pending",
        )?;
        image(&c.invitation["request"]["compose_file"])?;
        for name in ["CONSOLE_AUTH", "WORKER_AUTH"] {
            need(
                c.credentials[name]["certificate"]["value"]["kind"]
                    == "everyframe-hotkey-delegation-v2"
                    && c.credentials[name]["certificate"]["value"]["expiresAt"].is_null(),
                "updater_nonexpiring_delegate_required",
            )?;
        }
        need(
            c.invitation["authMode"] == "hotkey-v1" && c.deployment["providerConfigured"] == true,
            "worker_update_original_profile_required",
        )?;
        need(
            ["running", "drained"].contains(&c.deployment["phase"].as_str().unwrap_or("")),
            "update_not_ready",
        )?;
        self.ensure_ready(&c, &status)?;
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        need(
            status["automaticUpdates"]["report"]["pending"] != true,
            "worker_auto_update_recovery_pending",
        )?;
        need(status["activeJobs"] == 0, "updater_enrollment_busy")?;
        // Validate and size the entire handoff before pausing work, changing the
        // remote capsule, or writing credentials. A rejected enrollment is read-only.
        let capsule_id = crate::protocol::id();
        let credentials = permitted_credentials(&c.credentials, &c.invitation);
        let deployment = json!({"phase":"awaiting_readmission","operationAt":now(),
            "appId":c.deployment["appId"],"composeHash":c.invitation["composeHash"],
            "encryptionKey":c.deployment["encryptionKey"],"kms":c.deployment["kms"],
            "providerConfigured":true});
        let settings = json!({"version":1,"enabled":true,"controller":true,"capsuleId":capsule_id,"minerId":c.invitation["minerId"],"appId":c.deployment["appId"],
   "credentialHash":crate::protocol::digest(&credentials)?,"bootstrap":true,"restoreAccepting":status["draining"]==false,
   "previousAttestationAt":status["attestation"]["at"],"enabledAt":now()});
        // Never copy arbitrary local metadata (wallet hints, backups or future
        // secret fields) into the VM. Only the signed profile is required.
        let config = json!({"version":1,"invitation":c.config["invitation"],"initializedAt":now()});
        let capsule = json!({"version":1,"network":c.invitation["network"],"config":config,"deployment":deployment,"credentials":credentials,"settings":settings});
        let seed = capsule.to_string();
        need(seed.len() <= MAX_SEED_BYTES, "updater_seed_too_large")?;
        let mut local = c.credentials.clone();
        local["UPDATER_SEED"] = json!(seed);
        need(local.to_string().len() < 190000, "updater_seed_too_large")?;
        confirm(crate::worker_auto_update::CONSENT)?;
        // Opt-in is scoped to this exact VM. The coordinator never receives
        // the management token or the credential capsule.
        self.drain(&c)?;
        self.api(
            &c,
            "worker-auto-update-policy",
            json!({"appId":c.deployment["appId"],"enabled":true,"capsuleId":capsule_id}),
        )?;
        // Persist before PATCH so an ambiguous response is reconciled, not repeated.
        self.state.write("credentials", &local)?;
        self.state.write("worker-auto-update",&json!({"version":1,"enabled":true,"hosted":true,"capsuleId":capsule_id,"appId":c.deployment["appId"],"minerId":c.invitation["minerId"]}))?;
        self.activate_keys_locked_mode(&|_| Ok(()), true, true)?;
        Ok(
            json!({"ok":true,"state":"enrolling","enabled":true,"hosted":true,
   "next":"The in-VM updater will wait for fresh post-restart attestation, restore the previous accepting/paused state, and then check for signed updates while your computer is offline. Check everycli worker-auto-update for remote status. Do not repeat enrollment after an uncertain cloud response; reconcile it."}),
        )
    }
}

fn validate_seed(seed: &Value, trust: &Value) -> Result<Value> {
    need(seed["version"] == 1, "invalid_updater_seed")?;
    let inv = invitation::validate(&seed["config"]["invitation"], trust, now(), false)?;
    image(&inv["request"]["compose_file"])?;
    need(
        inv["authMode"] == "hotkey-v1"
            && seed["credentials"] == permitted_credentials(&seed["credentials"], &inv)
            && seed["settings"]["enabled"] == true
            && seed["settings"]["controller"] == true
            && crate::matches("[0-9a-f]{48}", &seed["settings"]["capsuleId"])
            && seed["settings"]["bootstrap"] == true
            && seed["settings"]["minerId"] == inv["minerId"]
            && seed["settings"]["appId"] == seed["deployment"]["appId"]
            && seed["deployment"]["providerConfigured"] == true
            && inv["appId"] == seed["deployment"]["appId"],
        "invalid_updater_seed",
    )?;
    for name in ["CONSOLE_AUTH", "WORKER_AUTH"] {
        need(
            seed["credentials"][name]["certificate"]["value"]["kind"]
                == "everyframe-hotkey-delegation-v2"
                && seed["credentials"][name]["certificate"]["value"]["expiresAt"].is_null(),
            "updater_nonexpiring_delegate_required",
        )?;
    }
    crate::hotkey::validate(&seed["credentials"]["CONSOLE_AUTH"], &inv, "console")?;
    crate::hotkey::validate(&seed["credentials"]["WORKER_AUTH"], &inv, "worker")?;
    need(
        seed["settings"]["credentialHash"] == crate::protocol::digest(&seed["credentials"])?,
        "invalid_updater_seed",
    )?;
    Ok(inv)
}

/// A new capsule is accepted only when the coordinator explicitly selected its
/// ID. Replayed bootstrap environment cannot reset a persistent update journal.
pub fn adopt_seed(miner: &Miner<'_>, seed: &Value) -> Result<()> {
    let inv = validate_seed(seed, &miner.trust)?;
    let context = crate::miner::Context {
        invitation: inv,
        config: seed["config"].clone(),
        credentials: seed["credentials"].clone(),
        deployment: seed["deployment"].clone(),
    };
    let remote = miner.api(&context, "worker-auto-update", json!({}))?;
    need(
        remote["automaticUpdates"]["enabled"] == true
            && remote["automaticUpdates"]["capsuleId"] == seed["settings"]["capsuleId"]
            && remote["automaticUpdates"]["appId"] == seed["deployment"]["appId"]
            && remote["appId"] == seed["deployment"]["appId"]
            && remote["composeHash"] == context.invitation["composeHash"],
        "invalid_updater_seed",
    )?;
    let state = &miner.state;
    let _lock = state.lock()?;
    let old = state.read("worker-auto-update", true)?;
    need(
        old["capsuleId"] != seed["settings"]["capsuleId"],
        "updater_already_enrolled",
    )?;
    let existing = state.read("config", true)?;
    if !existing.is_null() {
        need(
            existing["invitation"]["value"]["minerId"] == seed["settings"]["minerId"]
                && existing["invitation"]["value"]["appId"] == seed["settings"]["appId"],
            "invalid_updater_seed",
        )?;
    }
    let flow = state.read("worker-update", true)?;
    need(
        flow.is_null() || flow["stage"] == "complete",
        "worker_auto_update_recovery_pending",
    )?;
    let mut settings = seed["settings"].clone();
    // Credential rotation is not consent to retry a release that failed admission.
    if !old["blockedComposeHash"].is_null() {
        settings["blockedComposeHash"] = old["blockedComposeHash"].clone();
    }
    state.write("worker-update", &Value::Null)?;
    state.write("credentials", &seed["credentials"])?;
    state.write("deployment", &seed["deployment"])?;
    state.write("config", &seed["config"])?;
    // Capsule marker last. An interrupted adoption can finish with the same
    // coordinator-selected capsule; an old environment cannot roll it back.
    state.write("worker-auto-update", &settings)
}

fn run_iteration(state: &State) -> Result<Value> {
    let config = state.read("config", true)?;
    let raw = std::env::var("EVERYFRAME_UPDATER_SEED").unwrap_or_default();
    let seed: Value = if raw.is_empty() {
        Value::Null
    } else {
        need(raw.len() <= MAX_SEED_BYTES, "updater_seed_too_large")?;
        serde_json::from_str(&raw).map_err(|_| Error("invalid_updater_seed"))?
    };
    if config.is_null() && seed.is_null() {
        return Ok(json!({"state":"not_enrolled"}));
    }
    let network = if config.is_null() {
        &seed["network"]
    } else {
        &config["invitation"]["value"]["network"]
    };
    let miner = Miner {
        state: State {
            directory: state.directory.clone(),
        },
        trust: invitation::trust(s(network)?)?,
        http: &PublicHttp,
    };
    if !seed.is_null()
        && seed["settings"]["capsuleId"] != state.read("worker-auto-update", true)?["capsuleId"]
    {
        adopt_seed(&miner, &seed)?;
    }
    miner.worker_auto_tick()
}

pub fn run() -> Result<()> {
    unsafe { libc::umask(0o077) };
    let directory = "/state/profile";
    let state = State {
        directory: directory.into(),
    };
    state.prepare()?;
    let mut previous_event = Value::Null;
    loop {
        let result = run_iteration(&state);
        // Bounded transition-only logs; never print envelopes, responses or keys.
        let event = match result {
            Err(e) => {
                json!({"event":"updater_error","code":crate::diagnostics::safe_code(e.0)})
            }
            Ok(out) => json!({"event":"updater_state","state":out["state"],"code":out["code"]}),
        };
        if event != previous_event {
            eprintln!("{event}");
            previous_event = event;
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
