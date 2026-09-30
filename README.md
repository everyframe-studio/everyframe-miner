# Everyframe miner

The Everyframe attested worker **and `everycli`**, its command-line tool for miners.
Neither executable needs Node.js, npm, Python, pip, a GPU, or wallet signing keys.
The worker connects approved provider accounts to the Everyframe coordinator;
it does not run generation models locally.

Includes 45 immutable model contracts, nine provider adapters, a signed/encrypted
job protocol, and 14 miner commands, with built-in CLI self-updates. Provider
charges, hosting charges, operator admission, and routing restrictions remain unchanged.

**Release status:** locally tested port, not a production-admitted worker image.
The checked-in worker intentionally exits with `release_not_configured`. A new
worker image has its own digest and measurements; it needs an operator-reviewed release
and signed invitation before it can replace a live worker. No live deployment is
performed by building or testing this repository.

## Install everycli as a miner

Use a **prebuilt `everycli` binary**. You do not need a compiler, a language runtime,
Docker, a GPU, or a clone of this repository on your computer to run the CLI.
The miner worker runs separately in the deployed cloud workload.

Install with one command:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/everyframe-studios/everyframe-miner/releases/latest/download/everycli-installer.sh | sh
```

**First release pending:** this URL becomes available after a version tag is
pushed and the release workflow successfully publishes all platform builds. See
[GitHub Releases](https://github.com/everyframe-studios/everyframe-miner/releases).
A source-code archive is not a prebuilt CLI download.

The installer detects your platform, verifies the binary's SHA-256 checksum, and
installs into `~/.cargo/bin` (or `$CARGO_HOME/bin` if configured). No development
toolchain is installed or required. It adds a PATH entry to `.profile`, `.bashrc`, and `.zshrc`
without duplicating the entry; symlinked profiles are left unchanged. Restart
your terminal or run the source command printed by the installer, then:

```sh
everycli --version
everycli miner --help
```

Linux x86-64/ARM64 builds require glibc 2.35 or newer (for example, Ubuntu 22.04+).
macOS Intel and Apple Silicon builds run on the corresponding release CI runners
(macOS 15 and 14 respectively). Older macOS versions are not validated. Native
Windows and Alpine/musl Linux are not supported; use a compatible Linux under WSL
on Windows. `curl` and either `sha256sum` or `shasum` are needed for installation.

For a specific release, replace `latest/download` with `download/<tag>`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/everyframe-studios/everyframe-miner/releases/download/v0.1.0/everycli-installer.sh | sh
```

Every release's installer is pinned to that version. To avoid modifying shell
profiles, pipe into `EVERYCLI_NO_MODIFY_PATH=1 sh` instead; add the bin directory to
PATH yourself. Fish users should use this option and configure their Fish PATH.

### Upgrade everycli

```sh
everycli update          # Install the latest stable CLI; no login needed
everycli update --check  # Check for an update without changing anything
everycli --version
```

The built-in updater verifies the release checksum and atomically replaces
the executable in place. It does not access miner credentials, deploy anything,
change profiles, or update running workers. A failed download or checksum leaves
the old executable intact. It will not automatically downgrade. Run updates as
the account that owns the installation; no sudo is needed for a normal install.

`everycli update` upgrades the **CLI**; `everycli miner update --release FILE`
updates a **deployed worker** using an approved signed release. There are no
automatic background update checks. If a documented command is missing, check
`everycli --version` and upgrade (rerun the installer for older CLIs without an
`update` command). Installation does not bypass the current worker admission flow.

## CLI workflow

Store your own credentials in a private file, mode `0600`, outside this repository:

```dotenv
MINER_TOKEN=operator-issued-token
PHALA_CLOUD_API_KEY=your-own-cloud-key
FAL_KEY=your-own-provider-key
```

These are placeholders, not working credentials. Provider keys are optional
individually, but at least one release-approved provider is needed for paid work.
Never put secrets on the command line or in Git. Values are read without shell
execution or `${VARIABLE}` expansion. Public-image releases never send registry
credentials, including stale credentials already stored in a profile.

```sh
everycli miner init --invitation /private/invitation.json --secrets-file /private/miner.env
everycli miner doctor
everycli miner deploy --max-hourly-usd 0.06
everycli miner status
# After exact-image operator admission:
everycli miner activate
# After fresh post-restart admission:
everycli miner resume
everycli miner offers
everycli miner offer --model minimax/h3-max-turbo/text-to-video --discount-pct 10
everycli miner earnings
everycli miner stop
everycli miner reconcile
```

`--max-hourly-usd` is a **compute-rate ceiling, not a total spending budget**.
Storage is extra and may remain billable after shutdown. There is no automatic
total-budget cutoff in this CLI. Provider bills do not decrease when you discount
your mining offer; traffic and earnings are not guaranteed.

| Command | Purpose |
| --- | --- |
| `init` | Verify invitation and store private credentials |
| `doctor`, `status` | Diagnose readiness, admission, routing, and cloud state |
| `providers` | Show credential presence and signed-release permissions, never key values |
| `offers`, `offer` | Inspect, set, or withdraw a revision-checked model bid |
| `earnings` | Read coordinator accounting; does not initiate payouts |
| `deploy` | Create a pinned workload with provider credentials disabled |
| `activate` | Release encrypted provider keys only after fresh admission |
| `resume` | Permit new work after post-restart admission |
| `stop` | Drain and gracefully shut down; `--drain-only` leaves hosting running |
| `start` | Restart the saved stopped workload, with a reviewed hourly ceiling |
| `update` | Install an app-specific signed release; disables providers until readmission |
| `reconcile` | Inspect uncertain operations without replaying paid cloud mutations |

Mainnet SN117 is the default. Use `--network testnet` for SN566. Default state
paths match the original CLI: `~/.config/everycli-mainnet117` and
`~/.config/everycli`. `--state-dir` selects an explicit profile. Testnet alone
honors `EVERYCLI_DIR`. State directories must be owner-only (`0700`); files `0600`.
Back up an existing profile before switching CLI implementations. Do not operate
the same profile with two CLI processes at once.

All commands accept `--json`; modifying commands also accept `--yes` for explicit
noninteractive confirmation. Without `--yes`, noninteractive mutations fail.
Exit codes: `0` success, `1` command error, `2` unhealthy diagnosis or unresolved
reconciliation. Human output is readable JSON; it is not an exact reproduction
of the Python CLI's display formatting.

## Providers

Fal, MiniMax, OpenRouter, BFL, Replicate, Google, Runway, Luma, and ElevenLabs.
Exact contracts and immutable parameters are in `config/models.json`.
Credential names are centralized in `src/invitation.rs`. Possessing a provider key
does **not** enable a model: the operator must separately approve and price it.

## Tests

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Tests use synthetic keys and in-memory transports, not live accounts or paid
generation. They cover original cross-language signature/encryption fixtures,
all 45 contract hashes and adapter mappings, credential isolation, nonce binding,
stable completion receipts, ambiguous submission recovery, durable cloud intents,
admission, lifecycle commands, bids, filesystem security, and fail-closed startup.

See [COMPATIBILITY.md](COMPATIBILITY.md), [SECURITY.md](SECURITY.md), and
[RELEASING.md](RELEASING.md) before deployment or publication.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
See [LICENSE-NOTICE.md](LICENSE-NOTICE.md) for third-party licensing guidance.
