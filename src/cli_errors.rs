//! Contextual CLI diagnostics. Only fixed codes and explicit local paths/options
//! are displayed. Remote bodies, credential values and arbitrary argv are not.
use crate::{Error, ErrorContext, cli::Args};
use serde_json::{Value, json};

pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(500).collect()
}

pub fn registration_command(args: Option<&Args>) -> String {
    let network = args.and_then(|a| a.get("--network")).unwrap_or("mainnet");
    let (netuid, chain) = if network == "testnet" {
        (566, "test")
    } else {
        (117, "finney")
    };
    let name = |key, fallback| {
        args.and_then(|a| a.get(key))
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= 80
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .unwrap_or(fallback)
    };
    format!(
        "btcli subnet register --netuid {netuid} --subtensor.network {chain} --wallet.name {} --wallet.hotkey {}",
        name("--wallet", "<wallet>"),
        name("--hotkey", "<hotkey>")
    )
}

pub fn diagnostic(error: &Error, args: Option<&Args>) -> Value {
    if ["profile_not_initialized", "profile_credentials_missing"].contains(&error.0) {
        return crate::miner::profile_diagnostic(error);
    }
    let command = args
        .map(|a| crate::cli_help::public_name(&a.command))
        .unwrap_or("command");
    let help = format!("Run everycli help {command} for options and an example.");
    let (message, next) = match error.0 {
        "missing_option" | "missing_option_value" => {
            let option = match &error.1 { Some(ErrorContext::Option(o)) => o.as_str(), _ => "the required option" };
            (if error.0 == "missing_option" { format!("{command} needs {option}.") }
             else { format!("{option} needs a value.") }, help)
        }
        "unknown_command" => {
            let word = match &error.1 { Some(ErrorContext::Command(c)) => c.as_str(), _ => "" };
            let safe = !word.is_empty() && word.len() <= 24 && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
            let message = if safe { format!("Unknown command '{word}'.") } else { "Unknown command (unrecognized input omitted).".into() };
            let next = crate::cli_help::suggestion(word).map(|s| format!("Did you mean everycli {s}? Run everycli help {s}."))
                .unwrap_or_else(|| "Run everycli help to see available commands.".into());
            (message, next)
        }
        "invalid_discount" => ("--discount-pct must be between 0 and 99.90, with at most two decimal places.".into(), "Example: everycli offer --model MODEL --discount-pct 10".into()),
        "compute_limit_required" => ("--max-hourly-usd must be a positive finite number.".into(), "Example: everycli deploy --max-hourly-usd 0.05. Storage costs extra; this is not a total budget.".into()),
        "confirmation_required" => (format!("{command} needs confirmation, but no interactive terminal is available."), "Review the operation, then add --yes to approve it non-interactively (also required with --json).".into()),
        "setup_requires_terminal" => ("setup needs an interactive terminal and does not accept --yes or --json.".into(), "For automation, run register-hotkey, set-api-keys --provider NAME --stdin, init, then deploy --max-hourly-usd N --yes separately.".into()),
        "setup_existing_deployment" => ("An existing or uncertain deployment was found. Setup will not create another VM.".into(), "Use the original management profile. Run everycli status; use everycli reconcile for an uncertain operation.".into()),
        "setup_status_unavailable" => ("Setup could not verify whether this hotkey already has a deployed worker.".into(), "No new deployment was attempted. Check everycli status and resolve the coordinator error before continuing.".into()),
        "setup_deployment_incomplete" => ("Setup did not confirm a completed deployment.".into(), "Preserve the profile and run everycli status and everycli reconcile before retrying.".into()),
        "hotkey_not_registered" => {
            let network = args.and_then(|a| a.get("--network")).unwrap_or("mainnet");
            let subnet = if network == "testnet" { 566 } else { 117 };
            (format!("This hotkey is not registered on {network} SN{subnet}. No VM was created."),
             format!("Register with the wallet that owns this hotkey (registration may cost TAO):\n{}\nThen rerun everycli register-hotkey and everycli init. Everycli has not submitted a chain transaction.", registration_command(args)))
        }
        "file_not_found" => ("File not found.".into(), "Check the file path and the current OS user. Do not share the file contents.".into()),
        "file_permission_denied" => ("Cannot read this file: permission denied.".into(), "Use the file owner; do not make private keys world-readable.".into()),
        "insecure_file" => ("This private file has unsafe ownership or permissions.".into(), "Use the file owner and owner-only permissions (0600).".into()),
        "invalid_arguments" | "invalid_option" | "unexpected_argument" => (format!("Invalid arguments for {command}; an option or extra argument is not supported."), help),
        "local_hotkey_required_use_wallet_or_hotkey_file" | "hotkey_address_or_wallet_required" => ("Select a hotkey first.".into(), "Use --wallet NAME --hotkey NAME, or --hotkey-file FILE. For a public membership check only, register-hotkey also accepts --hotkey-ss58 ADDRESS.".into()),
        "enrollment_failed_no_vm_created" => ("Enrollment failed; the service did not provide a recognized reason. No VM was created.".into(), "Check everycli register-hotkey, connectivity and the CLI version. This error alone does not mean the hotkey is unregistered.".into()),
        "public_release_not_available" => ("The requested public CLI release is not available.".into(), "Check the official GitHub releases and try again after publication.".into()),
        "unsafe_install_permissions" | "update_locked_or_directory_not_writable" => ("The CLI install directory is not safely writable or another update is running.".into(), "Check directory ownership and the .everycli-update.lock directory. No installed binary was replaced.".into()),
        _ if crate::diagnostics::message(error.0).is_some() => {
            let d = crate::diagnostics::value(error.0);
            let next = if ["init", "register-hotkey"].contains(&command) && ["request_timeout", "connection_failed", "dns_failed", "remote_request_failed"].contains(&error.0) {
                "Check network connectivity and retry this setup step. No VM was created.".into()
            } else { d["next"].as_str().unwrap().into() };
            (d["message"].as_str().unwrap().into(), next)
        }
        _ => (format!("{}.", error.0.replace('_', " ")), help),
    };
    let mut out = json!({"ok":false,"error":error.0,"message":message,"next":next});
    if let Some(ErrorContext::File(path)) = &error.1 {
        out["file"] = json!(clean(&path.to_string_lossy()));
    }
    if error.0 == "hotkey_not_registered" {
        out["registrationCommand"] = json!(registration_command(args));
    }
    out
}
