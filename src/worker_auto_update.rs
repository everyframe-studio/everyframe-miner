//! Opt-in isolated in-VM automation. No management credential reaches the generation container.
//! Each timer tick uses the normal journaled same-VM lifecycle, not a second
//! deployment path. Unknown mutations are reconciled, never blindly replayed.
use crate::{Error, Result, miner::Miner, need, now};
use serde_json::{Value, json};

pub const CONSENT: &str = "Enable automatic signed worker updates inside this VM? This stores your Phala management token, scoped miner delegates and permitted provider keys in a separate attested updater container with persistent private storage. No wallet secret or billing-only provider key is transferred. The updater can manage this VM while your computer is offline. New jobs drain first; provider keys reach the generation container only after fresh attestation. VM resources and offers stay unchanged. Hosting and resumed provider work remain billable.";

impl Miner<'_> {
    pub fn worker_auto_settings(
        &self,
        enabled: Option<bool>,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        if enabled == Some(true) {
            return self.enroll_updater(confirm);
        }
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let status = self.api(&c, "worker-auto-update", json!({}))?;
        let result = if enabled == Some(false) {
            confirm(
                "Disable further automatic worker-update stages? Any in-progress operation remains journaled; the VM is not stopped or deleted. Hosting continues.",
            )?;
            self.api(
                &c,
                "worker-auto-update-policy",
                json!({"appId":status["appId"],"enabled":false}),
            )?
        } else {
            status
        };
        Ok(
            json!({"ok":true,"automaticUpdates":result["automaticUpdates"],
            "next":"The isolated updater runs inside the existing Phala VM, including while your computer is offline. Disabling does not stop VM/storage billing or clear a pending recovery journal."}),
        )
    }

    fn finish_updater_bootstrap(&self) -> Result<Value> {
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let mut settings = self.state.read("worker-auto-update", false)?;
        let status = self.api(&c, "status", json!({}))?;
        need(
            status["automaticUpdates"]["enabled"] == true
                && status["automaticUpdates"]["capsuleId"] == settings["capsuleId"]
                && status["automaticUpdates"]["appId"] == c.deployment["appId"],
            "worker_auto_update_not_enabled",
        )?;
        let ready = self.ensure_ready(&c, &status).is_ok()
            && status["attestation"]["at"].as_i64().is_some_and(|t| {
                t > c.deployment["operationAt"].as_i64().unwrap_or(i64::MAX)
                    && t > settings["previousAttestationAt"]
                        .as_i64()
                        .unwrap_or(i64::MAX)
            })
            && status["sessionExpires"].as_i64().is_some_and(|t| t > now());
        if !ready {
            return Ok(json!({"state":"waiting","reason":"bootstrap_attestation"}));
        }
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        if settings["restoreAccepting"] == true {
            if status["draining"] == true {
                self.resume_locked(&|_| Ok(()))?;
            }
        } else {
            need(status["draining"] == true, "worker_update_state_changed")?;
        }
        let mut d = self.state.read("deployment", false)?;
        d["phase"] = json!(if settings["restoreAccepting"] == true {
            "running"
        } else {
            "drained"
        });
        self.state.write("deployment", &d)?;
        settings["bootstrap"] = json!(false);
        self.state.write("worker-auto-update", &settings)?;
        Ok(json!({"state":"idle"}))
    }

    /// A single bounded step: no sleeping while holding the management lock.
    /// The pinned supervisor schedules subsequent steps and survives restarts.
    pub fn worker_auto_tick(&self) -> Result<Value> {
        let settings = self.state.read("worker-auto-update", true)?;
        if settings["enabled"] != true {
            return Ok(json!({"state":"disabled"}));
        }
        let c = self.load(false)?;
        let remote = self.api(&c, "worker-auto-update", json!({}))?;
        if remote["automaticUpdates"]["enabled"] != true
            || remote["automaticUpdates"]["capsuleId"] != settings["capsuleId"]
            || remote["automaticUpdates"]["appId"] != c.deployment["appId"]
        {
            return Ok(json!({"state":"disabled"}));
        }
        let flow = self.state.read("worker-update", true)?;
        let pending = !flow.is_null() && flow["stage"] != "complete";
        if !pending
            && settings["bootstrap"] != true
            && settings["lastCheckedAt"]
                .as_i64()
                .is_some_and(|t| t <= now() && now() - t < 300_000)
        {
            return Ok(json!({"state":"scheduled"}));
        }
        let result = if settings["bootstrap"] == true {
            self.finish_updater_bootstrap()
        } else {
            // Observe fresh admission first. A delayed controller tick must not
            // downgrade a healthy worker just because its timer elapsed offline.
            self.worker_update_automatic_with_wait(&|_| false)
                .and_then(|out| {
                    let latest = self.state.read("worker-update", true)?;
                    let deployment = self.state.read("deployment", false)?;
                    let since = latest["startedAt"]
                        .as_i64()
                        .unwrap_or(now())
                        .max(deployment["operationAt"].as_i64().unwrap_or(0));
                    let stalled = out["state"] == "waiting"
                        && latest["automatic"] == true
                        && ["attesting", "readmitting", "activating"]
                            .contains(&latest["stage"].as_str().unwrap_or(""))
                        && now() - since > 900_000;
                    if !stalled {
                        return Ok(out);
                    }
                    if latest["rollbackMode"] == true {
                        return Err(Error("worker_auto_rollback_needs_attention"));
                    }
                    self.prepare_automatic_rollback()?;
                    self.worker_update_automatic_with_wait(&|_| false)
                })
        };
        let out = match result {
            Ok(mut out) => {
                if out["state"] == "complete"
                    && self.state.read("worker-update", true)?["rollbackMode"] == true
                {
                    out["state"] = json!("rolled_back");
                    out["next"] = json!(
                        "Previous approved worker restored after fresh attestation. Failed release blocked; review the update before retrying manually."
                    );
                }
                out
            }
            Err(e) => {
                json!({"ok":false,"state":if e.0 == "worker_auto_fleet_update_busy" {"waiting"} else {"needs_attention"},"code":crate::diagnostics::safe_code(e.0)})
            }
        };
        // Re-read under lock: disabling or a changed target must not be undone.
        {
            let _lock = self.state.lock()?;
            let mut latest = self.state.read("worker-auto-update", false)?;
            if latest["appId"] == settings["appId"] && latest["minerId"] == settings["minerId"] {
                if out["state"] == "rejected" {
                    latest["blockedComposeHash"] =
                        self.state.read("worker-update", false)?["release"]["value"]["composeHash"]
                            .clone();
                }
                latest["lastCheckedAt"] = json!(now());
                latest["lastResult"] = out.clone();
                self.state.write("worker-auto-update", &latest)?;
            }
        }
        let current = self.load(false)?;
        let report_state = out["state"]
            .as_str()
            .filter(|s| {
                [
                    "idle",
                    "waiting",
                    "complete",
                    "current",
                    "unavailable",
                    "rolled_back",
                    "disabled",
                    "needs_attention",
                    "scheduled",
                ]
                .contains(s)
            })
            .unwrap_or("needs_attention");
        let report_flow = self.state.read("worker-update", true)?;
        let report_pending = !report_flow.is_null() && report_flow["stage"] != "complete";
        self.api(
            &current,
            "worker-auto-update-report",
            json!({"appId":current.deployment["appId"],"state":report_state,"code":out["code"],
            "capsuleId":settings["capsuleId"],"pending":report_pending}),
        )?;
        Ok(out)
    }

    fn prepare_automatic_rollback(&self) -> Result<()> {
        let _lock = self.state.lock()?;
        let c = self.load(false)?;
        let mut settings = self.state.read("worker-auto-update", false)?;
        let mut flow = self.state.read("worker-update", false)?;
        need(
            settings["enabled"] == true
                && settings["appId"] == c.deployment["appId"]
                && settings["minerId"] == c.invitation["minerId"]
                && flow["automatic"] == true
                && flow["rollbackMode"] != true
                && flow["appId"] == c.deployment["appId"]
                && ["attesting", "readmitting", "activating"]
                    .contains(&flow["stage"].as_str().unwrap_or(""))
                && ["awaiting_attestation", "awaiting_readmission"]
                    .contains(&c.deployment["phase"].as_str().unwrap_or("")),
            "worker_auto_rollback_needs_attention",
        )?;
        // Never reverse a cloud mutation whose outcome is still unknown.
        need(
            self.target(&c, false)?["compose_hash"] == c.invitation["composeHash"],
            "update_not_observed",
        )?;
        let status = self.api(&c, "status", json!({}))?;
        need(
            status["appId"] == flow["appId"]
                && status["enabled"] == true
                && status["draining"] == true
                && status["activeJobs"] == 0,
            "worker_auto_rollback_needs_attention",
        )?;
        let rollback = self.api(
            &c,
            "worker-rollback",
            json!({"from":c.invitation["composeHash"],"to":flow["fromComposeHash"]}),
        )?;
        let back = self.validate(&rollback["rollback"], false)?;
        crate::worker_update::compatible(&c, &back, &c.invitation["composeHash"])?;
        let saved = self.validate(&flow["rollback"], false)?;
        need(
            back["composeHash"] == saved["composeHash"] && back["request"] == saved["request"],
            "invalid_worker_recovery",
        )?;
        settings["blockedComposeHash"] = c.invitation["composeHash"].clone();
        self.state.write("worker-auto-update", &settings)?;
        flow["failedRelease"] = flow["release"].clone();
        flow["fromComposeHash"] = c.invitation["composeHash"].clone();
        flow["release"] = rollback["rollback"].clone();
        flow["rollbackMode"] = json!(true);
        flow["startedAt"] = json!(now());
        flow["previousAttestationAt"] = status["attestation"]["at"].clone();
        // Already drained and idle. Coordinator may still admit the predecessor
        // if the failed image never attested; no synthetic admission is made.
        flow["stage"] = json!("applying");
        self.state.write("worker-update", &flow)
    }
}
