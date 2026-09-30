# Validation — 2026-09-30

## Live Rust canary — in progress

An explicitly approved $10 / three-tempo canary uses mainnet SN117 UID129
(`everyframe-miner-1/default`), alongside the unchanged original UID127 miner.
It is isolated from customer assignments and synthetic jobs are not reward eligible.

- Fixed a live startup bug: release `chain.genesis` must match network config
  `genesisHash`. Added regression coverage for both networks and chain fencing.
- `cargo test --locked --release`: **37 passed**.
- `cargo clippy --release --all-targets --locked -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- Native CLI deploy, update, reconcile, activate, resume, drain, offer, and
  withdrawal exercised against the real cloud/coordinator.
- Initial and post-activation TEE admission verified, including hardware quote,
  reproduced guest measurements, exact runtime replay, and fresh signing key.
- Three disposable-database tests verify isolated assignment and rollback.
- Corrected canary worker digest:
  `sha256:6b64531358537cb8db3cd679d97052a9306e76c47ed6d1cc4bbf1b9f7a8df2ad`.

- First paid H3 Max Turbo generation succeeded: 1344×768, 24 fps, 5.184-second
  MP4. Signed receipt, output SHA-256, exactly one canary attempt, and complete
  ffmpeg decode verified. Provider billing matched its request ID ($0.10 recorded).
- Three-tempo clock: blocks 9179219–9180302, estimated end 11:04 UTC. At 09:43
  UTC, six jobs completed and verified (H3 Max Turbo and H3 Max). At the user's
  request, the limit was expanded to eight clips without increasing the $10
  total envelope or extending the runtime. Matched generation billing totals
  $1.00 so far, excluding hosting. Customer isolation and automatic canary-only
  cleanup remain enforced.

The three-tempo endurance run is not yet certified complete.
The public checkout remains unconfigured; this does not constitute a general
production release. Private operational records are kept outside this repository.

## Earlier local-only validation

- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo test --locked`: **36 passed**, no failures. Includes all 45 model
  contracts, all nine adapters, and original JavaScript/Python/Phala wire fixtures.
- `cargo build --release --locked --bins`: passed with Rust 1.97.1 on Linux x86-64.
- Native CLI `--version --json`: passed, reports Rust implementation 0.1.0.
- Docker multi-stage build: passed, using the locally available Rust 1.98.0 image
  and Debian trixie slim, both selected by immutable digest.
- CLI container: passed with networking disabled, read-only filesystem, and all
  Linux capabilities dropped.
- Worker container: correctly exits `release_not_configured`, without contacting
  services. No Node.js, npm, Python, or Cargo executable exists in the final image.
- Final worker image: 83,576,372 bytes; final CLI image: 83,637,380 bytes.
- All ten original worker source-file hashes remain unchanged.

Local image tags are `everyframe-miner-rust:local-review` and
`everyframe-miner-rust-cli:local-review`. These are unconfigured smoke-test images,
not approved production releases, and have not been pushed.

Builder: `rust@sha256:7f7a53a25a0319dd8284e279d529d45759cb384d59b14cc6806132910f45522e`

Runtime: `debian@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a`

Earlier native release binary SHA-256 checksums (superseded by the canary fix;
not container binaries):

```text
db970601c0b1588ab16a1f607b6432e83b5232ee5a13574a1840da31c8059cb7  everycli
557193cd39a0e5de4997814a7bb097531106737e8a1d30f206d99b59d228768e  everyframe-worker
```

At the earlier local-only stage, no real provider generation, cloud mutation,
live TEE attestation, rollout, or payment was performed. Synthetic tests are not a substitute for the separately
budgeted, operator-approved canary in RELEASING.md. Debug build artifacts were
removed after testing to conserve server space; release binaries remain available.
