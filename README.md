# Everyframe miner

Run an Everyframe miner on Bittensor **SN117** with `everycli` and an attested
cloud worker. The worker receives jobs from the Everyframe coordinator, calls
your configured generation providers, and returns signed completion receipts.
It does not run generation models locally, so you do not need a GPU.

This repository includes the worker, the CLI with built-in updates, 45 model
contracts, and nine provider adapters. Available jobs depend on the coordinator's
enabled models, current pricing, your credentials, and your active offers—not
every model in the registry is necessarily available for mining.

## Install everycli as a miner

Use a **prebuilt `everycli` binary**. You do not need a compiler, a language runtime,
Docker, a GPU, or a clone of this repository on your computer to run the CLI.
The miner worker runs separately in the deployed cloud workload.

Install with one command:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/everyframe-studios/everyframe-miner/releases/latest/download/everycli-installer.sh | sh
```

Prebuilt binaries and checksums are published on
[GitHub Releases](https://github.com/everyframe-studios/everyframe-miner/releases).
No GitHub account or token is needed to download them. A source-code archive is
not a prebuilt CLI download.

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
  https://github.com/everyframe-studios/everyframe-miner/releases/download/v0.1.1/everycli-installer.sh | sh
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
`update` command).

Checksums detect corrupted or mismatched downloads; they are not independent
publisher signatures. The installer and updater trust this GitHub repository
and HTTPS.

## Set up a miner

The hotkey flow below requires **everycli v0.1.2 or later**. The currently
published v0.1.1 uses the legacy token flow; these changes must be included in
the next CLI and worker release before upgrading an existing deployment.

Before deployment, you need:

- A miner hotkey registered on SN117. Registration is separate from `everycli`;
  the CLI does not create wallets, register hotkeys, or sign wallet transactions.
- A funded Phala Cloud account and its API key for the attested workload.
- At least one funded provider account supported by your deployment configuration.
- Your local miner hotkey file and a hotkey-enabled signed deployment
  configuration, passed to `init` as `--invitation`. No `MINER_TOKEN` is needed.

The downloads and public worker images do not require registry credentials.
The current CLI still validates the signed deployment configuration and requires
coordinator attestation approval before enabling provider keys. Hotkey
authentication does not bypass image/TEE admission. Downloading the
CLI alone does not register or activate a miner.

Store your own credentials in a private file, mode `0600`, outside this repository:

```dotenv
# Required for cloud hosting (not a generation provider).
PHALA_CLOUD_API_KEY=your-own-cloud-key

# Generation providers: fill the keys you use; leave unused entries empty.
FAL_KEY=
MINIMAX_API_KEY=
OPENROUTER_API_KEY=
BFL_API_KEY=
REPLICATE_API_TOKEN=
GEMINI_API_KEY=
RUNWAYML_API_SECRET=
LUMA_API_KEY=
ELEVENLABS_API_KEY=
```

You can configure multiple providers in the same file. Replace the cloud-key
placeholder and fill at least one provider key allowed by your deployment
configuration; you do not need all nine. Empty provider entries are ignored.

Use your actual file path when securing it:

```sh
chmod 600 /private/miner.env
```

The example contains no working credentials. Provider keys are optional
individually, but at least one release-approved provider is needed for paid work.
Never put secrets on the command line or in Git. Values are read without shell
execution or `${VARIABLE}` expansion. Public-image releases never send registry
credentials, including stale credentials already stored in a profile.

```sh
everycli miner init \
  --wallet my-miner --hotkey default \
  --invitation /private/deployment.json \
  --secrets-file /private/miner.env
everycli miner doctor
everycli miner deploy --max-hourly-usd 0.06
everycli miner status
```

The paths above are examples. `doctor` can report that no workload is deployed
before the initial `deploy`. Review its diagnosis rather than treating every
pre-deployment warning as an installation failure. The hourly ceiling is an
example; deployment is refused if the quoted compute rate exceeds it.

`--wallet my-miner --hotkey default` reads
`~/.bittensor/wallets/my-miner/hotkeys/default` locally. Alternatively, use
`--hotkey-file /absolute/path/to/hotkey` instead of those two options. The current
reader supports an unencrypted sr25519 wallet JSON containing `secretSeed`;
the file must be owner-only. Encrypted keyfiles are rejected rather than silently
decrypted or uploaded. Never paste a seed into command arguments.

The hotkey signs separate, expiring console and worker authorizations. Only the
worker's restricted delegate is sent through Phala's encrypted environment;
the hotkey and console delegate stay on your computer. This does not sign a
blockchain transaction or access your coldkey. Delegations last at most 30 days,
bounded by the deployment configuration's expiry. Renew with `init` using a
current configuration before expiry, then activate the updated worker credentials
and wait for fresh admission before resuming work.

Older token-based deployments remain compatible until explicitly migrated.
Simply deleting a token from an old profile will not migrate it: the coordinator
binding, deployment configuration, and worker image must all support hotkey auth.

Activation happens in stages. Check `status` and `doctor` between them; do not
run through admission failures or repeatedly retry an uncertain deployment.

```sh
# After exact-image operator admission:
everycli miner activate
# After fresh post-restart admission:
everycli miner resume
everycli miner offers
# Example: offer one enabled model at a 10% discount:
everycli miner offer --model minimax/h3-max-turbo/text-to-video --discount-pct 10
everycli miner earnings
```

`resume` permits work but does not create a model offer. Choose an available
model from `offers`, then submit your bid. The model above is an example, not
a guarantee that it is enabled for every deployment.

`--max-hourly-usd` is a **compute-rate ceiling, not a total spending budget**.
Storage is extra and may remain billable after shutdown. There is no automatic
total-budget cutoff in this CLI. Provider bills do not decrease when you discount
your mining offer; traffic and earnings are not guaranteed.

### Pause or stop

```sh
everycli miner stop --drain-only  # Stop accepting new work; keep hosting running
everycli miner stop              # Request shutdown when no active jobs remain
everycli miner reconcile         # Check whether the requested operation completed
```

If jobs are still active, `stop` returns a draining status and leaves the VM
running. Wait for those jobs to finish, then run `stop` again; it does not
schedule a later shutdown automatically.

Shutdown does not delete the VM or its storage. Check Phala Cloud for retained
resources and ongoing charges. To restart the saved workload, use
`everycli miner start --max-hourly-usd 0.06`, check admission, and resume when ready.

### Command reference

| Command | Purpose |
| --- | --- |
| `init` | Verify deployment configuration; sign local hotkey delegations and store credentials |
| `doctor`, `status` | Diagnose readiness, admission, routing, and cloud state |
| `providers` | Show credential presence and signed-release permissions, never key values |
| `offers`, `offer` | Inspect, set, or withdraw a revision-checked model bid |
| `earnings` | Read coordinator accounting; does not initiate payouts |
| `deploy` | Create a pinned workload with provider credentials disabled |
| `activate` | Release encrypted provider keys only after fresh admission |
| `resume` | Permit new work after post-restart admission |
| `stop` | Drain; request shutdown only when idle. `--drain-only` keeps hosting running |
| `start` | Restart the saved stopped workload, with a reviewed hourly ceiling |
| `update` | Install an app-specific signed release; disables providers until readmission |
| `reconcile` | Inspect uncertain operations without replaying paid cloud mutations |

Mainnet SN117 is the default. Use `--network testnet` for SN566. Default state
paths match the original CLI: `~/.config/everycli-mainnet117` and
`~/.config/everycli`. `--state-dir` selects an explicit profile. Testnet alone
honors `EVERYCLI_DIR`. State directories must be owner-only (`0700`); files `0600`.
Back up an existing profile before switching CLI implementations. Do not operate
the same profile with two CLI processes at once.

Commands accept `--json` for machine-readable output. Lifecycle operations and
offer changes require interactive confirmation or `--yes`; `--json` does not
imply consent. `init` with both input files supplied does not prompt for confirmation.
Exit codes: `0` success, `1` command error, `2` unhealthy diagnosis or unresolved
reconciliation. Normal command output is formatted JSON; `--json` uses compact JSON.

`earnings` shows coordinator accounting, not proof of an on-chain alpha payment.
Reward eligibility, validator weight submission, reveal, and chain emissions are
separate steps. The CLI does not initiate payouts.

## Providers

| Provider | Credential variable |
| --- | --- |
| Fal | `FAL_KEY` |
| MiniMax (direct API) | `MINIMAX_API_KEY` |
| OpenRouter | `OPENROUTER_API_KEY` |
| Black Forest Labs | `BFL_API_KEY` |
| Replicate | `REPLICATE_API_TOKEN` |
| Google | `GEMINI_API_KEY` |
| Runway | `RUNWAYML_API_SECRET` |
| Luma | `LUMA_API_KEY` |
| ElevenLabs | `ELEVENLABS_API_KEY` |

Use the key for the provider serving the contract, not just the model's brand.
For example, a MiniMax model served through Fal uses `FAL_KEY`; the direct
MiniMax adapter uses `MINIMAX_API_KEY`.

Exact contracts and immutable parameters are in `config/models.json`.
Credential names are centralized in `src/invitation.rs`. Possessing a provider key
does **not** enable a model: the operator must separately approve and price it.

After initialization, check which providers are configured and permitted, then
inspect the available model offers:

```sh
everycli miner providers  # Reports key presence and permissions, never key values
everycli miner offers
```

## Tests

For contributors only; miners installing a release do not need a compiler or
to run these checks.

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

## Worker releases and security

CLI downloads and deployed worker images are separate releases. Updating the CLI
does not change the image selected by your deployment configuration.

The source checkout's `config/release.json` is an **unconfigured build template**,
not the configuration of a deployed worker. Building a worker unchanged from that
template makes it exit with `release_not_configured`; this does not prevent the
prebuilt CLI from working. Worker publishers must configure the reviewed trust
pins, publish a digest-pinned image, and have its exact measurements admitted.
Miners using an existing deployment configuration do not need to build an image.

The worker never receives your hotkey private key. The CLI reads the hotkey only
to sign scoped authentication, never to move funds. Keep hotkeys, credentials,
and private state out of Git. See [SECURITY.md](SECURITY.md) for security guidance,
[COMPATIBILITY.md](COMPATIBILITY.md) for protocol details, and
[RELEASING.md](RELEASING.md) for maintainer release procedures.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
See [LICENSE-NOTICE.md](LICENSE-NOTICE.md) for third-party licensing guidance.
