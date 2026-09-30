use crate::{
    Error, Result, invitation,
    miner::{Miner, read_secrets},
    need,
    network::PublicHttp,
    state::{State, read_json},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
    path::Path,
};
pub const HELP: &str = "everycli · Everyframe miner tools (Rust)\n\nUsage: everycli miner COMMAND [options]\n\nCommands: init, doctor, status, providers, offers, offer, earnings, deploy, activate, resume, update, stop, start, reconcile\n\nOptions:\n  --network mainnet|testnet (default mainnet SN117; testnet SN566)\n  --state-dir DIR\n  --invitation FILE --secrets-file FILE (init)\n  --release FILE (update)\n  --max-hourly-usd N (deploy/start; storage extra)\n  --model ID --discount-pct PCT | --withdraw (offer)\n  --drain-only (stop)\n  --yes --json --help --version\n\nNative Rust. No Node.js, Python, GPU, wallet seed or wallet signing.\nHosting/provider charges are real. No guaranteed earnings or automatic total spending cap.\n";
pub struct Args {
    pub command: String,
    pub options: HashMap<String, String>,
    pub flags: Vec<String>,
}
const UPDATE_HELP: &str = "\nCLI upgrades (no login required):\n  everycli update          Install the latest stable CLI\n  everycli update --check  Check without installing\nThis does not update the deployed worker; use miner update for that.\n";
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
    need(
        positionals.len() == 2
            && positionals[0] == "miner"
            && [
                "init",
                "doctor",
                "status",
                "providers",
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
            .contains(&positionals[1]),
        "unknown_command",
    )?;
    out.command = positionals[1].into();
    let allowed = match out.command.as_str() {
        "init" => vec!["--invitation", "--secrets-file"],
        "deploy" | "start" => vec!["--max-hourly-usd"],
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
        "init" => {
            let inv = args
                .get("--invitation")
                .map(str::to_string)
                .map(Ok)
                .unwrap_or_else(|| question("Signed invitation file: ", args.flag("--json")))?;
            let creds = args
                .get("--secrets-file")
                .map(str::to_string)
                .map(Ok)
                .unwrap_or_else(|| {
                    question("Private credentials file (mode 600): ", args.flag("--json"))
                })?;
            miner.init(
                &read_json(Path::new(&inv), false)?,
                &read_secrets(Path::new(&creds))?,
            )
        }
        "doctor" => miner.doctor(),
        "status" => miner.status(),
        "providers" => miner.providers(),
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
                serde_json::to_string_pretty(&out).unwrap()
            }
        );
        Ok(
            if (a.command == "doctor" && out["ok"] != true)
                || (a.command == "status"
                    && (!out["coordinatorError"].is_null() || !out["cloudError"].is_null()))
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
                    "{}. Run status/doctor before retrying uncertain operations. Raw responses and secrets suppressed.",
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
