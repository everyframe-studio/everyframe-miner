//! Resumable, same-VM upgrades. The journal precedes each mutation; uncertain
//! cloud requests are observed, never blindly replayed. No bids are changed.
use crate::{
    Error, Result, cloud,
    miner::{Context, Miner},
    need, now, s,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupt_handler(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}
// Let normal SIGINT/SIGTERM return through the journal and lock guards instead
// of leaving an orphaned lock. SIGKILL/power loss still needs lock inspection.
struct InterruptGuard(Vec<(libc::c_int, libc::sigaction)>);
impl InterruptGuard {
    fn install() -> Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        let mut guard = Self(Vec::new());
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // sigaction structures are initialized before passing them to libc;
            // the handler only writes a lock-free atomic and performs no I/O.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                let mut old: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = interrupt_handler as *const () as usize;
                libc::sigemptyset(&mut action.sa_mask);
                need(
                    libc::sigaction(signal, &action, &mut old) == 0,
                    "interrupt_handler_failed",
                )?;
                guard.0.push((signal, old));
            }
        }
        Ok(guard)
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        for (signal, old) in &self.0 {
            unsafe {
                libc::sigaction(*signal, old, std::ptr::null_mut());
            }
        }
    }
}

fn compatible(c: &Context, inv: &Value, from: &Value) -> Result<()> {
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
        .all(|k| inv[k] == c.invitation[k])
            && inv["appId"] == c.deployment["appId"]
            && inv["request"]["name"] == c.invitation["request"]["name"]
            && inv["previousComposeHash"] == *from
            && inv["composeHash"] != *from
            && (c.invitation["composeHash"] == *from
                || c.invitation["composeHash"] == inv["composeHash"]),
        "update_target_mismatch",
    )?;
    need(
        cloud::compose_hash(&inv["request"]["compose_file"])? == inv["composeHash"],
        "compose_hash_mismatch",
    )
}

fn waiting(flow: &Value, reason: &'static str) -> Value {
    json!({"ok":false,"state":"waiting","stage":flow["stage"],"appId":flow["appId"],
        "release":flow["release"]["value"]["release"],"reason":reason,
        "next":"Rerun everycli worker-update using this same profile to continue. Do not redeploy. No offers were changed."})
}

impl Miner<'_> {
    pub fn worker_update(
        &self,
        check: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
        progress: &dyn Fn(&str),
    ) -> Result<Value> {
        let started = Instant::now();
        let _interrupt = InterruptGuard::install()?;
        self.worker_update_controlled(
            check,
            confirm,
            progress,
            &|_| {
                if started.elapsed() >= Duration::from_secs(600) {
                    return false;
                }
                for _ in 0..50 {
                    if INTERRUPTED.load(Ordering::Relaxed) {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                true
            },
            &|| INTERRUPTED.load(Ordering::Relaxed),
        )
    }

    /// Injectable bounded wait for lifecycle tests; production uses a ten-minute
    /// deadline. Returning false pauses without discarding recovery information.
    pub fn worker_update_with_wait(
        &self,
        check: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
        progress: &dyn Fn(&str),
        wait: &dyn Fn(&str) -> bool,
    ) -> Result<Value> {
        self.worker_update_controlled(check, confirm, progress, wait, &|| false)
    }

    fn worker_update_controlled(
        &self,
        check: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
        progress: &dyn Fn(&str),
        wait: &dyn Fn(&str) -> bool,
        interrupted: &dyn Fn() -> bool,
    ) -> Result<Value> {
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        need(
            c.deployment["appId"].is_string(),
            "worker_update_original_profile_required",
        )?;
        let mut flow = self.state.read("worker-update", true)?;
        if flow.is_null() || flow["stage"] == "complete" {
            progress("Checking for a compatible signed worker update…");
            let discovery = self.api(&c, "worker-update", json!({}))?;
            let update = &discovery["update"];
            if update["state"] == "unavailable" {
                return Ok(
                    json!({"ok":false,"state":"unavailable","next":"No compatible signed update is published for this miner and VM. Nothing was changed; do not redeploy."}),
                );
            }
            need(
                ["available", "current"].contains(&update["state"].as_str().unwrap_or("")),
                "invalid_coordinator_response",
            )?;
            let inv = self.validate(&update["release"], false)?;
            compatible(&c, &inv, &inv["previousComposeHash"])?;
            if c.invitation["composeHash"] == inv["composeHash"] {
                need(
                    self.target(&c, false)?["compose_hash"] == inv["composeHash"],
                    "update_target_mismatch",
                )?;
                return Ok(
                    json!({"ok":true,"state":"current","release":inv["release"],"next":"This signed worker release is already installed. Run everycli health to check readiness."}),
                );
            }
            need(
                [
                    "running",
                    "drained",
                    "awaiting_attestation",
                    "awaiting_readmission",
                ]
                .contains(&c.deployment["phase"].as_str().unwrap_or("")),
                "update_not_ready",
            )?;
            need(
                self.target(&c, false)?["status"] == "running",
                "worker_not_running",
            )?;
            let status = self.api(&c, "status", json!({}))?;
            need(
                status["appId"] == c.deployment["appId"]
                    && status["composeHash"] == c.invitation["composeHash"],
                "deployment_binding_mismatch",
            )?;
            need(
                status["draining"] == true || c.deployment["providerConfigured"] == true,
                "worker_update_original_profile_required",
            )?;
            if c.deployment["providerConfigured"] == true {
                cloud::environment(&inv, &c.credentials, true)?;
            }
            if check {
                return Ok(
                    json!({"ok":true,"state":"available","release":inv["release"],"appId":inv["appId"],
                    "next":"Run everycli worker-update to install on this VM and restore its previous accepting/paused state. No changes made."}),
                );
            }
            confirm(
                "Update this existing VM, wait for attestation, reactivate previously enabled provider keys and restore its accepting/paused state? The VM may restart; active jobs drain first. Offers and discounts stay unchanged. Resumed work can incur provider charges.",
            )?;
            flow = json!({"version":1,"stage":"draining","minerId":inv["minerId"],"appId":inv["appId"],
                "fromComposeHash":c.invitation["composeHash"],"release":update["release"],
                "restoreAccepting":status["draining"] == false,"reactivate":c.deployment["providerConfigured"] == true,
                "previousAttestationAt":status["attestation"]["at"].as_i64().unwrap_or(0),"startedAt":now()});
            self.state.write("worker-update", &flow)?;
        } else {
            need(
                flow["version"] == 1
                    && flow["minerId"] == c.invitation["minerId"]
                    && flow["appId"] == c.deployment["appId"]
                    && flow["restoreAccepting"].is_boolean()
                    && flow["reactivate"].is_boolean()
                    && (flow["restoreAccepting"] != true || flow["reactivate"] == true),
                "invalid_worker_update_journal",
            )?;
            compatible(
                &c,
                &self.validate(&flow["release"], false)?,
                &flow["fromComposeHash"],
            )?;
            if check {
                return Ok(waiting(
                    &flow,
                    "An update is already in progress. This check did not change it.",
                ));
            }
            confirm(
                "Continue this recorded worker update on the same VM and restore its original accepting/paused state? No new VM or offer will be created.",
            )?;
        }
        let mut last_stage = String::new();
        loop {
            if interrupted() {
                return Ok(waiting(
                    &flow,
                    "Interrupted safely. Progress is saved; no uncertain cloud request will be repeated.",
                ));
            }
            let c = self.load(false)?;
            let inv = self.validate(&flow["release"], false)?;
            compatible(&c, &inv, &flow["fromComposeHash"])?;
            let phase = c.deployment["phase"].as_str().unwrap_or("");
            let stage_owned = s(&flow["stage"])?.to_string();
            let stage = stage_owned.as_str();
            if last_stage != stage {
                progress(match stage {
                    "draining" => "Draining new work; waiting for current jobs to finish…",
                    "applying" => "Applying the signed update to the existing VM…",
                    "attesting" => "Waiting for the updated worker and verified attestation…",
                    "activating" => "Restoring provider keys to the verified worker…",
                    "readmitting" => "Waiting for fresh attestation after key activation…",
                    "restoring" => "Restoring the previous accepting/paused state…",
                    _ => "Checking saved update state…",
                });
                last_stage = stage.into();
            }
            if interrupted() {
                return Ok(waiting(
                    &flow,
                    "Interrupted safely. Progress is saved; no further update stage was started.",
                ));
            }
            match stage {
                "draining" => {
                    need(
                        c.invitation["composeHash"] == flow["fromComposeHash"],
                        "update_target_mismatch",
                    )?;
                    let mut status = self.api(&c, "status", json!({}))?;
                    need(
                        status["appId"] == flow["appId"]
                            && status["composeHash"] == flow["fromComposeHash"],
                        "deployment_binding_mismatch",
                    )?;
                    need(status["enabled"] == true, "worker_update_miner_disabled")?;
                    if status["draining"] != true {
                        status = self.api(&c, "drain", json!({}))?;
                    }
                    need(status["draining"] == true, "drain_not_confirmed")?;
                    if status["activeJobs"] != 0 {
                        if wait("draining") {
                            continue;
                        }
                        return Ok(waiting(
                            &flow,
                            "Current jobs have not finished. The VM was not updated.",
                        ));
                    }
                    flow["stage"] = json!("applying");
                }
                "applying" => {
                    if phase == "update_provision_intent" {
                        self.reconcile_locked()?;
                        continue;
                    }
                    if phase == "activate_intent" {
                        return Err(Error("worker_update_uncertain_operation"));
                    }
                    if ["update_commit_intent", "updating"].contains(&phase) {
                        match self.reconcile_locked() {
                            Ok(_) => {}
                            Err(e) if e.0 == "update_not_observed" => {}
                            Err(e) => return Err(e),
                        }
                        let updated = self.load(false)?;
                        if updated.deployment["phase"] != "awaiting_attestation" {
                            if wait("applying") {
                                continue;
                            }
                            return Ok(waiting(
                                &flow,
                                "The cloud has not confirmed the new worker. No update request was repeated.",
                            ));
                        }
                    } else if c.invitation["composeHash"] == flow["fromComposeHash"] {
                        // update_locked records provision/commit intent before each
                        // cloud mutation. A lost response never takes this branch again.
                        self.update_locked(&flow["release"], &|_| Ok(()))?;
                        continue;
                    } else {
                        need(
                            phase == "awaiting_attestation",
                            "worker_update_state_changed",
                        )?;
                    }
                    flow["stage"] = json!("attesting");
                }
                "attesting" | "readmitting" => {
                    need(
                        c.invitation["composeHash"] == inv["composeHash"]
                            && phase
                                == if stage == "attesting" {
                                    "awaiting_attestation"
                                } else {
                                    "awaiting_readmission"
                                },
                        "worker_update_state_changed",
                    )?;
                    let status = self.api(&c, "status", json!({}))?;
                    // A disabled miner is never automatically re-enabled.
                    need(status["enabled"] == true, "worker_update_miner_disabled")?;
                    need(
                        status["appId"] == flow["appId"],
                        "deployment_binding_mismatch",
                    )?;
                    let at = status["attestation"]["at"].as_i64().unwrap_or(0);
                    let after = c.deployment["operationAt"]
                        .as_i64()
                        .unwrap_or(i64::MAX)
                        .max(flow["previousAttestationAt"].as_i64().unwrap_or(i64::MAX));
                    let admitted = status["composeHash"] == inv["composeHash"]
                        && self.ensure_ready(&c, &status).is_ok()
                        && at > after
                        && status["sessionExpires"].as_i64().is_some_and(|t| t > now())
                        && self.target(&c, false)?["status"] == "running";
                    if !admitted {
                        if wait(stage) {
                            continue;
                        }
                        return Ok(waiting(
                            &flow,
                            "Fresh attestation is not ready. Work remains paused; no verification was bypassed.",
                        ));
                    }
                    need(status["draining"] == true, "worker_update_state_changed")?;
                    flow["previousAttestationAt"] = json!(at);
                    flow["stage"] = json!(if stage == "attesting" && flow["reactivate"] == true {
                        "activating"
                    } else {
                        "restoring"
                    });
                }
                "activating" => {
                    if phase == "activate_intent" {
                        return Err(Error("worker_update_uncertain_operation"));
                    }
                    if phase != "awaiting_readmission" {
                        need(
                            phase == "awaiting_attestation",
                            "worker_update_state_changed",
                        )?;
                        let status = self.api(&c, "status", json!({}))?;
                        self.ensure_ready(&c, &status)?;
                        need(
                            status["attestation"]["at"].as_i64().unwrap_or(0)
                                >= flow["previousAttestationAt"].as_i64().unwrap_or(i64::MAX)
                                && status["attestation"]["at"].as_i64().unwrap_or(0)
                                    > c.deployment["operationAt"].as_i64().unwrap_or(i64::MAX)
                                && status["sessionExpires"].as_i64().is_some_and(|t| t > now()),
                            "post_restart_attestation_required",
                        )?;
                        self.activate_keys_locked(&|_| Ok(()), false)?;
                    }
                    flow["stage"] = json!("readmitting");
                }
                "restoring" => {
                    need(
                        [
                            "awaiting_attestation",
                            "awaiting_readmission",
                            "running",
                            "drained",
                        ]
                        .contains(&phase),
                        "worker_update_state_changed",
                    )?;
                    let status = self.api(&c, "status", json!({}))?;
                    self.ensure_ready(&c, &status)?;
                    need(
                        status["attestation"]["at"].as_i64().unwrap_or(0)
                            >= flow["previousAttestationAt"].as_i64().unwrap_or(i64::MAX),
                        "post_restart_attestation_required",
                    )?;
                    need(
                        status["sessionExpires"].as_i64().is_some_and(|t| t > now())
                            && status["attestation"]["at"].as_i64().unwrap_or(0)
                                > c.deployment["operationAt"].as_i64().unwrap_or(i64::MAX),
                        "post_restart_attestation_required",
                    )?;
                    need(
                        self.target(&c, false)?["status"] == "running",
                        "worker_not_running",
                    )?;
                    if flow["restoreAccepting"] == true {
                        need(
                            c.deployment["providerConfigured"] == true,
                            "worker_update_state_changed",
                        )?;
                        if status["draining"] == true {
                            self.resume_locked(&|_| Ok(()))?;
                        }
                        // A lost resume response is resolved by the signed status;
                        // do not repeat a successful request just to rewrite the file.
                    } else {
                        need(status["draining"] == true, "worker_update_state_changed")?;
                    }
                    let mut d = self.state.read("deployment", false)?;
                    d["phase"] = json!(if flow["restoreAccepting"] == true {
                        "running"
                    } else if flow["reactivate"] == true {
                        "drained"
                    } else {
                        "awaiting_attestation"
                    });
                    self.state.write("deployment", &d)?;
                    flow["stage"] = json!("complete");
                    flow["completedAt"] = json!(now());
                    self.state.write("worker-update", &flow)?;
                    return Ok(
                        json!({"ok":true,"state":"complete","release":inv["release"],"appId":flow["appId"],
                        "accepting":flow["restoreAccepting"],"keysReactivated":flow["reactivate"],"routingEnabled":status["routingEnabled"],
                        "next":"Worker updated on the same VM. Previous accepting/paused state restored; offers and discounts unchanged. Run everycli health and everycli offers. No paid test generation was submitted."}),
                    );
                }
                _ => return Err(Error("invalid_worker_update_journal")),
            }
            self.state.write("worker-update", &flow)?;
        }
    }
}
