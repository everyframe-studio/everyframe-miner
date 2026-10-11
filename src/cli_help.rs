//! One discoverable help page per public command. No profile or network needed.
pub const COMMANDS: &[&str] = &[
    "help",
    "setup",
    "register-hotkey",
    "set-api-keys",
    "remove-api-key",
    "init",
    "doctor",
    "status",
    "providers",
    "balances",
    "offers",
    "offer",
    "earnings",
    "deploy",
    "activate",
    "apply-api-keys",
    "resume",
    "worker-update",
    "stop",
    "start",
    "reconcile",
    "update",
];

pub fn public_name(command: &str) -> &str {
    match command {
        "self-update" => "update",
        "update" => "worker-update",
        c => c,
    }
}

pub fn render(command: &str) -> String {
    let name = public_name(command);
    let (summary, options, example) = match name {
        "setup" => (
            "Guided first-time setup: check registration, save keys, initialize, then review deployment. Interactive only. Existing deployment records stop the wizard; it never activates or resumes paid work automatically.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE\n  --max-hourly-usd N  Optional compute ceiling; prompted if omitted\n  --provider NAME     Optional generation provider to configure (Phala is also prompted)",
            "setup --wallet my-wallet --hotkey default --max-hourly-usd 0.05",
        ),
        "register-hotkey" => (
            "Check finalized subnet membership. This does NOT register on-chain or pay a registration fee.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE | --hotkey-ss58 ADDRESS",
            "register-hotkey --wallet my-wallet --hotkey default",
        ),
        "init" => (
            "Verify registration and prove hotkey ownership locally to fetch the signed profile. No VM is created.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE\n  Saved registration can supply the signer.\n  --invitation FILE   Optional legacy signed configuration\n  --secrets-file FILE Optional owner-only credential import",
            "init --wallet my-wallet --hotkey default",
        ),
        "set-api-keys" => (
            "Save keys locally with hidden prompts. Enter keeps an existing key; running workers are unchanged.",
            "  --provider NAME  Select one provider (phala, fal, vivix, etc.)\n  --stdin          With --provider, read one key from a pipe; never pass keys as arguments",
            "set-api-keys --provider fal",
        ),
        "remove-api-key" => (
            "Remove a locally saved key. Does not revoke it or change a running worker.",
            "  --provider NAME  Required\n  --yes            Required without an interactive terminal",
            "remove-api-key --provider fal --yes",
        ),
        "doctor" => (
            "Check profile, connectivity and worker readiness. No generation is submitted.",
            "  No command-specific options.",
            "doctor",
        ),
        "status" => (
            "Read local deployment and signed coordinator status.",
            "  No command-specific options.",
            "status --json",
        ),
        "providers" => (
            "List saved key presence and signed-release permissions. Keys are never displayed; presence is not a billing/authentication check.",
            "  No command-specific options. Works before initialization.",
            "providers",
        ),
        "balances" => (
            "Read provider USD balances and synced snapshots. No generation is submitted.",
            "  --publish  Publish amounts only to the miner's private coordinator view",
            "balances --publish",
        ),
        "offers" => (
            "List available model offers, discounts and pricing status.",
            "  No command-specific options.",
            "offers",
        ),
        "offer" => (
            "Set or withdraw a model offer. Review the quote before confirmation; no guaranteed traffic or profit.",
            "  --model ID          Required; choose from everycli offers\n  --discount-pct PCT  Required unless --withdraw; 0–99.90, up to 2 decimals\n  --withdraw          Withdraw the offer; cannot combine with --discount-pct",
            "offer --model minimax/h3-max-turbo/text-to-video --discount-pct 10",
        ),
        "earnings" => (
            "Read recorded serving fees. Fees are not wallet payments, measured alpha or net profit.",
            "  No command-specific options.",
            "earnings",
        ),
        "deploy" => (
            "Review the live hosting quote, then create a worker after confirmation. Storage costs extra; the hourly ceiling is NOT a total budget. Existing deployments are never recreated.",
            "  --max-hourly-usd N  Required positive finite compute ceiling",
            "deploy --max-hourly-usd 0.05",
        ),
        "start" => (
            "Review the compute quote and start the existing stopped worker. Storage costs extra; no total spending cap.",
            "  --max-hourly-usd N  Required positive finite compute ceiling",
            "start --max-hourly-usd 0.05",
        ),
        "activate" => (
            "Release provider keys only to the verified workload, then restart it. Wait for fresh attestation before resume.",
            "  No command-specific options.",
            "activate",
        ),
        "apply-api-keys" => (
            "Drain and restart an existing worker with saved keys after confirmation and verification. Wait for fresh attestation before resume.",
            "  No command-specific options.",
            "apply-api-keys",
        ),
        "resume" => (
            "Accept paid work after verification and confirmation. Your provider account pays for assignments; a current offer is also required.",
            "  No command-specific options.",
            "resume",
        ),
        "worker-update" => (
            "Apply an approved signed worker release. This does not upgrade the CLI; preserve state and reconcile uncertain operations.",
            "  --release FILE  Required signed worker release",
            "worker-update --release ./worker-release.json",
        ),
        "stop" => (
            "Drain work and stop the existing worker after confirmation. Stopped workloads may retain billable storage.",
            "  --drain-only  Pause new work without stopping the VM",
            "stop --drain-only",
        ),
        "reconcile" => (
            "Inspect a recorded uncertain operation and reconcile its state. Never blindly repeat a cloud mutation.",
            "  No command-specific options.",
            "reconcile",
        ),
        "update" => (
            "Upgrade this CLI to the latest stable release. No miner login needed; profiles and deployed workers are unchanged.",
            "  --check  Check the available version without installing",
            "update --check",
        ),
        _ => {
            return format!(
                "{}{}\nRun everycli help COMMAND for command-specific options and an example.\n\nExample:\n  everycli setup --wallet my-wallet --hotkey default\n",
                crate::cli::HELP,
                crate::cli::UPDATE_HELP
            );
        }
    };
    let common = if name == "update" {
        "  --json  Machine-readable output\n  --help  Show this page"
    } else if name == "setup" {
        "  --network mainnet|testnet (default mainnet SN117; testnet SN566)\n  --state-dir DIR\n  --help\n  Interactive only; --yes/--json are not supported."
    } else {
        "  --network mainnet|testnet (default mainnet SN117; testnet SN566)\n  --state-dir DIR\n  --json\n  --yes  Approve confirmation prompts (does not bypass safety checks)\n  --help"
    };
    format!(
        "everycli {name}\n\n{summary}\n\nUsage: everycli {name} [options]\n\nOptions:\n{options}\n\nCommon options:\n{common}\n\nExample:\n  everycli {example}\n"
    )
}

pub fn suggestion(word: &str) -> Option<&'static str> {
    if word.len() > 40 || !word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return None;
    }
    fn distance(a: &str, b: &str) -> usize {
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, x) in a.bytes().enumerate() {
            let mut diagonal = row[0];
            row[0] = i + 1;
            for (j, y) in b.bytes().enumerate() {
                let above = row[j + 1];
                row[j + 1] = (row[j] + 1)
                    .min(above + 1)
                    .min(diagonal + usize::from(x != y));
                diagonal = above;
            }
        }
        row[b.len()]
    }
    COMMANDS
        .iter()
        .map(|c| (*c, distance(word, c)))
        .min_by_key(|(_, d)| *d)
        .filter(|(_, d)| *d <= 3)
        .map(|(c, _)| c)
}
