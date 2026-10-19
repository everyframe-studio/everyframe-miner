//! Contextual CLI diagnostics. Only fixed codes and explicit local paths/options
//! are displayed. Remote bodies, credential values and arbitrary argv are not.
use crate::{Error, ErrorContext, cli::Args};
use serde_json::{Value, json};

pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(500).collect()
}
// Flag names are useful context; attached values and key-like/oversized names
// are not. Never include an arbitrary credential-bearing argument in output.
fn safe_option(option: &str) -> bool {
    option.starts_with("--")
        && option.len() <= 32
        && option[2..]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'-')
        && !["secret", "token", "password", "api-key"]
            .iter()
            .any(|s| option.contains(s))
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
        name(
            "--hotkey",
            if args.is_some_and(|a| a.get("--wallet").is_some()) {
                "default"
            } else {
                "<hotkey>"
            }
        )
    )
}

pub fn diagnostic(error: &Error, args: Option<&Args>) -> Value {
    if error.0 == "file_not_found"
        && let Some(ErrorContext::File(path)) = &error.1
        && let Ok(home) = std::env::var("HOME")
        && let Some(out) = wallet_file_diagnostic(
            path,
            &std::path::Path::new(&home).join(".bittensor/wallets"),
        )
    {
        return out;
    }
    if ["profile_not_initialized", "profile_credentials_missing"].contains(&error.0) {
        return crate::miner::profile_diagnostic(error);
    }
    let command = args
        .map(|a| crate::cli_help::public_name(&a.command))
        .filter(|c| crate::cli_help::COMMANDS.contains(c))
        .unwrap_or("");
    let help = if command.is_empty() {
        "Run everycli help for available commands and examples.".into()
    } else {
        format!("Run everycli help {command} for options and an example.")
    };
    let (message, next) = match error.0 {
        "replacement_not_supported" => ("This coordinator does not support safe worker replacement yet. No VM was created.".into(), "Update everycli and retry after the coordinator upgrade. Preserve your deletion journal.".into()),
        "active_jobs" => ("This miner still has active or unresolved jobs. No replacement VM was created.".into(), "Wait for those jobs to settle, then retry. Do not delete the local profile.".into()),
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
        "conflicting_offer_options" => ("Offers cannot combine --discount-pct with --withdraw.".into(), "Choose a discount to set an offer, or --withdraw to remove it. Run everycli help offer for examples.".into()),
        "invalid_offer_batch" => ("Choose 1–128 distinct model IDs for this batch.".into(), "Use everycli offers, then everycli offer-batch --models ID,ID --discount-pct 10.".into()),
        "batch_offers_not_supported" => ("This coordinator does not support batch offers yet.".into(), "No offers were changed. Update the coordinator or use individual offers.".into()),
        "offer_pricing_unavailable" => ("Pricing is temporarily unavailable for a selected model. No discounts were changed.".into(), "The coordinator needs to refresh its pricing. Refresh offers and try again once pricing is available; changing your discount or API keys will not fix this.".into()),
        "provider_model_unavailable" => ("A selected model is no longer available for new offers or work. No discounts were changed.".into(), "Refresh offers and choose a supported model. Existing offers can still be withdrawn; historical jobs are preserved.".into()),
        "offer_revision_conflict" | "offer_pricing_changed" => ("An offer or its pricing changed during review. Nothing in this batch was applied.".into(), "Refresh offers and review the batch again.".into()),
        "invalid_network" | "unknown_network" => ("Invalid --network. Use mainnet or testnet.".into(), "Example: everycli doctor --network mainnet".into()),
        "unknown_provider" => ("Unknown provider.".into(), format!("Use --provider NAME. Valid providers: {}.", crate::onboarding::credential_providers().join(", "))),
        "hidden_input_required_or_use_stdin_with_provider" => ("Key entry needs an interactive terminal.".into(), "Run everycli set-api-keys in a terminal, or use --provider NAME --stdin to read one key from a pipe. Never put the key in command arguments.".into()),
        "stdin_requires_provider" => ("--stdin needs --provider NAME.".into(), "Pipe one key into everycli set-api-keys --provider NAME --stdin.".into()),
        "stdin_requires_pipe" => ("--stdin needs piped input, not a terminal.".into(), "Pipe one key into the command, or omit --stdin to use a hidden terminal prompt.".into()),
        "empty_credential" => ("The key was empty. Nothing was saved.".into(), "Provide a non-empty key with --provider NAME --stdin, or run everycli set-api-keys in a terminal.".into()),
        "credential_too_short" => ("That key looks too short (minimum 10 characters).".into(), "Nothing was saved. Copy the full API key from the provider and try again; do not share it here.".into()),
        "compute_limit_required" => ("--max-hourly-usd must be a positive finite number.".into(), "Example: everycli deploy --max-hourly-usd 0.05. Storage costs extra; this is not a total budget.".into()),
        "confirmation_required" => (format!("{command} needs confirmation, but no interactive terminal is available."), "Review the operation, then add --yes to approve it non-interactively (also required with --json).".into()),
        "setup_requires_terminal" => ("New setup needs an interactive terminal and does not accept --yes or --json. setup --continue also rejects --yes.".into(), "For automation, run register-hotkey, set-api-keys --provider NAME --stdin, init, then deploy --max-hourly-usd N --yes separately. For an existing deployment, setup --continue --json reads only its checklist.".into()),
        "setup_existing_deployment" => ("An existing or uncertain deployment was found. Setup will not create another VM.".into(), "Use the original management profile. Run everycli status; use everycli reconcile for an uncertain operation. After reconciliation, everycli setup --continue shows the remaining checklist without deploying.".into()),
        "setup_no_deployment" => ("There is no local deployment to continue.".into(), "For a new miner, run everycli setup in a terminal. If already deployed elsewhere, use the original management profile; do not create another VM.".into()),
        "setup_continue_options" => ("setup --continue only reads the existing deployment checklist.".into(), "Use the original --network and --state-dir, optionally with --json. Omit wallet, provider and hosting-limit options; no deployment will be created.".into()),
        "setup_status_unavailable" => ("Setup could not verify whether this hotkey already has a deployed worker.".into(), "No new deployment was attempted. Check everycli status and resolve the coordinator error before continuing.".into()),
        "setup_deployment_incomplete" => ("Setup did not confirm a completed deployment.".into(), "Preserve the profile and run everycli status and everycli reconcile before retrying.".into()),
        "hotkey_not_registered" => {
            let network = args.and_then(|a| a.get("--network")).unwrap_or("mainnet");
            let subnet = if network == "testnet" { 566 } else { 117 };
            (format!("This hotkey is not registered on {network} SN{subnet}. No VM was created."),
             format!("Register with the wallet that owns this hotkey (registration may cost TAO):\n{}\nThen rerun {}. No chain transaction was submitted by everycli.", registration_command(args), if command == "setup" { "everycli setup" } else { "everycli register-hotkey and everycli init" }))
        }
        "file_not_found" => ("File not found.".into(), "Check the file path and the current OS user. Do not share the file contents.".into()),
        "file_permission_denied" => ("Cannot read this file: permission denied.".into(), "Use the file owner; do not make private keys world-readable.".into()),
        "insecure_file" => ("This private file has unsafe ownership or permissions.".into(), "Use the file owner and owner-only permissions (0600).".into()),
        "invalid_arguments" | "invalid_option" | "unexpected_argument" => {
            let subject = if command.is_empty() { "this command" } else { command };
            let message = match &error.1 {
                Some(ErrorContext::Option(option)) if safe_option(option) => format!("Unsupported option {option} for {subject}."),
                _ => format!("Invalid arguments for {subject}; an option or extra argument is not supported (unrecognized input omitted)."),
            };
            (message, help)
        }
        "local_hotkey_required_use_wallet_or_hotkey_file" | "hotkey_address_or_wallet_required" => ("Select a hotkey first.".into(), "Use --wallet NAME --hotkey NAME, or --hotkey-file FILE. For a public membership check only, register-hotkey also accepts --hotkey-ss58 ADDRESS.".into()),
        "enrollment_failed_no_vm_created" => ("Enrollment failed; the service did not provide a recognized reason. No VM was created.".into(), "Check everycli register-hotkey, connectivity and the CLI version. This error alone does not mean the hotkey is unregistered.".into()),
        "os_catalog_changed" | "deployment_os_unavailable" => ("The approved OS image is no longer available in Phala's catalog. No VM was created.".into(), "Update everycli, then reconnect to fetch a reviewed replacement deployment. If this persists, the replacement OS is still awaiting server-side approval; do not bypass the image hash check.".into()),
        "deployment_catalog_unavailable" => ("The service could not verify Phala's OS catalog. No replacement was issued.".into(), "Retry reconnect when connectivity returns. Your existing workload binding was not retired.".into()),
        "deleted_workload_reconnect_required" => ("The coordinator still has an old VM binding for this profile.".into(), "Run everycli reconnect. If you deleted the VM, recovery will ask you to confirm retiring its old authorization. No VM is created automatically.".into()),
        "recovery_target_changed" => ("The workload binding changed before recovery completed.".into(), "Run everycli status and reconnect again to inspect the current binding. Do not repeat a cloud deployment blindly.".into()),
        "recovery_active_jobs" => ("This miner still has active or uncertain jobs. Its deployment cannot be retired yet.".into(), "Wait for reconciliation, then retry reconnect. Recovery does not discard paid work.".into()),
        "recovery_worker_still_online" => ("The old worker is still online or has unfinished work. Missing-workload recovery was refused.".into(), "Check the Phala workspace and stop the existing worker normally. A wrong-workspace API key can make a live VM appear missing.".into()),
        "remote_workload_exists_use_original_profile" => ("The bound VM still exists in Phala. It was not retired.".into(), "Use the original management profile, or explicitly delete that VM before replacing it.".into()),
        "public_release_not_available" => ("The requested public CLI release is not available.".into(), "Check the official GitHub releases and try again after publication.".into()),
        "worker_update_conflicting_options" => ("worker-update cannot combine --check with --release.".into(), "Use everycli worker-update --check for automatic discovery, or --release FILE for the advanced manual flow.".into()),
        "worker_update_original_profile_required" => ("This update needs the original VM management profile, including its deployment journal and provider keys.".into(), "Run everycli worker-update on the device/profile used to deploy this VM. Do not redeploy from a fresh profile.".into()),
        "worker_update_uncertain_operation" => ("A cloud operation was recorded but its result is uncertain. It was not repeated.".into(), "Keep this profile and inspect everycli status and the existing VM in Phala. Reconcile the recorded operation before continuing; do not redeploy or delete the journal.".into()),
        "update_compose_mismatch" => ("Phala's prepared configuration does not match the signed worker release. The update was not committed and the hash check was not bypassed.".into(), "Run everycli reconcile to verify the old worker is unchanged. Then retry with a corrected signed release, or use everycli stop. Keep the profile; do not redeploy.".into()),
        "update_provision_state_changed" => ("The existing VM no longer matches the unchanged worker expected before this update.".into(), "Keep this profile and inspect everycli status and the VM in Phala. No preparation, commit or restart was repeated. Do not delete the journal.".into()),
        "existing_cloud_workload" => ("Your Phala account already has a worker for this miner, probably deployed from another computer or an earlier setup.".into(), "Run everycli status to see its app ID. Keep using it from the computer that deployed it, or delete it with everycli delete --app-id ID and deploy again.".into()),
        "deployment_already_recorded" => ("This profile already manages a worker.".into(), "Use everycli status. To replace it, delete it with everycli delete --app-id ID, then deploy again.".into()),
        "cloud_workload_not_found" => ("No worker with that app ID and this miner's name is in your Phala account.".into(), "It may already be deleted. Run everycli status to list current workers.".into()),
        "invalid_worker_label" => ("A worker label must be 1 to 64 characters, without control characters.".into(), "Choose a shorter name, for example studio-worker-1.".into()),
        "credential_not_saved" => ("No key is saved for this provider yet.".into(), "Paste a key and save it first.".into()),
        "invalid_port" => ("--port must be a number from 1024 to 65535.".into(), "Run everycli ui without --port to use 47117 or the next free port.".into()),
        "ui_port_unavailable" => ("No local port was free for the UI.".into(), "Stop the other everycli ui, or choose one with everycli ui --port N.".into()),
        "public_catalog_unavailable" => ("Public model discovery is not available on this coordinator yet.".into(), "No VM or generation was created. An initialized profile can still use everycli offers to view its signed model catalog.".into()),
        "conflicting_auto_update_options" => ("Choose either --enable or --disable, not both.".into(), "Run everycli auto-update to view the current setting.".into()),
        "invalid_auto_update_settings" => ("The automatic CLI update settings file is invalid.".into(), "Run everycli auto-update --enable or --disable to save a valid setting. Miner profiles are separate and unchanged.".into()),
        "worker_update_state_changed" | "invalid_worker_update_journal" => ("Worker state changed outside the saved update, or its journal does not match this profile.".into(), "Inspect everycli status before continuing. Preserve the profile and update journal; do not create another VM.".into()),
        "worker_update_miner_disabled" => ("The coordinator has disabled this miner. The update will not override that setting.".into(), "The update is saved and work remains paused. Resolve the disabled status before rerunning everycli worker-update.".into()),
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
    if let Some(ErrorContext::Option(option)) = &error.1
        && safe_option(option)
    {
        out["option"] = json!(option);
    }
    if error.0 == "hotkey_not_registered" {
        out["registrationCommand"] = json!(registration_command(args));
    }
    out
}

/// Discover directory names only, never wallet/key contents or symlink targets.
/// Other missing files and permission errors keep their original diagnostics.
pub fn wallet_file_diagnostic(path: &std::path::Path, root: &std::path::Path) -> Option<Value> {
    use std::{fs, io::ErrorKind};
    let relative = path.strip_prefix(root).ok()?;
    let parts = relative
        .iter()
        .map(|p| p.to_str())
        .collect::<Option<Vec<_>>>()?;
    let valid_name = |name: &str| {
        !name.is_empty()
            && name.len() <= 80
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if parts.len() != 3 || parts[1] != "hotkeys" || !valid_name(parts[0]) || !valid_name(parts[2]) {
        return None;
    }
    crate::state::no_symlink(root).ok()?;
    match fs::symlink_metadata(root.join(parts[0])) {
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        _ => return None,
    }
    let mut wallets = vec![];
    match fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries.take(1000).flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir())
                    && let Ok(name) = entry.file_name().into_string()
                    && valid_name(&name)
                {
                    wallets.push(name);
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    wallets.sort();
    wallets.truncate(100);
    let available = if wallets.is_empty() {
        "No wallets found in this directory.".to_string()
    } else {
        format!("Available wallets (up to 100): {}.", wallets.join(", "))
    };
    Some(json!({
        "ok":false,"error":"wallet_not_found",
        "message":format!("Wallet ‘{}’ not found.", parts[0]),
        "wallet":parts[0],"availableWallets":wallets,
        "walletDirectory":clean(&root.to_string_lossy()),
        "next":format!("{available}\nWallet directory: {}\nUse --wallet NAME for an available wallet, or --hotkey-file FILE for a wallet stored elsewhere. Check that you are using the correct OS user.", clean(&root.to_string_lossy()))
    }))
}
