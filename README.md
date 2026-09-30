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

## Build and use

Requires a Rust toolchain supporting edition 2024 (minimum declared version 1.88),
a C linker, and a Unix environment: Linux, macOS, or Linux under WSL. Native
Windows is not supported because credential-file protections use POSIX ownership,
permissions, and `O_NOFOLLOW`. Linux is the worker deployment target.

```sh
cargo build --release --locked --bins
./target/release/everycli --version
./target/release/everycli miner --help
```

The two distributable binaries are `target/release/everycli` and
`target/release/everyframe-worker`. Copy `everycli` to a directory on your PATH,
or install it from this checkout with:

```sh
cargo install --path . --locked --bin everycli
```

The binaries include the reviewed configuration at compile time; no source tree,
JavaScript/Python interpreter, writable model registry, or external CA override is
required at runtime. Linux binaries still require a compatible system C runtime.

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
