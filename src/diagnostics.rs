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
