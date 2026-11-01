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

pub(crate) fn compatible(c: &Context, inv: &Value, from: &Value) -> Result<()> {
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
        "next":"Rerun everycli worker-update using this same profile to continue, or run everycli worker-update --abort to cancel it and keep the current worker. Do not redeploy. No offers were changed."})
}

fn release_expired(flow: &Value, installed: bool) -> Value {
    let (reason, next) = if installed {
        (
            "The signed worker release expired after it was installed, so it can no longer authorize key activation or resume.",
            "Run everycli worker-update --abort to close this update, then renew the signed profile with everycli init --wallet <wallet> --hotkey default, then run everycli activate and everycli resume. The updated VM keeps running; do not redeploy.",
        )
    } else {
        (
            "The signed worker release expired before it was installed. The VM was not changed by this update.",
            "Run everycli worker-update --abort to cancel this update and keep the current worker, then run everycli worker-update to fetch a fresh signed release. Work stays paused until you resume or a new update finishes. Do not redeploy.",
        )
    };
    json!({"ok":false,"state":"release_expired","stage":flow["stage"],"appId":flow["appId"],
        "release":flow["release"]["value"]["release"],"reason":reason,"next":next})
}

/// Phases left by an interrupted cloud operation; reconcile settles them.
fn unsettled(phase: &str) -> bool {
    (phase.ends_with("_intent") && phase != "delete_intent")
        || ["provisioned", "starting", "stopping", "updating"].contains(&phase)
}

impl Miner<'_> {
    /// Adopt only a freshly signed replacement for a pending update whose
    /// environment allow-list was reordered by Phala. No cloud mutation or
    /// credential release occurs here, and no unsigned hash is accepted.
    pub(crate) fn reconcile_environment_order(&self, c: &mut Context) -> Result<Value> {
        need(
            ["update_commit_intent", "updating"]
                .contains(&c.deployment["phase"].as_str().unwrap_or("")),
            "unapproved_running_compose",
        )?;
        let pending = self.validate(&c.deployment["pendingInvitation"], true)?;
        compatible(c, &pending, &c.invitation["composeHash"])?;
        let mut flow = self.state.read("worker-update", false)?;
        need(
            flow["version"] == 1
                && flow["stage"] == "applying"
                && flow["minerId"] == c.invitation["minerId"]
                && flow["appId"] == c.deployment["appId"]
                && flow["fromComposeHash"] == c.invitation["composeHash"],
            "unapproved_running_compose",
        )?;
        let info = self.cloud(c)?.info(s(&c.deployment["appId"])?)?;
        need(
            info["app_id"] == c.deployment["appId"]
                && info["name"] == c.invitation["request"]["name"]
                && info["status"] == "running",
            "unapproved_running_compose",
        )?;
        let response = self.api(c, "worker-update", json!({"providerEnvironment":"pika-v1"}))?;
        need(
            ["available", "current"].contains(&response["update"]["state"].as_str().unwrap_or("")),
            "unapproved_running_compose",
        )?;
        let envelope = &response["update"]["release"];
        let corrected = self.validate(envelope, false)?;
        compatible(c, &corrected, &c.invitation["composeHash"])?;
        let mut request = pending["request"].clone();
        let before = &request["compose_file"]["allowed_envs"];
        let after = &corrected["request"]["compose_file"]["allowed_envs"];
        let names = |v: &Value| -> Result<std::collections::BTreeSet<String>> {
            let values = v.as_array().ok_or(Error("unapproved_running_compose"))?;
            let set: std::collections::BTreeSet<String> = values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or(Error("unapproved_running_compose"))
                })
                .collect::<Result<_>>()?;
            need(set.len() == values.len(), "unapproved_running_compose")?;
            Ok(set)
        };
        need(
            names(before)? == names(after)?,
            "unapproved_running_compose",
        )?;
        request["compose_file"]["allowed_envs"] = after.clone();
        need(
            request == corrected["request"]
                && pending["models"] == corrected["models"]
                && pending["release"] == corrected["release"]
                && corrected["composeHash"] == info["compose_hash"],
            "unapproved_running_compose",
        )?;
        let prior = self.validate(&flow["release"], true)?;
        need(
            prior["composeHash"] == pending["composeHash"]
                || prior["composeHash"] == corrected["composeHash"],
            "unapproved_running_compose",
        )?;
        flow["release"] = envelope.clone();
        flow["environmentOrderReconciledAt"] = json!(now());
        self.state.write("worker-update", &flow)?;
        c.deployment["pendingInvitation"] = envelope.clone();
        c.deployment["environmentOrderReconciledAt"] = json!(now());
        self.state.write("deployment", &c.deployment)?;
        Ok(info)
    }

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
            false,
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
        self.worker_update_controlled(check, confirm, progress, wait, &|| false, false)
    }

    pub fn worker_update_automatic_with_wait(&self, wait: &dyn Fn(&str) -> bool) -> Result<Value> {
        self.worker_update_controlled(false, &|_| Ok(()), &|_| {}, wait, &|| false, true)
    }

    fn worker_update_controlled(
        &self,
        check: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
        progress: &dyn Fn(&str),
        wait: &dyn Fn(&str) -> bool,
        interrupted: &dyn Fn() -> bool,
        automatic: bool,
    ) -> Result<Value> {
        match self.worker_update_run(check, confirm, progress, wait, interrupted, automatic) {
            // A release (or the profile's adopted copy of it) expiring mid-update
            // stops with recovery steps instead of a bare validation error.
            Err(e) if e.0 == "invitation_expired" => {
                let flow = self.state.read("worker-update", true)?;
                if flow.is_null() || flow["stage"] == "complete" {
                    return Err(e);
                }
                let config = self.state.read("config", false)?;
                let installed = config["invitation"]["value"]["composeHash"]
                    == flow["release"]["value"]["composeHash"];
                Ok(release_expired(&flow, installed))
            }
            other => other,
        }
    }

    /// Mark the saved flow finished without completing it. Readers that only
    /// know "complete" (reconnect, preparation recovery) are then unblocked.
    fn close_flow(&self, flow: &mut Value, reason: &str) -> Result<()> {
        if !flow.is_object() {
            return Ok(());
        }
        flow["abortedFromStage"] = flow["stage"].clone();
        flow["stage"] = json!("complete");
        flow["aborted"] = json!(reason);
        flow["completedAt"] = json!(now());
        self.state.write("worker-update", flow)
    }

    fn close_rejected_update(&self, flow: &mut Value, settled: &Value) -> Result<Value> {
        self.close_flow(flow, "rejected")?;
        let paused = if flow["restoreAccepting"] == true {
            " The update paused new work; it stays paused until you run everycli resume."
        } else {
            ""
        };
        Ok(
            json!({"ok":false,"state":"rejected","appId":flow["appId"],"phase":settled["phase"],
            "release":flow["release"]["value"]["release"],"rejectedCode":settled["rejectedCode"],
            "reason":"Phala rejected the signed worker update; the existing worker is unchanged and this update was closed.",
            "next":format!("{}{paused} To try again later, rerun everycli worker-update. Do not redeploy; offers were not changed.", settled["next"].as_str().unwrap_or(""))}),
        )
    }

    /// Cancel an unfinished worker update. Settles any recorded cloud intent
    /// from observed state (never replaying it), keeps whatever signed release
    /// the VM actually runs, and closes the saved flow. Safe when idle.
    pub fn worker_update_abort(&self) -> Result<Value> {
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let mut flow = self.state.read("worker-update", true)?;
        let flow_pending = !flow.is_null() && flow["stage"] != "complete";
        let phase = c.deployment["phase"].as_str().unwrap_or("").to_string();
        let update_intent = [
            "update_provision_intent",
            "update_commit_intent",
            "updating",
        ]
        .contains(&phase.as_str());
        if !flow_pending && !update_intent {
            let next = if unsettled(&phase) {
                "Nothing was changed. A different operation is unfinished; run everycli reconcile to settle it."
            } else {
                "Nothing was changed. Run everycli worker-update --check to look for a signed update."
            };
            return Ok(
                json!({"ok":true,"state":"nothing_pending","aborted":false,"phase":c.deployment["phase"],
                "reason":"No worker update was in progress.","next":next}),
            );
        }
        let mut done = vec![];
        if update_intent {
            let info = self.target(&c, true)?;
            if phase != "update_provision_intent"
                && info["compose_hash"] != c.invitation["composeHash"]
            {
                // target() proved the VM runs the pending signed release: it
                // already landed, so keep it rather than pretend to undo it.
                let settled = self.reconcile_locked()?;
                need(
                    settled["phase"] == "awaiting_attestation",
                    "update_in_progress_cannot_abort",
                )?;
                done.push("The update had already reached the VM, so it was kept: this profile now tracks the release that is running.");
            } else if phase == "update_provision_intent" {
                // Preparation only; the existing reconcile path verifies the
                // old worker is unchanged before dropping the candidate.
                self.reconcile_locked()?;
                done.push("The prepared (uncommitted) update was discarded; the VM still runs the previous release.");
            } else {
                let uncertain = c.deployment["commitRejected"].is_null();
                self.restore_unchanged_worker(&c, s(&info["status"])?, uncertain)?;
                done.push(if uncertain {
                    "The pending update was cancelled; the VM still runs the previous release. If Phala applies the earlier request later, everycli reconcile will adopt it."
                } else {
                    "Phala had rejected the update; the VM still runs the previous release."
                });
            }
        } else if phase == "activate_intent" {
            self.reconcile_locked()?;
            done.push(
                "The interrupted key activation was settled from the worker's observed state.",
            );
        }
        let stage = flow["stage"].as_str().unwrap_or("").to_string();
        if flow_pending {
            self.close_flow(&mut flow, "aborted")?;
            done.push("The saved worker-update journal was closed.");
        }
        let d = self.state.read("deployment", false)?;
        let phase = d["phase"].as_str().unwrap_or("");
        let mut next = match phase {
            "running" if stage == "draining" || stage == "applying" => {
                "If everycli status shows the miner draining, run everycli resume to accept work again."
            }
            "running" => "The worker keeps its current state.",
            "drained" => {
                "Provider keys remain active and work remains paused. Run everycli resume when you are ready to accept provider-billed work."
            }
            "awaiting_readmission" => {
                "Wait for fresh accepted attestation in everycli status, then run everycli resume."
            }
            "stopped" => "The VM is stopped. Run everycli start --max-hourly-usd N when you want it back.",
            _ => {
                "Provider keys are not active on this worker. Once everycli status shows accepted attestation, run everycli activate, then everycli resume."
            }
        }
        .to_string();
        next.push_str(" Run everycli worker-update later to start a fresh update. No VM was created or deleted and offers were not changed.");
        let config = self.state.read("config", false)?;
        Ok(
            json!({"ok":true,"state":"aborted","aborted":true,"phase":phase,"appId":d["appId"],
            "release":config["invitation"]["value"]["release"],"reason":done.join(" "),"next":next}),
        )
    }

    /// The saved release, verified. After the update reached the VM (or its
    /// commit may have), an elapsed signature window must not strand the
    /// worker half-updated; before that, an expired release stops the update.
    fn flow_release(&self, c: &Context, flow: &Value) -> Result<Value> {
        match self.validate(&flow["release"], false) {
            Err(e) if e.0 == "invitation_expired" => {
                let inv = self.validate(&flow["release"], true)?;
                let phase = c.deployment["phase"].as_str().unwrap_or("");
                need(
                    c.invitation["composeHash"] == inv["composeHash"]
                        || ["update_commit_intent", "updating"].contains(&phase),
                    "invitation_expired",
                )?;
                Ok(inv)
            }
            other => other,
        }
    }

    fn worker_update_run(
        &self,
        check: bool,
        confirm: &dyn Fn(&str) -> Result<()>,
        progress: &dyn Fn(&str),
        wait: &dyn Fn(&str) -> bool,
        interrupted: &dyn Fn() -> bool,
        automatic: bool,
    ) -> Result<Value> {
        let _lock = self.state.lock()?;
        let mut c = self.load(false)?;
        let settings = self.state.read("worker-auto-update", true)?;
        if automatic {
            need(
                settings["enabled"] == true
                    && settings["appId"] == c.deployment["appId"]
                    && settings["minerId"] == c.invitation["minerId"],
                "worker_auto_update_not_enabled",
            )?;
            need(
                settings["credentialHash"] == crate::protocol::digest(&c.credentials)?,
                "worker_auto_credentials_changed",
            )?;
            let status = self.api(&c, "worker-auto-update", json!({}))?;
            need(
                status["automaticUpdates"]["enabled"] == true
                    && status["automaticUpdates"]["capsuleId"] == settings["capsuleId"]
                    && status["automaticUpdates"]["appId"] == c.deployment["appId"],
                "worker_auto_update_not_enabled",
            )?;
        }
        need(
            c.deployment["appId"].is_string(),
            "worker_update_original_profile_required",
        )?;
        let mut flow = self.state.read("worker-update", true)?;
        if automatic && !flow.is_null() && flow["stage"] != "complete" {
            need(flow["automatic"] == true, "manual_worker_update_pending")?;
        }
        if flow.is_null() || flow["stage"] == "complete" {
            // An interrupted activation, update or start/stop must be settled
            // from observed cloud state before a new update can be planned.
            if unsettled(c.deployment["phase"].as_str().unwrap_or("")) {
                need(!check, "reconciliation_required")?;
                progress("Settling an unfinished operation from the observed cloud state…");
                self.reconcile_locked()?;
                c = self.load(false)?;
                need(
                    !unsettled(c.deployment["phase"].as_str().unwrap_or("")),
                    "reconciliation_required",
                )?;
            }
            progress("Checking for a compatible signed worker update…");
            // Older offline controllers keep a compatible environment. A
            // reviewed manual update can add Pika and migrate its controller.
            let discovery = self.api(
                &c,
                "worker-update",
                if automatic {
                    json!({})
                } else {
                    json!({"providerEnvironment":"pika-v1"})
                },
            )?;
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
            if automatic {
                need(
                    settings["blockedComposeHash"] != inv["composeHash"],
                    "worker_auto_update_release_blocked",
                )?;
                let mut before = c.invitation["request"].clone();
                let mut after = inv["request"].clone();
                before
                    .as_object_mut()
                    .ok_or(Error("update_target_mismatch"))?
                    .remove("compose_file");
                after
                    .as_object_mut()
                    .ok_or(Error("update_target_mismatch"))?
                    .remove("compose_file");
                need(before == after, "worker_auto_update_resources_changed")?;
                need(
                    crate::updater_vm::image(&c.invitation["request"]["compose_file"])?
                        == crate::updater_vm::image(&inv["request"]["compose_file"])?,
                    "worker_auto_update_resources_changed",
                )?;
            }
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
            let controller = |compose: &Value| -> Result<Option<String>> {
                if compose["allowed_envs"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "EVERYFRAME_UPDATER_SEED"))
                {
                    crate::updater_vm::image(compose).map(Some)
                } else {
                    Ok(None)
                }
            };
            if controller(&c.invitation["request"]["compose_file"])?
                != controller(&inv["request"]["compose_file"])?
            {
                confirm(
                    "This signed release upgrades the isolated automatic-update controller on the same VM. Its persistent state may contain the management credentials you previously enrolled. Review this controller migration before continuing.",
                )?;
            }
            confirm(
                "Update this existing VM, wait for attestation, reactivate previously enabled provider keys and restore its accepting/paused state? The VM may restart; active jobs drain first. Offers and discounts stay unchanged. Resumed work can incur provider charges.",
            )?;
            flow = json!({"version":1,"stage":"draining","minerId":inv["minerId"],"appId":inv["appId"],
                "fromComposeHash":c.invitation["composeHash"],"release":update["release"],
                "restoreAccepting":status["draining"] == false,"reactivate":c.deployment["providerConfigured"] == true,
                "previousAttestationAt":status["attestation"]["at"].as_i64().unwrap_or(0),"startedAt":now()});
            if automatic {
                // Recovery must be authorized BEFORE draining or cloud mutation.
                let recovery = self.api(
                    &c,
                    "worker-update-prepare",
                    json!({"from":c.invitation["composeHash"],"to":inv["composeHash"]}),
                )?;
                let back = self.validate(&recovery["rollback"], false)?;
                need(
                    back["composeHash"] == c.invitation["composeHash"]
                        && back["previousComposeHash"] == inv["composeHash"]
                        && back["request"] == c.invitation["request"]
                        && back["appId"] == c.deployment["appId"]
                        && back["minerId"] == c.invitation["minerId"],
                    "invalid_worker_recovery",
                )?;
                flow["automatic"] = json!(true);
                flow["rollback"] = recovery["rollback"].clone();
            }
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
            compatible(&c, &self.flow_release(&c, &flow)?, &flow["fromComposeHash"])?;
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
            if automatic {
                let status = self.api(&c, "worker-auto-update", json!({}))?;
                need(
                    status["automaticUpdates"]["enabled"] == true
                        && status["automaticUpdates"]["capsuleId"] == settings["capsuleId"]
                        && status["automaticUpdates"]["appId"] == c.deployment["appId"],
                    "worker_auto_update_not_enabled",
                )?;
            }
            if automatic {
                let status = self.api(&c, "worker-auto-update", json!({}))?;
                need(
                    status["automaticUpdates"]["leaseHeld"] == true,
                    "worker_auto_update_lease_lost",
                )?;
            }
            let inv = self.flow_release(&c, &flow)?;
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
                        // Settle from observed state (confirm or roll back);
                        // the env request itself is never replayed here.
                        self.reconcile_locked()?;
                        continue;
                    }
                    if ["update_commit_intent", "updating"].contains(&phase) {
                        match self.reconcile_locked() {
                            Ok(v) if v["update"] == "rolled_back" => {
                                return self.close_rejected_update(&mut flow, &v);
                            }
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
                        if let Err(e) = self.update_locked(&flow["release"], &|_| Ok(())) {
                            // A definitive commit rejection is journaled; the next
                            // pass verifies the old worker and closes the update.
                            if self.state.read("deployment", false)?["commitRejected"].is_object() {
                                continue;
                            }
                            return Err(e);
                        }
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
                        // A lost activation response: confirm it from the
                        // worker's own evidence or roll back, then continue.
                        self.reconcile_locked()?;
                        continue;
                    }
                    if phase != "awaiting_readmission" {
                        need(
                            phase == "awaiting_attestation",
                            "worker_update_state_changed",
                        )?;
                        let status = self.api(&c, "status", json!({}))?;
                        let fresh = self.ensure_ready(&c, &status).is_ok()
                            && status["attestation"]["at"].as_i64().unwrap_or(0)
                                >= flow["previousAttestationAt"].as_i64().unwrap_or(i64::MAX)
                            && status["attestation"]["at"].as_i64().unwrap_or(0)
                                > c.deployment["operationAt"].as_i64().unwrap_or(i64::MAX)
                            && status["sessionExpires"].as_i64().is_some_and(|t| t > now());
                        if !fresh {
                            // After a rolled-back activation, wait for an
                            // attestation newer than that request.
                            if wait(stage) {
                                continue;
                            }
                            return Ok(waiting(
                                &flow,
                                "Fresh attestation is not ready for key activation. Work remains paused; no verification was bypassed.",
                            ));
                        }
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
