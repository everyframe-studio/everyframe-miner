//! Fixed diagnostic vocabulary. Never include remote bodies, URLs or error strings.
use crate::Error;
use serde_json::{Value, json};

// The scoped code identifies Phala requests without retaining credential-bearing URLs.
macro_rules! reasons {
    ($(($code:literal, $message:literal, $next:literal)),* $(,)?) => {
        const REASONS: &[(&str, &str, &str, &str)] = &[
            $(($code, concat!("phala_", $code), $message, $next)),*
        ];
    };
}
reasons![
    (
        "kling_auth_rejected",
        "Kling rejected the developer API key or its permissions.",
        "Use everycli set-api-keys --provider kling with a current Kling developer API key, not a legacy access/secret pair."
    ),
    (
        "kling_billing_required",
        "Kling reports arrears or an exhausted or expired API resource pack.",
        "Review the Kling developer billing page. A website subscription does not fund these API jobs."
    ),
    (
        "kling_capacity_limited",
        "Kling's account concurrency or request limit was reached.",
        "Wait for running tasks to finish. Do not resubmit a task whose acceptance is uncertain."
    ),
    (
        "kling_content_rejected",
        "Kling rejected the request under its content policy.",
        "Review the request. It will not be retried on another miner to bypass moderation."
    ),
    (
        "kling_invalid_parameters",
        "Kling rejected the request parameters or access configuration.",
        "Check the supported model settings and developer console IP restrictions."
    ),
    (
        "kling_unavailable",
        "Kling could not complete the API request.",
        "Check task status before retrying. A lost submission response does not prove that no charge occurred."
    ),
    (
        "worker_recovery_unavailable",
        "The previous worker is no longer authorized for automatic recovery.",
        "Keep the VM paused and preserve its journal. Review the expired or revoked recovery with support; do not bypass attestation."
    ),
    (
        "worker_recovery_in_progress",
        "A previous recovery is still in progress.",
        "Finish the recorded recovery before requesting another update. Do not clear the journal."
    ),
    (
        "worker_recovery_not_idle",
        "The worker is not paused and idle for this operation.",
        "Let active jobs finish and inspect worker status. No cloud operation was authorized by this response."
    ),
    (
        "worker_recovery_target_changed",
        "The worker's deployment or authorization changed after recovery was prepared.",
        "Recovery is blocked. Reconcile the exact VM and keep its journal; do not recreate it or release keys to an unverified image."
    ),
    (
        "worker_update_target_changed",
        "The approved update changed before preparation completed.",
        "Check the signed release again. No new update was authorized by this response."
    ),
    (
        "invalid_updater_capsule",
        "The updater enrollment identifier is missing or invalid.",
        "Use explicit enrollment for this exact VM. Never share or edit the credential capsule."
    ),
    (
        "updater_enrollment_busy",
        "The worker has active jobs; updater enrollment has not started.",
        "Wait for the jobs to finish and enable automatic updates again. No keys were transferred and the worker was not paused."
    ),
    (
        "worker_auto_fleet_update_busy",
        "Another miner is currently updating.",
        "The updater will check again. Your worker has not been drained or restarted; automatic rollouts are serialized."
    ),
    (
        "worker_auto_update_lease_lost",
        "The automatic update's rollout lease is no longer valid.",
        "No further cloud mutation was sent. Inspect the saved update and coordinator status before resuming recovery."
    ),
    (
        "worker_auto_update_recovery_pending",
        "The existing updater has an unfinished operation.",
        "Finish recovery before replacing updater credentials. The persistent journal was preserved and no new VM was created."
    ),
    (
        "worker_auto_update_disable_first",
        "The current updater must be disabled before replacing its enrollment.",
        "Disable automatic updates, let any current operation settle, then explicitly re-enroll."
    ),
    (
        "invalid_updater_seed",
        "The encrypted updater enrollment does not match the coordinator's current authorization.",
        "No credentials or journals were replaced. Check the current enrollment and same-VM binding."
    ),
    (
        "updater_seed_too_large",
        "The updater enrollment exceeds the safe size limit.",
        "No enrollment was submitted. Contact support with this diagnostic code; never share the credential capsule."
    ),
    (
        "updater_already_enrolled",
        "This updater enrollment is already installed.",
        "The persistent journal was kept. Check automatic-update status instead of replaying enrollment."
    ),
    (
        "updater_nonexpiring_delegate_required",
        "The profile still uses expiring miner delegates.",
        "Reconnect with the same hotkey to obtain the non-expiring, revocable delegates before enrolling the offline updater. Never transfer the wallet secret."
    ),
    (
        "worker_auto_credentials_changed",
        "The updater's credentials changed after consent.",
        "Automatic mutations are paused. Review the credential change and re-enroll explicitly; no new key was silently released."
    ),
    (
        "worker_auto_update_bootstrap_required",
        "This worker does not include the signed isolated updater yet.",
        "Install the approved bootstrap with everycli worker-update, wait for fresh attestation, then enable automatic updates. No credentials were transferred."
    ),
    (
        "worker_auto_update_not_enabled",
        "Automatic updates are disabled or belong to a different VM.",
        "Review everycli worker-auto-update. Existing miners remain manual until explicitly enrolled."
    ),
    (
        "worker_auto_update_resources_changed",
        "The proposed update changes VM resources or the pinned updater.",
        "Review this migration manually. Automatic updates never resize, replace the OS or replace their own recovery controller."
    ),
    (
        "worker_auto_update_release_blocked",
        "This release previously failed and is blocked from automatic retry.",
        "Check the updater report. Review a corrected signed release; no repeated restart loop was started."
    ),
    (
        "worker_auto_rollback_needs_attention",
        "Automatic recovery cannot safely continue; work remains paused.",
        "Inspect worker status and the cloud workload. Preserve the recovery journal; do not recreate the VM or bypass attestation."
    ),
    (
        "manual_worker_update_pending",
        "A manually started update is unfinished.",
        "Finish or abort that update explicitly. Automatic updates will not take over a manual operation."
    ),
    (
        "invalid_worker_recovery",
        "The signed recovery does not match this VM's previous deployment.",
        "No rollback was performed. Contact support with the public miner ID and diagnostic code."
    ),
    (
        "updater_not_isolated",
        "The signed workload does not isolate the updater as required.",
        "Do not release management credentials. Use an approved bootstrap release."
    ),
    (
        "reconciliation_required",
        "A previous deployment operation is unfinished; saved keys have not been changed.",
        "Run everycli reconcile in this same profile, or choose Re-sync operation in the UI. Do not delete profile files or deploy another VM."
    ),
    (
        "preparation_recovery_conflict",
        "The unfinished preparation cannot be cleared safely because another workload or operation may exist.",
        "Check everycli status and your Phala workspace. Keep the profile and journal; do not retry deployment or delete local files."
    ),
    (
        "manual_reconciliation_required",
        "The operation journal does not identify a workload that can be safely reconciled automatically.",
        "Update everycli and run reconcile in the original profile. If unresolved, contact support with your public miner ID and sanitized status; do not delete the journal or repeat deployment."
    ),
    (
        "configuration_refresh_changed_profile",
        "The coordinator's setup profile differs from the workload managed here. Local files and the VM were not changed.",
        "Run everycli reconnect with the same hotkey to synchronize the profile. Do not redeploy an existing worker."
    ),
    (
        "configuration_refresh_profile_race",
        "The local setup profile changed while it was being refreshed; it was not overwritten.",
        "Finish the other management command, then retry from the same profile."
    ),
    (
        "worker_not_running",
        "The existing Phala VM is not running, so it cannot accept jobs.",
        "Run everycli status. If Phala reports stopped, use everycli start --max-hourly-usd 10 and review the hosting quote. Keep the existing VM; do not redeploy."
    ),
    (
        "coordinator_auth_rejected",
        "The coordinator rejected this profile's miner authorization. This is not a Phala API key error.",
        "Check that your computer's clock is correct. Run everycli reconnect with the same hotkey to refresh the signed profile. No VM is created and saved API keys are kept."
    ),
    (
        "profile_recovery_pending",
        "An approved local profile reconnect was interrupted.",
        "Run everycli reconnect in this same profile to finish safely. Do not delete profile files or deploy another VM."
    ),
    (
        "reconnect_target_mismatch",
        "The fresh signed profile does not safely match this profile's existing workload.",
        "Nothing was deployed. Check everycli status and the original management profile; finish any pending worker update before reconnecting."
    ),
    (
        "reconnect_state_changed",
        "Local files changed during profile recovery; they were not overwritten.",
        "Preserve the profile and its recovery journal. Do not delete files or retry deployment; resolve the concurrent local change first."
    ),
    (
        "coordinator_unavailable_use_stop_force",
        "The coordinator could not confirm draining; no cloud shutdown was sent.",
        "To stop compute anyway, run everycli stop --force. This can lose paid in-flight work. Storage remains billable."
    ),
    (
        "source_build_update_disabled",
        "This executable was built from source; automatic binary replacement is disabled.",
        "Rebuild from source, or explicitly install the official binary with the documented installer. everycli update --check still works."
    ),
    (
        "update_binary_unusable",
        "The downloaded executable failed its pre-install check. Your installed binary was not replaced.",
        "Check platform compatibility and retry later. The existing executable is intact."
    ),
    (
        "worker_logs_unavailable",
        "This workload has no readable container logs endpoint.",
        "Use the authenticated Phala console for private or boot logs. Do not enable public logs just for troubleshooting."
    ),
    (
        "miner_disabled",
        "This miner is disabled on the coordinator.",
        "Check everycli status. Contact EveryFrame support if this is unexpected; creating another VM will not enable it."
    ),
    (
        "deployment_app_mismatch",
        "The coordinator's workload does not match this local deployment.",
        "Compare the local and coordinator appId in everycli status. Use the original management profile; do not deploy another VM or release keys to a different workload."
    ),
    (
        "deployment_compose_mismatch",
        "The coordinator's workload configuration does not match this profile's signed release.",
        "Check everycli status and the selected profile. Resolve the release mismatch before continuing; do not bypass verification or redeploy."
    ),
    (
        "attestation_pending",
        "Automatic workload verification is in progress.",
        "Wait for everycli status to show accepted attestation and online: true, then retry the command. No manual approval request is needed."
    ),
    (
        "attestation_rejected",
        "The workload failed automatic security verification.",
        "Check everycli doctor and contact EveryFrame support with your public miner ID if this persists. Do not release provider keys or bypass verification."
    ),
    (
        "attestation_unavailable",
        "No recognized workload attestation result is available yet.",
        "Check everycli status and everycli doctor. If already deployed, wait for the existing worker to connect; do not create another VM."
    ),
    (
        "attestation_timestamp_invalid",
        "The attestation response has a missing or invalid timestamp.",
        "Check the CLI version and coordinator compatibility. Do not bypass freshness validation."
    ),
    (
        "attestation_clock_skew",
        "The attestation timestamp is ahead of this device's clock.",
        "Enable automatic date and time on this device, then check everycli status. If this persists, report the clock mismatch to EveryFrame support."
    ),
    (
        "attestation_stale",
        "The last accepted attestation is too old to continue safely.",
        "Wait for the worker to refresh its attestation, then check everycli status. If it stays stale, check the existing workload with everycli doctor; do not redeploy."
    ),
    (
        "attestation_session_not_ready",
        "The coordinator does not report a current verified workload session.",
        "Wait for everycli status to show attested: true. If this persists, check everycli doctor; do not bypass verification or create another VM."
    ),
    (
        "worker_offline",
        "The coordinator has not received a recent worker heartbeat.",
        "Check the existing workload with everycli doctor. Wait for everycli status to show online: true before retrying; do not deploy another VM."
    ),
    (
        "post_restart_attestation_required",
        "The restarted worker has not supplied a fresh attestation yet.",
        "Wait for a new accepted attestation in everycli status, then run everycli resume. Do not repeat activation or redeploy."
    ),
    (
        "phala_key_required",
        "No Phala Cloud API key is configured in this local profile.",
        "Save it with everycli set-api-keys --provider phala. Never paste keys into command arguments."
    ),
    (
        "no_local_api_keys_to_check",
        "No local provider API keys are configured for balance queries.",
        "Use everycli set-api-keys on this device, or run balances --publish on the device holding the keys."
    ),
    (
        "profile_not_initialized",
        "No local miner profile was found for this OS user and network.",
        "Run everycli init --wallet <wallet> --hotkey default, or select the original --state-dir and OS user. Do not deploy again to restore status access."
    ),
    (
        "profile_credentials_missing",
        "The miner profile exists but its local credentials file is missing.",
        "Preserve deployment records and reconnect with your hotkey using everycli init, or restore the original management profile."
    ),
    (
        "hosting_limit_exceeded",
        "No matching permitted hosting offer was found within the hourly compute ceiling.",
        "Review the available hosting offer and your spending limit. This is not a provider-credit balance error."
    ),
    (
        "invalid_coordinator_response",
        "The coordinator response failed identity, freshness or protocol validation.",
        "Check your system clock and CLI version. Do not bypass signature or nonce checks."
    ),
    (
        "invalid_signature",
        "A signed response could not be verified.",
        "Check the CLI version and trusted configuration. Do not disable signature verification."
    ),
    (
        "invalid_fields",
        "The response did not match the expected signed protocol fields.",
        "Check the CLI version and coordinator compatibility."
    ),
    (
        "invalid_base64",
        "A signed protocol field used invalid encoding.",
        "Check the CLI version and coordinator compatibility; do not bypass validation."
    ),
    (
        "http_400",
        "HTTP 400: the service rejected the request as invalid.",
        "Check the CLI version and service configuration."
    ),
    (
        "http_401",
        "HTTP 401: authentication was rejected.",
        "Check the API key for the selected account; it may be invalid, expired or revoked."
    ),
    (
        "http_402",
        "HTTP 402: the service requires payment or billing action.",
        "Check the provider billing dashboard. This response alone does not establish a zero balance."
    ),
    (
        "http_403",
        "HTTP 403: access was forbidden.",
        "Check API-key permissions, account access and provider access restrictions."
    ),
    (
        "http_404",
        "HTTP 404: the requested resource or endpoint was not found.",
        "Check the selected account, resource and CLI version."
    ),
    (
        "http_405",
        "HTTP 405: the service does not allow this request method.",
        "Check the CLI version and provider API compatibility."
    ),
    (
        "http_408",
        "HTTP 408: the service timed out waiting for the request.",
        "Check connectivity. Reconcile any cloud operation before retrying."
    ),
    (
        "http_409",
        "HTTP 409: the request conflicts with the current resource state.",
        "Check status and reconcile before repeating an operation."
    ),
    (
        "http_422",
        "HTTP 422: the service rejected the request parameters.",
        "Check the CLI version and deployment configuration; do not increase the spending limit blindly."
    ),
    (
        "http_429",
        "HTTP 429: the service rate-limited the request.",
        "Wait before making another read. Reconcile any cloud operation before retrying."
    ),
    (
        "http_500",
        "HTTP 500: the service encountered an internal error.",
        "Check provider service health. Reconcile any cloud operation before retrying."
    ),
    (
        "http_502",
        "HTTP 502: the service gateway received an invalid upstream response.",
        "Check provider service health. Reconcile any cloud operation before retrying."
    ),
    (
        "http_503",
        "HTTP 503: the service is unavailable.",
        "Check provider service health. Reconcile any cloud operation before retrying."
    ),
    (
        "http_504",
        "HTTP 504: the service gateway timed out.",
        "Check provider service health. Reconcile any cloud operation before retrying."
    ),
    (
        "http_client_error",
        "The service rejected the request with another HTTP 4xx status.",
        "Check account permissions and API compatibility."
    ),
    (
        "http_server_error",
        "The service failed with another HTTP 5xx status.",
        "Check provider service health. Reconcile any cloud operation before retrying."
    ),
    (
        "http_unexpected_status",
        "The service returned an unexpected HTTP status or a disallowed redirect.",
        "Check endpoint availability and CLI compatibility; redirects are not followed with API credentials."
    ),
    (
        "request_timeout",
        "The request timed out; no successful response was received.",
        "Check network connectivity. Reconcile any cloud operation before retrying."
    ),
    (
        "connection_failed",
        "A connection could not be established (network or TLS failure).",
        "Check DNS, firewall, VPN and TLS connectivity. The CLI connects directly and does not use proxy environment variables."
    ),
    (
        "dns_failed",
        "The service hostname could not be resolved.",
        "Check the device's DNS and network connection."
    ),
    (
        "response_read_failed",
        "The response could not be read completely.",
        "Check connectivity. Reconcile any cloud operation before retrying."
    ),
    (
        "remote_request_failed",
        "The request failed without a more specific diagnostic.",
        "Check credentials and connectivity; reconcile any cloud operation before retrying."
    ),
    (
        "transport_failed",
        "The secure HTTP client could not be initialized.",
        "Check the local TLS/network environment and CLI installation."
    ),
    (
        "invalid_header",
        "A saved credential could not be used in an HTTP header.",
        "Save the API key again using everycli set-api-keys; do not include a header prefix or line breaks."
    ),
    (
        "private_destination",
        "The service hostname resolved to a disallowed or private address.",
        "Check DNS or VPN configuration. Do not disable destination safety checks."
    ),
    (
        "response_too_large",
        "The response exceeded the safe size limit.",
        "Check the provider API and CLI version; do not disable response limits."
    ),
    (
        "invalid_remote_json",
        "The service returned invalid JSON.",
        "Check provider service health and CLI compatibility."
    ),
    (
        "invalid_cloud_response",
        "Phala returned a response the CLI could not interpret.",
        "Check the CLI version and Phala API compatibility."
    ),
    (
        "invalid_balance",
        "The provider response did not contain a valid numeric USD balance.",
        "Check billing permissions and provider API compatibility. Missing data is not a zero balance."
    ),
    (
        "non_usd_balance",
        "The provider reported a balance in a currency other than USD.",
        "Check the billing dashboard; the CLI does not guess exchange rates."
    ),
    (
        "unsupported_balance",
        "This provider has no supported USD balance query in the CLI.",
        "Use the provider billing dashboard."
    ),
    (
        "balance_snapshot_reason_unavailable",
        "The saved balance snapshot does not include a failure reason.",
        "Refresh with everycli balances --publish on the device holding the API keys."
    ),
    (
        "request_failed",
        "The request failed; no further safe diagnostic is available.",
        "Check the CLI version, account access and connectivity. Do not share credentials or raw responses."
    ),
];

pub fn message(code: &str) -> Option<String> {
    REASONS.iter().find_map(|(plain, scoped, message, _)| {
        if code == *plain {
            Some((*message).into())
        } else if code == *scoped {
            Some(format!("Phala: {message}"))
        } else {
            None
        }
    })
}

/// Every fixed diagnostic, for the UI's troubleshooting page.
pub fn catalog() -> Vec<Value> {
    REASONS
        .iter()
        .filter(|(plain, ..)| *plain != "request_failed")
        .map(|(plain, _, message, next)| json!({"code":plain,"message":message,"next":next}))
        .collect()
}

pub fn safe_code(code: &str) -> &'static str {
    for (plain, scoped, _, _) in REASONS {
        if code == *plain {
            return plain;
        }
        if code == *scoped {
            return scoped;
        }
    }
    "request_failed"
}

pub fn value(code: &str) -> Value {
    let code = safe_code(code);
    let next = REASONS
        .iter()
        .find(|(plain, scoped, _, _)| code == *plain || code == *scoped)
        .unwrap()
        .3;
    json!({"code":code,"message":message(code).unwrap(),"next":next})
}

pub fn phala_error(error: Error) -> Error {
    REASONS
        .iter()
        .find(|(plain, _, _, _)| error.0 == *plain)
        .map(|(_, scoped, _, _)| Error(scoped))
        .unwrap_or(error)
}

pub fn http_error(status: u16) -> Error {
    Error(match status {
        400 => "http_400",
        401 => "http_401",
        402 => "http_402",
        403 => "http_403",
        404 => "http_404",
        405 => "http_405",
        408 => "http_408",
        409 => "http_409",
        413 => "http_413",
        422 => "http_422",
        429 => "http_429",
        500 => "http_500",
        502 => "http_502",
        503 => "http_503",
        504 => "http_504",
        _ if (400..=499).contains(&status) => "http_client_error",
        _ if (500..=599).contains(&status) => "http_server_error",
        _ => "http_unexpected_status",
    })
}

pub fn transport_error(error: &reqwest::Error) -> Error {
    Error(if error.is_timeout() {
        "request_timeout"
    } else if error.is_connect() {
        "connection_failed"
    } else {
        "remote_request_failed"
    })
}
