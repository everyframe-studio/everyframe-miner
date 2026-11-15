//! Recovery of laptop-initiated updater enrollment. A signed read resolves lost
//! policy replies. A cloud PATCH is never replayed by this recovery path.
use crate::{
    Error, Result,
    miner::{Context, Miner},
    need, now,
};
use serde_json::{Value, json};

pub(crate) fn credential_hash(credentials: &Value) -> Result<String> {
    let mut plain = credentials.clone();
    plain
        .as_object_mut()
        .ok_or(Error("invalid_credentials"))?
        .remove("UPDATER_SEED");
    crate::protocol::digest(&plain)
}

fn policy_matches(status: &Value, app: &Value, enabled: bool, capsule: &Value) -> bool {
    status["appId"] == *app
        && status["automaticUpdates"]["appId"] == *app
        && status["automaticUpdates"]["enabled"] == enabled
        && (capsule.is_null() || status["automaticUpdates"]["capsuleId"] == *capsule)
}

impl Miner<'_> {
    pub(crate) fn updater_enrollment_observed(&self, c: &Context, journal: &Value) -> bool {
        let Ok(status) = self.api(c, "worker-auto-update", json!({})) else {
            return false;
        };
        let report = &status["automaticUpdates"]["report"];
        policy_matches(&status, &journal["appId"], true, &journal["capsuleId"])
            && report["composeHash"] == journal["composeHash"]
            && report["at"]
                .as_i64()
                .is_some_and(|at| at > c.deployment["operationAt"].as_i64().unwrap_or(i64::MAX))
    }

    /// Exactly one mutation; only a fresh signed observation can turn an error
    /// into success. Even HTTP 400 can follow a committed database write.
    pub(crate) fn set_updater_policy_observed(&self, c: &Context, payload: Value) -> Result<Value> {
        let enabled = payload["enabled"]
            .as_bool()
            .ok_or(Error("invalid_auto_update_policy"))?;
        match self.api(c, "worker-auto-update-policy", payload.clone()) {
            Ok(status) => {
                need(
                    policy_matches(&status, &payload["appId"], enabled, &payload["capsuleId"]),
                    "updater_policy_not_confirmed",
                )?;
                Ok(status)
            }
            Err(error) => match self.api(c, "worker-auto-update", json!({})) {
                Ok(status)
                    if policy_matches(
                        &status,
                        &payload["appId"],
                        enabled,
                        &payload["capsuleId"],
                    ) =>
                {
                    Ok(status)
                }
                Ok(_) => Err(error),
                Err(_) => Err(Error("updater_policy_result_unknown").caused_by(&error)),
            },
        }
    }

    pub(crate) fn finish_updater_enrollment_record(&self) -> Result<()> {
        // Remove copied credential material after the operation is resolved.
        self.state.write(
            "updater-enrollment",
            &json!({"version":1,"stage":"complete","completedAt":now()}),
        )
    }

    /// Restore only the exact pre-enrollment VM and capsule. If the PATCH may
    /// have been sent, normal activation reconciliation must run first.
    pub(crate) fn recover_updater_enrollment_locked(&self) -> Result<Option<Value>> {
        let journal = self.state.read("updater-enrollment", true)?;
        if journal.is_null() || journal["stage"] == "complete" {
            return Ok(None);
        }
        let c = self.load(false)?;
        need(
            journal["version"] == 1
                && journal["appId"] == c.deployment["appId"]
                && journal["composeHash"] == c.invitation["composeHash"]
                && journal["credentialHash"] == credential_hash(&c.credentials)?,
            "updater_enrollment_state_changed",
        )?;
        if ["installing", "cloud_pending"].contains(&journal["stage"].as_str().unwrap_or("")) {
            if c.deployment["phase"] == "activate_intent"
                || journal["stage"] == "cloud_pending"
                    && c.deployment["phase"] != "awaiting_readmission"
            {
                return Err(Error("updater_enrollment_recovery_pending"));
            }
            if c.deployment["phase"] == "awaiting_readmission" {
                self.finish_updater_enrollment_record()?;
                return Ok(Some(json!({"ok":true,"state":"enrolling","enabled":true,
                    "next":"The enrollment restart is recorded. The in-VM updater will wait for fresh attestation and restore the original accepting/paused state. Check everycli worker-auto-update."})));
            }
        }
        need(
            ["running", "drained"].contains(&c.deployment["phase"].as_str().unwrap_or("")),
            "updater_enrollment_recovery_pending",
        )?;
        let mut status = self.api(&c, "worker-auto-update", json!({}))?;
        let old = &journal["previousPolicy"];
        need(
            status["appId"] == journal["appId"]
                && status["composeHash"] == journal["composeHash"]
                && (status["automaticUpdates"]["capsuleId"] == old["capsuleId"]
                    || status["automaticUpdates"]["capsuleId"] == journal["capsuleId"])
                && status["automaticUpdates"]["report"]["pending"] != true,
            "updater_enrollment_state_changed",
        )?;
        self.ensure_ready(&c, &status)?;
        need(
            self.target(&c, false)?["status"] == "running",
            "worker_not_running",
        )?;
        // Stop a selected new capsule before restoring local credentials or
        // accepting work. No new capsule was installed on this VM.
        if status["automaticUpdates"]["enabled"] == true {
            status = self.set_updater_policy_observed(
                &c,
                json!({"appId":journal["appId"],"enabled":false}),
            )?;
        }
        if !old["capsuleId"].is_null()
            && status["automaticUpdates"]["capsuleId"] != old["capsuleId"]
        {
            self.set_updater_policy_observed(
                &c,
                json!({"appId":journal["appId"],
                "enabled":false,"capsuleId":old["capsuleId"]}),
            )?;
        }
        let mut credentials = c.credentials.clone();
        if journal["previousSeed"].is_null() {
            credentials.as_object_mut().unwrap().remove("UPDATER_SEED");
        } else {
            credentials["UPDATER_SEED"] = journal["previousSeed"].clone();
        }
        self.state.write("credentials", &credentials)?;
        self.state
            .write("worker-auto-update", &journal["previousLocalSettings"])?;
        if journal["restoreAccepting"] == true && status["draining"] == true {
            // Resume itself is idempotent, but a lost response must still be
            // observed before declaring recovery complete.
            if let Err(error) = self.resume_locked(&|_| Ok(())) {
                let observed = self.api(&c, "worker-auto-update", json!({}))?;
                if observed["draining"] != false {
                    return Err(error);
                }
                self.ensure_ready(&c, &observed)?;
            }
        } else if journal["restoreAccepting"] != true {
            need(
                status["draining"] == true,
                "updater_enrollment_state_changed",
            )?;
        }
        // Manual resume disables automatic updates. Restore the old policy
        // last, after accepting/paused state and credentials are restored.
        if !old["capsuleId"].is_null() {
            self.set_updater_policy_observed(
                &c,
                json!({"appId":journal["appId"],
                "enabled":old["enabled"] == true}),
            )?;
        }
        self.finish_updater_enrollment_record()?;
        Ok(Some(json!({"ok":true,"state":"recovered","resolved":true,
            "draining":journal["restoreAccepting"] != true,"enabled":old["enabled"] == true,
            "next":"The previous accepting/paused state and automatic-update policy were restored. No VM restart was submitted. Run everycli worker-auto-update --enable to try enrollment again."})))
    }

    pub(crate) fn failed_updater_enrollment(&self, error: Error) -> Error {
        if let Ok(deployment) = self.state.read("deployment", true)
            && deployment["phase"] == "activate_intent"
            && let Ok(mut journal) = self.state.read("updater-enrollment", false)
        {
            journal["stage"] = json!("cloud_pending");
            let _ = self.state.write("updater-enrollment", &journal);
        }
        match self.recover_updater_enrollment_locked() {
            Ok(Some(_)) => Error("updater_enrollment_restored").caused_by(&error),
            _ => Error("updater_enrollment_recovery_pending").caused_by(&error),
        }
    }
}
