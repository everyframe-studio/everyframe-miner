# Everyframe miner — Rust

Native Rust implementation of the Everyframe attested worker **and `everycli`**.
Neither executable needs Node.js, npm, Python, pip, a GPU, or wallet signing keys.
The worker connects approved provider accounts to the Everyframe coordinator;
it does not run generation models locally.

This ports the existing miner's 45 immutable model contracts, nine provider
adapters, signed/encrypted job protocol, and 14 CLI commands. Provider charges,
hosting charges, operator admission, and routing restrictions remain unchanged.

**Release status:** locally tested port, not a production-admitted worker image.
The checked-in worker intentionally exits with `release_not_configured`. A Rust
image has a new digest and measurements; it needs a new operator-reviewed release
and signed invitation before it can replace a live worker. No live deployment is
performed by building or testing this repository.

## Install everycli as a miner

Use a **prebuilt `everycli` binary**. You do not need Rust, Cargo, Node.js, Python,
Docker, a GPU, or a clone of this repository on your computer to run the CLI.
The miner worker runs separately in the deployed cloud workload.

**Download availability:** a public prebuilt CLI download has not been verified
yet. The steps below apply once the operator publishes binaries and their
SHA-256 checksums to [GitHub Releases](https://github.com/everyframe-studios/everyframe-miner/releases).
The repository's **Source code** archives are not prebuilt CLI downloads.

### Linux installation (no sudo required)

1. Open GitHub Releases and choose a versioned CLI binary matching your machine.
   Run `uname -m` to check your architecture (`x86_64` or `aarch64`); download only
   an architecture actually listed in that release. Check its Linux/glibc
   requirements too. The CLI has been tested on Linux x86-64; other builds must
   be published and validated separately.
2. Download the executable (extract it first if it is archived), save it as
   `everycli`, and open a terminal in that download directory.
3. Copy its published SHA-256 checksum from the same official release. Replace
   the placeholder below, then verify and install:

```sh
(
  set -eu
  expected_sha256='REPLACE_WITH_THE_PUBLISHED_BINARY_SHA256'
  printf '%s  %s\n' "$expected_sha256" everycli | sha256sum --check -
  install -d "$HOME/.local/bin"
  install -m 0755 everycli "$HOME/.local/bin/everycli"
  "$HOME/.local/bin/everycli" --version
  "$HOME/.local/bin/everycli" miner --help
)
```

If the checksum does not match, installation stops; do not run that download.
If the release publishes a checksum for an archive instead, verify the archive
against that checksum **before extracting**, rather than comparing it to the
extracted executable.

Add the installation directory to your current shell's PATH:

```sh
export PATH="$HOME/.local/bin:$PATH"
everycli --version
```

For future terminals, add that `export` line once to your shell configuration
(for example, `~/.bashrc` for Bash). Installing a new version this way replaces
the CLI executable, not your miner profile. Back up your profile before upgrading
and do not replace the CLI while a command is running.

For Windows, use a supported Linux distribution under WSL; native Windows is not
supported. macOS requires a separately published, compatible macOS binary—do not
use the Linux binary. After installation, continue with the CLI workflow below.

## CLI workflow

Obtain a signed invitation from the operator. Store your own credentials in a
private file, mode `0600`, outside this repository:

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
