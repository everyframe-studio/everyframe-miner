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
pub const HELP: &str = "everycli · Everyframe miner tools

Usage: everycli COMMAND [options]

Setup: register-hotkey, set-api-keys, remove-api-key, init
Manage: doctor, status, providers, balances, offers, offer, earnings, deploy, activate,
        apply-api-keys, resume, worker-update, stop, start, reconcile

Options:
  --network mainnet|testnet (default mainnet SN117; testnet SN566)
  --state-dir DIR
  --wallet NAME --hotkey NAME (register-hotkey/init; default hotkey: default)
  --hotkey-file FILE (alternative to wallet/name)
  --hotkey-ss58 ADDRESS (register-hotkey; public address only)
  --provider NAME (set-api-keys/remove-api-key; includes phala, fal-billing,
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

set-api-keys prompts privately; Enter keeps existing keys. Saved locally only.
apply-api-keys explicitly drains/restarts a reviewed worker; fresh admission required.
register-hotkey verifies finalized subnet membership; it does not register on-chain.
No Node.js, Python or GPU. Hotkey signs locally; no chain transaction or coldkey access.
Hosting/provider charges are real. No guaranteed earnings or automatic total spending cap.
";
pub struct Args {
    pub command: String,
    pub options: HashMap<String, String>,
    pub flags: Vec<String>,
}
const UPDATE_HELP: &str = "\nCLI upgrades (no login required):\n  everycli update          Install the latest stable CLI\n  everycli update --check  Check without installing\nWorker upgrades: everycli worker-update --release FILE\nCLI upgrades never update the deployed worker.\n";
impl Args {
    pub fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|v| v == k)
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.options.get(k).map(String::as_str)
    }
    pub fn required(&self, k: &str) -> Result<&str> {
        self.get(k).ok_or(Error("missing_option"))
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
            need(
                [
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
                ]
                .contains(&key),
                "invalid_arguments",
            )?;
            let value = if let Some(v) = inline {
                v
            } else {
                i += 1;
                args.get(i)
                    .map(String::as_str)
                    .ok_or(Error("invalid_arguments"))?
            };
            out.options.insert(key.into(), value.into());
        } else {
            positionals.push(a)
        }
        i += 1
    }
    if out.flag("--help")
        || out.flag("--version")
        || positionals.is_empty()
        || positionals == ["miner"]
    {
        return Ok(out);
    }
    if positionals == ["update"] {
        need(
            out.options.is_empty()
                && out
                    .flags
                    .iter()
                    .all(|f| ["--check", "--json", "--yes"].contains(&f.as_str())),
            "invalid_option",
        )?;
        out.command = "self-update".into();
        return Ok(out);
    }
    // Retain the old namespace only as a compatibility alias. Bare `update`
    // always upgrades the CLI; signed worker updates are explicitly named.
    let command = match positionals.as_slice() {
        ["worker-update"] => "update",
        [command] => *command,
        ["miner", command] => *command,
        _ => return Err(Error("unknown_command")),
    };
    need(
        [
            "init",
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
    )?;
    out.command = command.into();
    let allowed = match out.command.as_str() {
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
        )?
    }
    need(
        !(out.flag("--withdraw") && out.get("--discount-pct").is_some()),
        "invalid_option",
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
            need(!value.is_empty(), "invalid_credential")?;
            value
        } else {
            need(
                io::stdin().is_terminal() && !args.flag("--json"),
                "hidden_input_required_or_use_stdin_with_provider",
            )?;
            rpassword::prompt_password(format!(
                "{provider} API key (hidden; Enter keeps existing): "
            ))
            .map_err(|_| Error("interrupted"))?
        };
        let value = zeroize::Zeroizing::new(value);
        if !value.is_empty() {
            updates[key] = json!(*value);
        }
    }
    onboarding::save_keys(state, &updates, None)
}
pub fn run(args: &Args) -> Result<Value> {
    if args.command == "self-update" {
        return crate::update::run(args.flag("--check"));
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
    let confirm = |text: &str| {
        eprintln!("\n{text}");
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
            } else if let Some(wallet) = args.get("--wallet") {
                out["registrationCommand"] = json!(format!(
                    "btcli subnet register --netuid {} --subtensor.network {} --wallet.name {} --wallet.hotkey {}",
                    miner.trust["netuid"],
                    if network == "mainnet" {
                        "finney"
                    } else {
                        "test"
                    },
                    wallet,
                    args.get("--hotkey").unwrap_or("default")
                ));
            }
            Ok(out)
        }
        "set-api-keys" => set_keys(args, &miner.state),
        "remove-api-key" => {
            let provider = args.required("--provider")?;
            onboarding::credential_key(provider)?;
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
                onboarding::enroll(
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
                    json!({"help":format!("{HELP}{UPDATE_HELP}")}).to_string()
                } else {
                    format!("{HELP}{UPDATE_HELP}")
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
            if (a.command == "register-hotkey" && out["registered"] != true)
                || (a.command == "doctor" && out["ok"] != true)
                || (a.command == "status"
                    && (out["ok"] == false
                        || !out["coordinatorError"].is_null()
                        || !out["cloudError"].is_null()))
                || (a.command == "reconcile" && out["resolved"] == false)
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
            let message = if raw.first().is_some_and(|a| a == "update") {
                match e.0 {
                    "public_release_not_available" => "No public CLI release is available yet. Publish the first tagged GitHub release, then retry.".to_string(),
                    "unsafe_install_permissions" | "update_locked_or_directory_not_writable" => "The install directory must be writable by its owner and no other update may be running. Check installation permissions and the .everycli-update.lock directory.".to_string(),
                    _ => format!("CLI update failed: {}. Check the official GitHub release and your network connection. Miner profiles and deployed workers are unchanged.", e.0.replace('_', " ")),
                }
            } else {
                format!(
                    "{}. Use everycli --help for commands. For uncertain operations, run everycli status or everycli doctor before retrying. Raw responses and secrets suppressed.",
                    e.0.replace('_', " ")
                )
            };
            if json_mode {
                println!("{}", json!({"error":e.0,"message":message}))
            } else {
                eprintln!("{message}")
            }
            1
        }
    }
}
