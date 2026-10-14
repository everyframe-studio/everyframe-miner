//! Explicit, same-identity local authorization recovery. No cloud mutations.
use crate::{
    Error, Result,
    cloud::Cloud,
    hotkey,
    miner::{Context, Miner},
    need, now, onboarding, s,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const QUESTION: &str = "Reconnect this local profile using the same hotkey? This refreshes signed authorization and keeps API keys, the worker label and deployment history. No VM will be created, deleted, started or updated, and no work will be resumed.";

impl Miner<'_> {
    pub fn reconnect(
        &self,
        path: Option<&Path>,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.reconnect_inner(path, confirm, true)
    }

    // Setup/authorization repair must never silently retire a remote workload.
    pub fn reconnect_authorization(&self, path: Option<&Path>) -> Result<Value> {
        let _lock = self.state.lock()?;
        self.reconnect_locked(path, &|_| Ok(()))
    }

    // Caller must hold the profile lock. Deployment uses this only during its
    // read-only preflight, never after a cloud operation has been submitted.
    pub(crate) fn reconnect_locked(
        &self,
        path: Option<&Path>,
        confirm: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        self.reconnect_inner(path, confirm, false)
    }

    fn reconnect_inner(
        &self,
        path: Option<&Path>,
        confirm: &dyn Fn(&str) -> Result<()>,
        allow_retirement: bool,
    ) -> Result<Value> {
        let pending = self.state.read("profile-recovery", true)?;
        if !pending.is_null() {
            confirm(QUESTION)?;
            return self.finish_reconnect(&pending);
        }
        // Do not require the old delegate to still be valid: the wallet proves ownership.
        let config = self.state.read("config", false)?;
        let old = self.validate(&config["invitation"], true)?;
        need(
            old["publicOnboarding"] == true && old["authMode"] == "hotkey-v1",
            "hotkey_deployment_required",
        )?;
        let deployment = self.state.read("deployment", true)?;
        let flow = self.state.read("worker-update", true)?;
        need(
            flow.is_null() || flow["stage"] == "complete",
            "reconciliation_required",
        )?;
        need(
            [
                "",
                "deleted",
                "running",
                "stopped",
                "drained",
                "awaiting_attestation",
                "awaiting_readmission",
            ]
            .contains(&deployment["phase"].as_str().unwrap_or("")),
            "reconciliation_required",
        )?;
        let registration = self.state.read("registration", true)?;
        let saved_path = registration["hotkeyFile"]
            .as_str()
            .map(PathBuf::from)
            .or_else(|| {
                if path.is_some() {
                    return None;
                }
                let root = PathBuf::from(std::env::var("HOME").ok()?).join(".bittensor/wallets");
                onboarding::find_matching_hotkey(&root, old["hotkey"].as_str()?)
            });
        let path = path
            .or(saved_path.as_deref())
            .ok_or(Error("local_hotkey_required_use_wallet_or_hotkey_file"))?;
        need(
            onboarding::file_address(path)? == old["hotkey"],
            "hotkey_does_not_match_deployment",
        )?;
        confirm(QUESTION)?;
        let receipt = onboarding::check_registration(
            self.http,
            s(&self.trust["network"])?,
            s(&old["hotkey"])?,
        )?;
        need(receipt["registered"] == true, "hotkey_not_registered")?;
        let mut envelope = onboarding::enroll(self.http, &self.trust, path)?;
        let mut inv = self.validate(&envelope, false)?;
        need(
            [
                "minerId",
                "hotkey",
                "network",
                "netuid",
                "coordinatorUrl",
                "authMode",
                "publicOnboarding",
            ]
            .iter()
            .all(|k| old[k] == inv[k])
                && old["request"]["name"] == inv["request"]["name"],
            "reconnect_target_mismatch",
        )?;
        let before_credentials = self.state.read("credentials", false)?;
        need(before_credentials.is_object(), "invalid_credentials_file")?;
        let mut credentials = before_credentials.clone();
        credentials["CONSOLE_AUTH"] = hotkey::create(path, &inv, "console")?;
        credentials["WORKER_AUTH"] = hotkey::create(path, &inv, "worker")?;
        credentials.as_object_mut().unwrap().remove("MINER_TOKEN");
        Self::token(&credentials, &inv)?;
        let mut next_config = json!({"version":1,"invitation":envelope,"initializedAt":config["initializedAt"],"reconnectedAt":now()});
        let mut context = Context {
            config: next_config.clone(),
            invitation: inv.clone(),
            credentials: credentials.clone(),
            deployment: deployment.clone(),
        };
        let mut status = self.api(&context, "status", json!({}))?;
        need(
            status["composeHash"] == inv["composeHash"],
            "reconnect_target_mismatch",
        )?;
        let mut binding_retired = false;
        if deployment["appId"].is_null() && status["appId"] != "0".repeat(40) {
            need(allow_retirement, "deleted_workload_reconnect_required")?;
            let app = s(&status["appId"])?;
            let cloud = Cloud::new(self.http, &credentials)?;
            // A 404 alone is not proof of deletion: the key may belong to a
            // different workspace. Require explicit owner intent and sign it.
            match cloud.info(app) {
                Err(e) if e.0 == "phala_http_404" => {}
                Err(e) => return Err(e),
                Ok(_) => return Err(Error("remote_workload_exists_use_original_profile")),
            }
            need(
                status["activeJobs"] == 0 && status["online"] == false,
                "recovery_worker_still_online",
            )?;
            need(
                !cloud.catalog("/cvms/paginated")?["items"]
                    .as_array()
                    .ok_or(Error("invalid_cloud_catalog"))?
                    .iter()
                    .any(|v| v["name"] == inv["request"]["name"]),
                "duplicate_vm_name",
            )?;
            confirm(&format!(
                "Retire the old coordinator binding for app {app}? This Phala workspace cannot find it, but a wrong-workspace key can also return 404. Continue only if you deleted this VM. Its old authorization will be revoked. This does not delete any VM, stop storage billing, or create a new VM."
            ))?;
            envelope = onboarding::recover(
                self.http,
                &self.trust,
                path,
                &json!({"appId":app,"keyVersion":inv["keyVersion"]}),
            )?;
            let recovered = self.validate(&envelope, false)?;
            need(
                [
                    "minerId",
                    "hotkey",
                    "network",
                    "netuid",
                    "coordinatorUrl",
                    "authMode",
                    "publicOnboarding",
                ]
                .iter()
                .all(|k| recovered[k] == inv[k])
                    && recovered["request"]["name"] == inv["request"]["name"],
                "reconnect_target_mismatch",
            )?;
            inv = recovered;
            credentials["CONSOLE_AUTH"] = hotkey::create(path, &inv, "console")?;
            credentials["WORKER_AUTH"] = hotkey::create(path, &inv, "worker")?;
            next_config = json!({"version":1,"invitation":envelope,"initializedAt":config["initializedAt"],"reconnectedAt":now()});
            context.config = next_config.clone();
            context.invitation = inv.clone();
            context.credentials = credentials.clone();
            status = self.api(&context, "status", json!({}))?;
            need(
                status["composeHash"] == inv["composeHash"]
                    && status["appId"] == "0".repeat(40)
                    && status["draining"] == true
                    && status["activeJobs"] == 0,
                "invalid_recovery_response",
            )?;
            binding_retired = true;
        }
        if let Some(app) = deployment["appId"].as_str() {
            need(
                (inv["appId"] == app
                    // Initial public envelopes remain unbound after their first
                    // deployment. The fresh signed status and exact cloud target
                    // below provide the binding; upgrade envelopes cannot float.
                    || (inv["appId"].is_null() && inv["previousComposeHash"].is_null()))
                    && status["appId"] == app,
                "reconnect_target_mismatch",
            )?;
            let cloud = Cloud::new(self.http, &credentials)?;
            if deployment["phase"] == "deleted" {
                need(
                    status["activeJobs"] == 0 && status["draining"] == true,
                    "reconnect_target_mismatch",
                )?;
                match cloud.info(app) {
                    Err(e) if e.0 == "phala_http_404" => {}
                    Err(e) => return Err(e),
                    Ok(_) => return Err(Error("reconnect_target_mismatch")),
                }
            } else {
                let info = cloud.info(app)?;
                need(
                    info["app_id"] == app
                        && info["name"] == inv["request"]["name"]
                        && info["compose_hash"] == inv["composeHash"],
                    "reconnect_target_mismatch",
                )?;
            }
        } else {
            // An existing remote binding needs the original deployment record.
            need(
                inv["appId"].is_null() && status["appId"] == "0".repeat(40),
                "reconnect_target_mismatch",
            )?;
        }
        let mut next_registration = receipt;
        next_registration["hotkeyFile"] = json!(
            crate::state::absolute(path)?
                .to_str()
                .ok_or(Error("unsafe_path"))?
        );
        let journal = json!({"version":1,"deployment":deployment,"bindingRetired":binding_retired,
            "before":{"config":config,"credentials":before_credentials,"registration":registration},
            "after":{"config":next_config,"credentials":credentials,"registration":next_registration}});
        // Durable intent before any profile replacement; secrets remain owner-only.
        self.state.write("profile-recovery", &journal)?;
        self.finish_reconnect(&journal)
    }

    fn finish_reconnect(&self, journal: &Value) -> Result<Value> {
        need(journal["version"] == 1, "reconnect_state_changed")?;
        let before = self.validate(&journal["before"]["config"]["invitation"], true)?;
        let inv = self.validate(&journal["after"]["config"]["invitation"], true)?;
        need(
            [
                "minerId",
                "hotkey",
                "network",
                "netuid",
                "coordinatorUrl",
                "authMode",
                "publicOnboarding",
            ]
            .iter()
            .all(|k| before[k] == inv[k]),
            "reconnect_target_mismatch",
        )?;
        Self::token(&journal["after"]["credentials"], &inv)?;
        need(
            self.state.read("deployment", true)? == journal["deployment"],
            "reconnect_state_changed",
        )?;
        for name in ["config", "credentials", "registration"] {
            let current = self.state.read(name, true)?;
            need(
                current == journal["before"][name] || current == journal["after"][name],
                "reconnect_state_changed",
            )?;
        }
        for name in ["credentials", "config", "registration"] {
            self.state.write(name, &journal["after"][name])?;
        }
        self.state.write("profile-recovery", &Value::Null)?;
        Ok(
            json!({"state":"reconnected","minerId":inv["minerId"],"hotkey":inv["hotkey"],"uid":journal["after"]["registration"]["uid"],"workerChanged":false,"bindingRetired":journal["bindingRetired"]==true,"generationSubmitted":false,
            "next":if journal["deployment"]["phase"] == "deleted" || journal["bindingRetired"]==true {"Profile reconnected. Run everycli doctor, then review a new deployment with everycli deploy --max-hourly-usd <limit>. Nothing was deployed automatically."} else {"Profile reconnected. Run everycli doctor. A running worker was not changed; review everycli apply-api-keys separately if its authorization also needs refreshing."}}),
        )
    }
}
