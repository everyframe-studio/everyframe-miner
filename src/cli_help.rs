//! One discoverable help page per public command. No profile or network needed.
pub const COMMANDS: &[&str] = &[
    "help",
    "setup",
    "register-hotkey",
    "set-api-keys",
    "remove-api-key",
    "init",
    "reconnect",
    "doctor",
    "health",
    "status",
    "providers",
    "balances",
    "offers",
    "offer",
    "offer-batch",
    "jobs",
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
    "auto-update",
    "ui",
    "delete",
    "logs",
    "label",
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
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE\n  --max-hourly-usd N  Optional compute ceiling; prompted if omitted\n  --provider NAME     Optional generation provider to configure (Phala is also prompted)\n  --continue          Show the numbered checklist for an existing local deployment; no paid operations",
            "setup --wallet my-wallet --hotkey default --max-hourly-usd 0.05",
        ),
        "register-hotkey" => (
            "Check finalized subnet membership. This does NOT register on-chain or pay a registration fee.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE | --hotkey-ss58 ADDRESS",
            "register-hotkey --wallet my-wallet --hotkey default",
        ),
        "reconnect" => (
            "Refresh a saved profile with proof from the same local hotkey. Keeps API keys, label and deployment records. Requires confirmation. Never creates, deletes, starts or updates a VM, changes bids or resumes work.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE\n  The saved registration can supply the signer. Rerun to finish an interrupted reconnect.",
            "reconnect --wallet my-wallet --hotkey default",
        ),
        "init" => (
            "Verify registration and prove hotkey ownership locally to fetch the signed profile. No VM is created.",
            "  --wallet NAME --hotkey NAME | --hotkey-file FILE\n  Saved registration can supply the signer.\n  --invitation FILE   Optional legacy signed configuration\n  --secrets-file FILE Optional owner-only credential import",
            "init --wallet my-wallet --hotkey default",
        ),
        "set-api-keys" => (
            "Save keys locally with hidden prompts showing Saved or Not saved. Enter keeps saved keys or skips missing ones; running workers are unchanged.",
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
        "health" => (
            "Read-only readiness checklist with green PASS, amber WARN and red FAIL rows. All green means current readiness checks passed, not a paid generation test or guaranteed traffic. Exit 0 when all checks pass; 2 otherwise. Use --json for structured checks.",
            "  No command-specific options. No activation, restart or generation is submitted.",
            "health",
        ),
        "status" => (
            "Show deployment, worker, routing and job activity as tables. Use --json for the original signed coordinator fields. For a complete readiness checklist, run everycli health.",
            "  No command-specific options.",
            "status",
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
            "List Studio models, offers, discounts and model readiness. A saved key does not enable routing.",
            "  No command-specific options.",
            "offers",
        ),
        "offer" => (
            "Set or withdraw a model offer. Review the quote before confirmation; no guaranteed traffic or profit.",
            "  --model ID          Required; choose from everycli offers\n  --discount-pct PCT  Required unless --withdraw; 0–99.90, up to 2 decimals\n  --withdraw          Withdraw the offer; cannot combine with --discount-pct",
            "offer --model minimax/h3-max-turbo/text-to-video --discount-pct 10",
        ),
        "earnings" => (
            "Read all-time work value, review status and quoted provider exposure. Separates accounting from payments; actual provider bills, hosting costs, alpha received and net profit remain unmeasured.",
            "  No command-specific options.",
            "earnings",
        ),
        "jobs" => (
            "Read your individual attempts, newest first, including verification, review status and safe failure reasons. No prompts, media or other miners' history. Completed means verified output, not payment. No generation is submitted.",
            "  --limit N      Page size, 1–100 (default 20)\n  --status NAME  all, active, completed, failed or cancelled\n  --model ID     Filter by EveryFrame contract ID\n  --cursor VALUE Next-page cursor; keep the same status/model filters\n  --summary      All-time per-model counts; cannot combine with filters",
            "jobs --status failed --limit 20",
        ),
        "offer-batch" => (
            "Review and apply multiple offers atomically. Changed pricing or revisions reject the whole batch. No automatic retry.",
            "  --models ID,ID      Required comma-separated model IDs (up to 128)\n  --discount-pct PCT  Required unless --withdraw; 0–99.90, up to 2 decimals\n  --withdraw          Withdraw active selected offers",
            "offer-batch --models minimax/h3-max-turbo/text-to-video,minimax/h3-max/text-to-video --discount-pct 10",
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
            "Fetch the latest compatible signed worker update, drain jobs, update the existing VM, wait for attestation and restore its previous accepting/paused state. Previously active keys are reactivated. Offers and discounts stay unchanged. Rerun to continue a saved update; uncertain cloud mutations are never blindly repeated. Use the original management profile. This does not upgrade the CLI.",
            "  --check         Check availability without changing the worker\n  --abort         Cancel an unfinished update and keep the release the VM is running\n  --release FILE  Advanced: apply an explicit signed file (manual reconcile/activation)",
            "worker-update",
        ),
        "stop" => (
            "Drain work and stop the existing worker after confirmation. Stopped workloads may retain billable storage.",
            "  --drain-only  Pause new work without stopping the VM\n  --force       Emergency shutdown without coordinator availability; may lose paid in-flight work",
            "stop --drain-only",
        ),
        "reconcile" => (
            "Inspect a recorded uncertain operation and reconcile its state. Never blindly repeat a cloud mutation.",
            "  No command-specific options.",
            "reconcile",
        ),
        "delete" => (
            "Permanently delete the verified VM and its storage after exact-ID confirmation. Also deletes a worker with this miner's name that another computer deployed (listed by status). Stops compute/storage billing for that VM, not unrelated resources. After deletion, deploy can create a new worker. Irreversible; in-flight work may be lost.",
            "  --app-id ID  Required exact VM ID from status\n  --yes        Explicit consent without an interactive terminal",
            "delete --app-id EXACT_APP_ID",
        ),
        "logs" => (
            "Read bounded, sanitized worker events. Does not enable public log access; private logs may require the Phala console.",
            "  No command-specific options.",
            "logs",
        ),
        "label" => (
            "Show or set a local worker label. Does not change the signed VM name or identity.",
            "  --name NAME  Local display name, up to 64 safe characters",
            "label --name studio-worker-1",
        ),
        "ui" => (
            "Open a local browser page with a step-by-step setup guide, a live status dashboard, model offers, API keys and worker actions. It runs the same checks and asks the same questions as the terminal commands. Listens on 127.0.0.1 only; the printed link works once. On a remote server, forward the port with ssh -L. Closing the UI never stops your worker.",
            "  --port N    Local port (default 47117, or the next free one)\n  --no-open   Print the link without opening a browser\n  --network / --state-dir select the profile, as for other commands",
            "ui",
        ),
        "auto-update" => (
            "Show automatic CLI update settings and the last attempt. Native user installs check in the background after successful interactive commands and when the UI starts, at most once every six hours. Downloads are checksum-verified and installed atomically for the next invocation; reopen the UI after an update. Never updates workers or changes offers. JSON/CI/help/version invocations do not launch the updater.",
            "  --enable   Enable automatic CLI updates (default)\n  --disable  Disable automatic CLI updates; manual everycli update still works\n  EVERYCLI_DISABLE_AUTO_UPDATE=1 disables background updates for this environment.\n  No flags: show status. Settings apply across miner profiles.",
            "auto-update --disable",
        ),
        "update" => (
            "Upgrade this CLI to the latest stable release. Shows download progress and a readable summary; --json emits only the structured result. No miner login needed; profiles and deployed workers are unchanged.",
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
    let common = if name == "update" || name == "auto-update" {
        "  --json  Machine-readable output\n  --help  Show this page"
    } else if name == "setup" {
        "  --network mainnet|testnet (default mainnet SN117; testnet SN566)\n  --state-dir DIR\n  --help\n  New setup is interactive only: no --yes/--json.\n  --continue is read-only and also supports --json without a terminal."
    } else if ![
        "reconnect",
        "deploy",
        "start",
        "activate",
        "apply-api-keys",
        "resume",
        "worker-update",
        "stop",
        "offer",
        "offer-batch",
        "remove-api-key",
    ]
    .contains(&name)
    {
        "  --network mainnet|testnet (default mainnet SN117; testnet SN566)\n  --state-dir DIR\n  --json\n  --help"
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
