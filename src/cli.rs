use crate::{
    Error, Result, invitation,
    miner::{Miner, read_secrets},
    need,
    network::PublicHttp,
    onboarding,
    state::{State, read_json},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
    path::Path,
};
pub const HELP: &str = "everycli · EveryFrame miner tools

Usage: everycli COMMAND [options]

Setup: setup (guided), register-hotkey, set-api-keys, remove-api-key, init
Manage: doctor, status, providers, balances, offers, offer, earnings, deploy, activate,
        apply-api-keys, resume, worker-update, stop, start, reconcile

Options:
  --network mainnet|testnet (default mainnet SN117; testnet SN566)
  --state-dir DIR
  --wallet NAME --hotkey NAME (register-hotkey/init; default hotkey: default)
  --hotkey-file FILE (alternative to wallet/name)
  --hotkey-ss58 ADDRESS (register-hotkey; public address only)
  --provider NAME (set-api-keys/remove-api-key; includes vivix, phala, fal-billing,
                   openrouter-billing; billing-only keys stay on this device)
  --publish (balances; sync amounts only for hotkey-only remote viewing)
  --stdin (set-api-keys --provider NAME; read one key from a pipe)
  --invitation FILE (init; optional legacy deployment import)
  --secrets-file FILE (init; optional legacy credential import)
  --release FILE (worker-update)
  --max-hourly-usd N (deploy/start; storage extra)
  --model ID --discount-pct PCT | --withdraw (offer)
  --drain-only (stop)
  --yes --json --help --version

set-api-keys prompts privately; Enter keeps saved keys or skips missing keys. Local only.
apply-api-keys explicitly drains/restarts a reviewed worker; fresh admission required.
register-hotkey verifies finalized subnet membership; it does not register on-chain.
No Node.js, Python or GPU. Hotkey signs locally; no chain transaction or coldkey access.
Hosting/provider charges are real. No guaranteed earnings or automatic total spending cap.
";
#[derive(Clone)]
pub struct Args {
    pub command: String,
    pub options: HashMap<String, String>,
    pub flags: Vec<String>,
}
pub const UPDATE_HELP: &str = "\nCLI upgrades (no login required):\n  everycli update          Install the latest stable CLI\n  everycli update --check  Check without installing\nWorker upgrades: everycli worker-update --release FILE\nCLI upgrades never update the deployed worker.\n";
const VALUE_OPTIONS: &[&str] = &[
    "--network",
    "--state-dir",
    "--invitation",
    "--secrets-file",
    "--hotkey-file",
    "--wallet",
    "--hotkey",
    "--hotkey-ss58",
    "--provider",
    "--release",
    "--max-hourly-usd",
    "--model",
    "--discount-pct",
];

// Recover only a recognized command for parse-error help. Never echo positional
// values, unknown-option values or credentials as a command name.
pub fn diagnostic_context(raw: &[String]) -> Option<Args> {
    let mut words = raw.iter();
    while let Some(word) = words.next() {
        let key = word.split('=').next().unwrap_or("");
        if VALUE_OPTIONS.contains(&key) {
            if !word.contains('=') {
                words.next();
            }
            continue;
        }
        if word.starts_with('-') || ["miner", "help"].contains(&word.as_str()) {
            continue;
        }
        if !crate::cli_help::COMMANDS.contains(&word.as_str()) {
            return None;
        }
        let command = match word.as_str() {
            "update" => "self-update",
            "worker-update" => "update",
            c => c,
        };
        return Some(Args {
            command: command.into(),
            options: HashMap::new(),
            flags: vec![],
        });
    }
    None
}
impl Args {
    pub fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|v| v == k)
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.options.get(k).map(String::as_str)
    }
    pub fn required(&self, k: &str) -> Result<&str> {
        self.get(k).ok_or_else(|| Error("missing_option").option(k))
    }
}
pub fn parse(args: &[String]) -> Result<Args> {
    let mut out = Args {
        command: String::new(),
        options: HashMap::new(),
        flags: vec![],
    };
    let mut positionals = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if [
            "--help",
            "-h",
            "--version",
            "--json",
            "--yes",
            "--drain-only",
            "--withdraw",
            "--check",
            "--stdin",
            "--publish",
            "--continue",
        ]
        .contains(&a)
        {
            out.flags
                .push(if a == "-h" { "--help".into() } else { a.into() })
        } else if a.starts_with("--") {
            let (key, inline) = a
                .split_once('=')
                .map(|(k, v)| (k, Some(v)))
                .unwrap_or((a, None));
            need(VALUE_OPTIONS.contains(&key), "invalid_option").map_err(|e| e.option(key))?;
            let value = if let Some(v) = inline {
                v
            } else {
                i += 1;
                args.get(i)
                    .map(String::as_str)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| Error("missing_option_value").option(key))?
            };
            need(!value.is_empty(), "missing_option_value").map_err(|e| e.option(key))?;
            out.options.insert(key.into(), value.into());
        } else {
            positionals.push(a)
        }
        i += 1
    }
    if positionals.first() == Some(&"help") {
        positionals.remove(0);
        out.flags.push("--help".into());
    }
    if out.flag("--version") || positionals.is_empty() || positionals == ["miner"] {
        return Ok(out);
    }
    if positionals == ["update"] {
        out.command = "self-update".into();
        if out.flag("--help") {
            return Ok(out);
        }
        for option in out.options.keys().chain(out.flags.iter()) {
            need(
                ["--check", "--json", "--yes"].contains(&option.as_str()),
                "invalid_option",
            )
            .map_err(|e| e.option(option))?;
        }
        out.command = "self-update".into();
        return Ok(out);
    }
    // Retain the old namespace only as a compatibility alias. Bare `update`
    // always upgrades the CLI; signed worker updates are explicitly named.
    let command = match positionals.as_slice() {
        ["worker-update"] => "update",
        [command] => *command,
        ["miner", command] => *command,
        _ => return Err(Error("unexpected_argument")),
    };
    need(
        [
            "init",
            "setup",
            "register-hotkey",
            "set-api-keys",
            "remove-api-key",
            "apply-api-keys",
            "doctor",
            "status",
            "providers",
            "balances",
            "offers",
            "offer",
            "earnings",
            "deploy",
            "activate",
            "resume",
            "update",
            "stop",
            "start",
            "reconcile",
        ]
        .contains(&command),
        "unknown_command",
    )
    .map_err(|e| e.command(command))?;
    out.command = command.into();
    if out.flag("--help") {
        return Ok(out);
    }
    let allowed = match out.command.as_str() {
        "setup" => vec![
            "--continue",
            "--wallet",
            "--hotkey",
            "--hotkey-file",
            "--provider",
            "--max-hourly-usd",
        ],
        "init" => vec![
            "--invitation",
            "--secrets-file",
            "--hotkey-file",
            "--wallet",
            "--hotkey",
        ],
        "deploy" | "start" => vec!["--max-hourly-usd"],
        "register-hotkey" => vec!["--wallet", "--hotkey", "--hotkey-file", "--hotkey-ss58"],
        "set-api-keys" => vec!["--provider", "--stdin"],
        "remove-api-key" => vec!["--provider"],
        "balances" => vec!["--publish"],
        "offer" => vec!["--model", "--discount-pct", "--withdraw"],
        "update" => vec!["--release"],
        "stop" => vec!["--drain-only"],
        _ => vec![],
    };
    for option in out
        .options
        .keys()
        .map(String::as_str)
        .chain(out.flags.iter().map(String::as_str))
    {
        need(
            ["--network", "--state-dir", "--yes", "--json"].contains(&option)
                || allowed.contains(&option),
            "invalid_option",
        )
        .map_err(|e| e.option(option))?
    }
    need(
        !(out.flag("--withdraw") && out.get("--discount-pct").is_some()),
        "conflicting_offer_options",
    )?;
    Ok(out)
}
fn question(label: &str, json_mode: bool) -> Result<String> {
    need(io::stdin().is_terminal() && !json_mode, "input_required")?;
    eprint!("{label}");
    io::stderr().flush().map_err(|_| Error("input_required"))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|_| Error("interrupted"))?;
    Ok(line.trim().into())
}
fn local_hotkey(args: &Args) -> Result<Option<std::path::PathBuf>> {
    need(
        !(args.get("--hotkey-file").is_some()
            && (args.get("--wallet").is_some() || args.get("--hotkey").is_some())),
        "choose_wallet_or_hotkey_file",
    )?;
    if let Some(path) = args.get("--hotkey-file") {
        return Ok(Some(Path::new(path).to_path_buf()));
    }
    if args.get("--wallet").is_some() || args.get("--hotkey").is_some() {
        return Ok(Some(onboarding::wallet_path(
            args.required("--wallet")?,
            args.get("--hotkey").unwrap_or("default"),
        )?));
    }
    Ok(None)
}
fn set_keys(args: &Args, state: &State) -> Result<Value> {
    let selected = if let Some(provider) = args.get("--provider") {
        onboarding::credential_key(provider)?;
        vec![provider]
    } else {
        need(!args.flag("--stdin"), "stdin_requires_provider")?;
        std::iter::once("phala")
            .chain(invitation::PROVIDERS.iter().map(|(p, _)| *p))
            .collect()
    };
    if !args.flag("--stdin") {
        need(
            io::stdin().is_terminal() && !args.flag("--json"),
            "hidden_input_required_or_use_stdin_with_provider",
        )?;
    }
    state.prepare()?;
    let saved = state.read("credentials", true)?;
    need(
        saved.is_null() || saved.is_object(),
        "invalid_credentials_file",
    )?;
    let mut updates = json!({});
    for provider in selected {
        let key = onboarding::credential_key(provider)?;
        let value = if args.flag("--stdin") {
            use std::io::Read;
            need(!io::stdin().is_terminal(), "stdin_requires_pipe")?;
            let mut value = String::new();
            io::stdin()
                .take(8003)
                .read_to_string(&mut value)
                .map_err(|_| Error("invalid_credential"))?;
            need(value.len() <= 8002, "invalid_credential")?;
            let value = value.trim_end_matches(['\n', '\r']).to_string();
            need(!value.trim().is_empty(), "empty_credential")?;
            value
        } else {
            rpassword::prompt_password(key_prompt(provider, invitation::configured(&saved[key])))
                .map_err(|_| Error("interrupted"))?
        };
        let value = zeroize::Zeroizing::new(value);
        if !value.is_empty() {
            need(!value.trim().is_empty(), "empty_credential")?;
            updates[key] = json!(*value);
        }
    }
    onboarding::save_keys(state, &updates, None)
}
pub fn key_prompt(provider: &str, saved: bool) -> String {
    format!(
        "{provider} API key (hidden; {}): ",
        if saved {
            "Saved — Enter keeps existing"
        } else {
            "Not saved — Enter skips"
        }
    )
}
pub fn validate_args(args: &Args) -> Result<()> {
    if args.flag("--help") || args.flag("--version") {
        return Ok(());
    }
    if let Some(network) = args.get("--network") {
        need(["mainnet", "testnet"].contains(&network), "invalid_network")?;
    }
    if let Some(discount) = args.get("--discount-pct") {
        invitation::discount(discount)?;
    }
    if let Some(limit) = args.get("--max-hourly-usd") {
        let limit = limit
            .parse::<f64>()
            .map_err(|_| Error("compute_limit_required"))?;
        need(limit.is_finite() && limit > 0., "compute_limit_required")?;
    }
    match args.command.as_str() {
        "deploy" | "start" => {
            args.required("--max-hourly-usd")?;
        }
        "offer" => {
            args.required("--model")?;
            if !args.flag("--withdraw") {
                args.required("--discount-pct")?;
            }
        }
        "update" => {
            args.required("--release")?;
        }
        "remove-api-key" => {
            onboarding::credential_key(args.required("--provider")?)?;
        }
        "setup" => {
            need(
                !args.flag("--yes")
                    && (args.flag("--continue")
                        || (!args.flag("--json") && io::stdin().is_terminal())),
                "setup_requires_terminal",
            )?;
            if args.flag("--continue") {
                need(
                    args.options
                        .keys()
                        .all(|k| ["--network", "--state-dir"].contains(&k.as_str())),
                    "setup_continue_options",
                )?;
            }
        }
        _ => {}
    }
    if let Some(provider) = args.get("--provider") {
        onboarding::credential_key(provider)?;
    }
    if ["init", "register-hotkey", "setup"].contains(&args.command.as_str()) {
        local_hotkey(args)?;
    }
    Ok(())
}

fn setup(args: &Args, miner: &Miner<'_>) -> Result<Value> {
    setup_with(
        args,
        &miner.state,
        &|label| question(label, false),
        &mut run,
    )
}

// Injectable prompts/steps allow the complete wizard to be tested without a
// terminal, real keys, blockchain calls or a paid VM.
pub fn setup_with(
    args: &Args,
    state: &State,
    ask: &dyn Fn(&str) -> Result<String>,
    execute: &mut dyn FnMut(&Args) -> Result<Value>,
) -> Result<Value> {
    state.prepare()?;
    let existing = state.read("deployment", true)?;
    if args.flag("--continue") {
        need(existing.is_object(), "setup_no_deployment")?;
        need(
            !existing["phase"]
                .as_str()
                .unwrap_or("")
                .ends_with("_intent"),
            "setup_existing_deployment",
        )?;
        return Ok(setup_checklist(
            json!({"phase":existing["phase"],"appId":existing["appId"]}),
        ));
    }
    // Any journal (including an incomplete/uncertain one) must be reconciled,
    // never overwritten or treated as a request to provision a second VM.
    need(existing.is_null(), "setup_existing_deployment")?;
    let mut base = args.clone();
    let saved = state.read("registration", true)?;
    if local_hotkey(&base)?.is_none() {
        if let Some(path) = saved["hotkeyFile"].as_str() {
            base.options.insert("--hotkey-file".into(), path.into());
        } else {
            let wallet = ask("Wallet name: ")?;
            let key = ask("Hotkey name [default]: ")?;
            base.options.insert("--wallet".into(), wallet);
            base.options.insert(
                "--hotkey".into(),
                if key.is_empty() {
                    "default".into()
                } else {
                    key
                },
            );
            local_hotkey(&base)?;
        }
    }
    let step = |command: &str, extra: &[&str]| {
        let mut a = base.clone();
        a.command = command.into();
        a.options.retain(|k, _| {
            ["--network", "--state-dir"].contains(&k.as_str()) || extra.contains(&k.as_str())
        });
        a
    };
    eprintln!("Step 1/11 — Check hotkey registration (read-only)");
    let registration = execute(&step(
        "register-hotkey",
        &["--wallet", "--hotkey", "--hotkey-file"],
    ))?;
    if registration["registered"] != true {
        return Ok(crate::cli_errors::diagnostic(
            &Error("hotkey_not_registered"),
            Some(&base),
        ));
    }
    eprintln!(
        "Step 2/11 — Save API keys locally. Phala: Settings → API Tokens, not Private AI. Enter keeps saved keys or skips missing keys."
    );
    if let Some(provider) = args.get("--provider") {
        for p in ["phala", provider] {
            let mut a = step("set-api-keys", &[]);
            a.options.insert("--provider".into(), p.into());
            execute(&a)?;
            if provider == "phala" {
                break;
            }
        }
    } else {
        execute(&step("set-api-keys", &[]))?;
    }
    eprintln!("Step 3/11 — Initialize the signed miner profile (no VM created)");
    execute(&step("init", &["--wallet", "--hotkey", "--hotkey-file"]))?;
    let status = execute(&step("status", &[]))?;
    need(
        status["coordinator"].is_object() && status["coordinatorError"].is_null(),
        "setup_status_unavailable",
    )?;
    let remote = &status["coordinator"];
    need(
        remote["appId"].is_null() || remote["appId"] == "0000000000000000000000000000000000000000",
        "setup_existing_deployment",
    )?;
    eprintln!(
        "Step 4/11 — Review paid deployment. Storage is extra; this is not a total budget. You must confirm the live quote. Activation and model offers follow in steps 5–11."
    );
    let mut deploy = step("deploy", &["--max-hourly-usd"]);
    if deploy.get("--max-hourly-usd").is_none() {
        deploy.options.insert(
            "--max-hourly-usd".into(),
            ask("Maximum compute USD/hour: ")?,
        );
    }
    validate_args(&deploy)?;
    let mut out = execute(&deploy)?;
    need(
        out["ok"] != false && out["appId"].is_string(),
        "setup_deployment_incomplete",
    )?;
    out["deploymentComplete"] = json!(true);
    out["setupComplete"] = json!(false);
    Ok(setup_checklist(out))
}

// A replayable checklist, not an automatic lifecycle runner. The local journal
// cannot establish current admission or authorize paid work.
pub fn setup_checklist(mut out: Value) -> Value {
    out["checklistOnly"] = json!(true);
    out["nextStepNumber"] = json!(5);
    out["readinessVerified"] = json!(false);
    out["next"] = json!(
        "Use everycli setup --continue in the same management profile to show this checklist again. No activation, resume or generation was performed by this checklist."
    );
    out["nextSteps"] = json!([
        "Run everycli status and everycli doctor. Verify the current workload and accepted attestation; a local phase alone does not prove readiness.",
        "If keys have not been activated, run everycli activate after accepted attestation. Review and confirm the restart. Skip this step if already activated.",
        "After activation or any restart, check everycli status and wait for fresh accepted attestation and a current heartbeat. Do not reuse the earlier session.",
        "Run everycli resume only when ready to accept provider-billed work. Review the confirmation; hosting charges continue while waiting.",
        "Run everycli offers. Choose an available model and review its pricing and any existing offer.",
        "To set a bid, run everycli offer --model MODEL --discount-pct PCT with your chosen model and discount. Review and confirm; no guaranteed traffic or profit.",
        "Run everycli doctor again. Resolve any failed checks before expecting work. For stopped or uncertain deployments, use status/reconcile and the original profile; never deploy a duplicate."
    ]);
    out
}

pub fn run(args: &Args) -> Result<Value> {
    validate_args(args)?;
    if args.command == "self-update" {
        return crate::update::run(args.flag("--check"), args.flag("--json"));
    }
    for key in ["PHALA_CLOUD_API_PREFIX", "DEBUG", "SSLKEYLOGFILE"] {
        need(
            std::env::var(key).unwrap_or_default().is_empty(),
            "unsafe_environment",
        )?
    }
    let network = args.get("--network").unwrap_or("mainnet");
    let miner = Miner {
        state: State::new(network, args.get("--state-dir"))?,
        trust: invitation::trust(network)?,
        http: &PublicHttp,
    };
    if args.command == "setup" {
        return setup(args, &miner);
    }
    // Fail locally before any transport or paid-operation preparation. Providers
    // and key management intentionally remain usable before initialization.
    if ![
        "register-hotkey",
        "set-api-keys",
        "remove-api-key",
        "init",
        "providers",
        "status",
        "doctor",
    ]
    .contains(&args.command.as_str())
    {
        miner.state.prepare()?;
        if miner.state.read("config", true)?.is_null() {
            return Ok(crate::miner::profile_diagnostic(
                &Error("profile_not_initialized")
                    .at_file(&miner.state.directory.join("config.json")),
            ));
        }
        if miner.state.read("credentials", true)?.is_null() {
            return Ok(crate::miner::profile_diagnostic(
                &Error("profile_credentials_missing")
                    .at_file(&miner.state.directory.join("credentials.json")),
            ));
        }
    }
    let confirm = |text: &str| {
        need(
            args.flag("--yes") || (io::stdin().is_terminal() && !args.flag("--json")),
            "confirmation_required",
        )?;
        // Keep hosting quotes and lifecycle notices visible. Only local key
        // removal suppresses its already-approved question with --yes.
        if !(args.flag("--yes") && args.command == "remove-api-key") {
            eprintln!("\n{text}");
        }
        if args.flag("--yes") {
            Ok(())
        } else {
            need(
                question("Continue? Type yes: ", args.flag("--json"))? == "yes",
                "cancelled",
            )
        }
    };
    match args.command.as_str() {
        "register-hotkey" => {
            let path = local_hotkey(args)?;
            need(
                !(path.is_some() && args.get("--hotkey-ss58").is_some()),
                "choose_address_or_wallet",
            )?;
            let address = if let Some(address) = args.get("--hotkey-ss58") {
                address.to_string()
            } else if let Some(path) = &path {
                onboarding::file_address(path)?
            } else {
                return Err(Error("hotkey_address_or_wallet_required"));
            };
            let mut out = onboarding::check_registration(miner.http, network, &address)?;
            if out["registered"] == true {
                onboarding::save_registration(&miner.state, &out, path.as_deref())?;
            } else {
                out["registrationCommand"] =
                    json!(crate::cli_errors::registration_command(Some(args)));
            }
            Ok(out)
        }
        "set-api-keys" => set_keys(args, &miner.state),
        "remove-api-key" => {
            let provider = args.required("--provider")?;
            let key = onboarding::credential_key(provider)?;
            miner.state.prepare()?;
            let saved = miner.state.read("credentials", true)?;
            need(
                saved.is_null() || saved.is_object(),
                "invalid_credentials_file",
            )?;
            if saved.get(key).is_none() {
                return Ok(
                    json!({"savedLocally":false,"updated":[],"removed":null,"notSaved":key,"workerChanged":false,"next":"Nothing changed. No local key was saved for this provider."}),
                );
            }
            confirm(&format!(
                "Remove the locally saved {provider} key? This does not revoke the key at the provider or remove it from a running worker."
            ))?;
            onboarding::save_keys(&miner.state, &json!({}), Some(provider))
        }
        "apply-api-keys" => miner.apply_api_keys(&confirm),
        "init" => {
            miner.state.prepare()?;
            let registration = miner.state.read("registration", true)?;
            let path = local_hotkey(args)?.or_else(|| {
                registration["hotkeyFile"]
                    .as_str()
                    .map(std::path::PathBuf::from)
            });
            if let Some(path) = &path
                && !registration.is_null()
            {
                need(
                    registration["network"] == network
                        && registration["hotkey"] == onboarding::file_address(path)?,
                    "hotkey_does_not_match_deployment",
                )?;
            }
            let envelope = if let Some(file) = args.get("--invitation") {
                read_json(Path::new(file), false)?
            } else {
                onboarding::enroll_registered(
                    miner.http,
                    &miner.trust,
                    path.as_deref()
                        .ok_or(Error("local_hotkey_required_use_wallet_or_hotkey_file"))?,
                )?
            };
            let deployment = miner.validate(&envelope, false)?;
            let mut credentials = if let Some(file) = args.get("--secrets-file") {
                read_secrets(Path::new(file))?
            } else {
                json!({})
            };
            if !registration.is_null() {
                need(
                    registration["hotkey"] == deployment["hotkey"]
                        && registration["network"] == network,
                    "hotkey_does_not_match_deployment",
                )?;
            }
            if deployment["authMode"] == "hotkey-v1" {
                let path = path.ok_or(Error("local_hotkey_required_use_wallet_or_hotkey_file"))?;
                credentials.as_object_mut().unwrap().remove("MINER_TOKEN");
                credentials["CONSOLE_AUTH"] = crate::hotkey::create(&path, &deployment, "console")?;
                credentials["WORKER_AUTH"] = crate::hotkey::create(&path, &deployment, "worker")?;
            } else {
                need(
                    args.get("--hotkey-file").is_none()
                        && args.get("--wallet").is_none()
                        && args.get("--hotkey").is_none(),
                    "hotkey_deployment_required",
                )?;
            }
            miner.init(&envelope, &credentials)
        }
        "doctor" => miner.doctor(),
        "status" => miner.status(),
        "providers" => miner.providers(),
        "balances" => miner.balances(args.flag("--publish")),
        "offers" => miner.offers(),
        "earnings" => miner.earnings(),
        "offer" => miner.offer(
            args.required("--model")?,
            args.get("--discount-pct"),
            args.flag("--withdraw"),
            &confirm,
        ),
        "deploy" | "start" => {
            let limit = args
                .required("--max-hourly-usd")?
                .parse::<f64>()
                .map_err(|_| Error("compute_limit_required"))?;
            if args.command == "deploy" {
                miner.deploy(limit, &confirm)
            } else {
                miner.start(limit, &confirm)
            }
        }
        "activate" => miner.activate(&confirm),
        "resume" => miner.resume(&confirm),
        "stop" => miner.stop(args.flag("--drain-only"), &confirm),
        "update" => miner.update(
            &read_json(Path::new(args.required("--release")?), false)?,
            &confirm,
        ),
        "reconcile" => miner.reconcile(),
        _ => Err(Error("unknown_command")),
    }
}
pub fn main_entry() -> i32 {
    unsafe { libc::umask(0o077) };
    let raw = std::env::args().skip(1).collect::<Vec<_>>();
    let json_mode = raw.iter().any(|a| a == "--json");
    let parsed = parse(&raw);
    let diagnostic_args = parsed
        .as_ref()
        .ok()
        .cloned()
        .or_else(|| diagnostic_context(&raw));
    let result = parsed.and_then(|a| {
        if a.flag("--version") {
            println!(
                "{}",
                if json_mode {
                    json!({"version":env!("CARGO_PKG_VERSION"),"implementation":"rust"}).to_string()
                } else {
                    format!("everycli {} (Rust)", env!("CARGO_PKG_VERSION"))
                }
            );
            return Ok(0);
        }
        if a.flag("--help") || a.command.is_empty() {
            println!(
                "{}",
                if json_mode {
                    json!({"help":crate::cli_help::render(&a.command)}).to_string()
                } else {
                    crate::cli_help::render(&a.command)
                }
            );
            return Ok(0);
        }
        let out = run(&a)?;
        println!(
            "{}",
            if json_mode {
                out.to_string()
            } else {
                crate::display::render(&a.command, &out)
            }
        );
        Ok(
            if out["ok"] == false
                || (a.command == "register-hotkey" && out["registered"] != true)
                || (a.command == "doctor" && out["ok"] != true)
                || (a.command == "status"
                    && (out["ok"] == false
                        || !out["coordinatorError"].is_null()
                        || !out["cloudError"].is_null()))
                || (a.command == "reconcile" && out["resolved"] == false)
                || (a.command == "balances" && balance_check_failed(&out))
            {
                2
            } else {
                0
            },
        )
    });
    match result {
        Ok(n) => n,
        Err(e) => {
            let d = crate::cli_errors::diagnostic(&e, diagnostic_args.as_ref());
            if json_mode {
                println!("{d}")
            } else {
                eprintln!("{}", crate::display::render("error", &d))
            }
            2
        }
    }
}

pub fn balance_check_failed(out: &Value) -> bool {
    (out["publishRequested"] == true && out["published"] != true)
        || out["balances"]
            .as_array()
            .is_none_or(|rows| rows.is_empty() || rows.iter().any(|r| r["status"] == "unavailable"))
}
