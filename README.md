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
without duplicating the entry; symlinked profiles are left unchanged. If the bin
directory is already on your PATH, you can use `everycli` immediately. Otherwise,
run the source command printed by the installer or open a new terminal, then:

```sh
everycli --version
everycli miner --help
```

Linux x86-64/ARM64 builds require glibc 2.35 or newer (for example, Ubuntu 22.04+).
macOS Intel and Apple Silicon builds run on the corresponding release CI runners
(macOS 15 and 14 respectively). Older macOS versions are not validated. Native
Windows and Alpine/musl Linux are not supported; use a compatible Linux under WSL
on Windows. `curl` and either `sha256sum` or `shasum` are needed for installation.

To avoid modifying shell profiles, pipe into `EVERYCLI_NO_MODIFY_PATH=1 sh`
instead; add the bin directory to
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

Self-service onboarding requires **everycli v0.1.4 or later**. Run `everycli update`
first if you installed an older release. Updating the CLI does not migrate an
existing token-based worker automatically.

Before deployment, you need:

- A miner hotkey registered on SN117. `register-hotkey` checks and records subnet
  membership; on-chain registration is done separately using your wallet tool.
- A funded Phala Cloud account and its API key for the attested workload.
- At least one funded provider account supported by your deployment configuration.
- Your local miner hotkey file. No invitation file or `MINER_TOKEN` is needed.

The downloads and public worker images do not require registry credentials.
`init` proves hotkey ownership, verifies finalized subnet membership on the server,
and automatically downloads the signed deployment configuration. The CLI checks
its pinned coordinator signature. The deployed worker must then pass exact
image/TEE attestation before provider keys are enabled. Downloading the CLI alone
does not register or activate a miner, and no coldkey transaction is signed.

### 1. Check your hotkey

```sh
everycli miner register-hotkey --wallet my-miner --hotkey default
```

This reads the local hotkey's public address, checks the pinned chain genesis,
and verifies SN117 membership at a finalized block through the Bittensor RPC.
It records the UID and local keyfile path, never a seed. If you already registered
elsewhere, use `--hotkey-ss58 YOUR_SS58_ADDRESS` instead; no local wallet or
`btcli` is needed for this public lookup. Local signing still requires your
hotkey file during `init`.

If the hotkey is absent, the command exits unsuccessfully and, when wallet names
were supplied, prints the `btcli subnet register` command for you to run yourself.
It never creates a wallet, pays registration fees, or signs an on-chain transaction.
This membership check is not proof of ownership or coordinator admission.
For testnet SN566, pass `--network testnet` to each command; networks are not sticky.

### 2. Set your API keys

```sh
everycli miner set-api-keys
```

The CLI prompts for Phala Cloud and all nine generation providers with hidden
input. Enter skips a provider or keeps its existing key. You need a funded Phala
account and at least one release-approved generation provider, not all nine.
Keys are saved in an owner-only local profile (`credentials.json`, mode `0600`);
you do not need to create or maintain a `.env` file. Local storage is not encrypted
at rest. Never commit the profile or share its contents.

To change just one provider, or remove a locally saved key:

```sh
everycli miner set-api-keys --provider minimax
everycli miner set-api-keys --provider phala
everycli miner remove-api-key --provider fal
everycli miner providers
```

Provider names: `phala`, `fal`, `minimax`, `openrouter`, `bfl`, `replicate`,
`google`, `runway`, `luma`, `elevenlabs`. For automation, pipe a secret manager's
output into `everycli miner set-api-keys --provider NAME --stdin`; never put the
key itself in command arguments or shell history. `--json` never enables prompts.

Saving or removing keys is local-only. To apply saved generation keys to an
already activated worker, run `everycli miner apply-api-keys`. This asks for
confirmation, drains work, refuses to restart while jobs remain, checks the
reviewed workload, and encrypts credentials to its pinned key. Wait for fresh
post-restart admission, then run `everycli miner resume`. Only providers allowed
by the signed deployment can be applied. At least one must remain configured;
if retiring the last provider, stop the worker instead. Removing a local key
does not revoke it at the provider—revoke leaked credentials there immediately.

### 3. Initialize and deploy

```sh
everycli miner init
everycli miner doctor
everycli miner deploy --max-hourly-usd 0.06
everycli miner status
```

`init` reuses the hotkey path recorded in step 1 and the saved API keys. If you
used the public-address lookup, also provide `--wallet my-miner --hotkey default`
or `--hotkey-file FILE`. Existing automation can still use `--secrets-file FILE`
to import an owner-only dotenv file; omitted credentials are preserved. The import
does not execute shell expressions. Public-image deployments never send registry
credentials, even if stale registry keys exist locally.

`doctor` can report that no workload is deployed
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
bounded by the deployment configuration's expiry. Renew with `init` before
expiry (it fetches a fresh signed configuration), then use `apply-api-keys` for an activated worker
and wait for fresh admission before resuming work.

Older token-based deployments remain compatible until explicitly migrated.
Simply deleting a token from an old profile will not migrate it: the coordinator
binding, deployment configuration, and worker image must all support hotkey auth.

Activation happens in stages. Check `status` and `doctor` between them; do not
run through admission failures or repeatedly retry an uncertain deployment.

```sh
# Once status shows accepted attestation:
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
| `register-hotkey` | Check finalized subnet membership and record the public identity; no chain transaction |
| `set-api-keys` | Privately prompt for all keys or one provider; save locally |
| `remove-api-key` | Remove one local credential after confirmation; does not revoke it remotely |
| `apply-api-keys` | Explicitly drain and apply saved keys to an activated, reviewed worker; fresh admission required |
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
imply consent. `init` with a local hotkey and saved credentials does not prompt
for confirmation and does not create a paid VM. The optional legacy deployment
file import remains available for existing managed deployments.
Exit codes: `0` success, `1` command error, `2` unhealthy diagnosis or unresolved
reconciliation (also an unregistered hotkey). Normal command output is formatted
JSON; `--json` uses compact JSON.

`earnings` shows coordinator accounting, not proof of an on-chain alpha payment.
Reward eligibility, validator weight submission, reveal, and chain emissions are
separate steps. The CLI does not initiate payouts.

## Providers

| Provider | `--provider` | Credential variable (optional file import) |
| --- | --- | --- |
| Fal | `fal` | `FAL_KEY` |
| MiniMax (direct API) | `minimax` | `MINIMAX_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
| Black Forest Labs | `bfl` | `BFL_API_KEY` |
| Replicate | `replicate` | `REPLICATE_API_TOKEN` |
| Google | `google` | `GEMINI_API_KEY` |
| Runway | `runway` | `RUNWAYML_API_SECRET` |
| Luma | `luma` | `LUMA_API_KEY` |
| ElevenLabs | `elevenlabs` | `ELEVENLABS_API_KEY` |

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

`config/release.json` pins the SN117 coordinator and chain. Self-service setup
selects the approved digest-pinned worker automatically; miners do not build an
image or supply registry credentials. A locally modified image is not automatically
admitted. Unknown OS measurements or KMS CA identities fail attestation rather
than being trusted on first use. The KMS runtime probe can vary between boots;
the coordinator still verifies the hardware configuration binding and complete
boot transcript. Phala remains the trusted KMS operator; see [SECURITY.md](SECURITY.md).

The worker never receives your hotkey private key. The CLI reads the hotkey only
to sign scoped authentication, never to move funds. Keep hotkeys, credentials,
and private state out of Git. See [SECURITY.md](SECURITY.md) for security guidance,
[COMPATIBILITY.md](COMPATIBILITY.md) for protocol details, and
[RELEASING.md](RELEASING.md) for maintainer release procedures.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
See [LICENSE-NOTICE.md](LICENSE-NOTICE.md) for third-party licensing guidance.
