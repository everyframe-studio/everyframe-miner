# Validation

## Worker safety and CLI recovery — 14 October 2026 (unreleased)

Implemented on `fix/worker-safety-and-cli-recovery`; this is local verification,
not a published CLI, reviewed worker image, or live coordinator deployment.

- Terminal polling/result errors stop generation polling and report failure.
  Failure acknowledgements are idempotent; already-paid jobs are not resubmitted.
- Paid requests are deadline-fenced after reference preparation and again after DNS.
  Only Fal's explicit exhausted-balance rejection enables safe account failover.
- Worker/coordinator uploads allow 180 seconds; acceptance still enforces job deadlines.
- Vivix W=0 is rejected before generation. Any positive W amount is allowed;
  unknown balances are not invented as zero. Coordinator serialization covers
  miners using the same reported Vivix key, not unrelated external applications
  or different keys belonging to the same provider workspace.
- Emergency stop bypasses coordinator availability, not signed cloud identity.
  Delete requires an exact app ID, confirmation, a durable intent, and a subsequent
  cloud not-found check before reporting deletion confirmed.
- New v2 hotkey delegates are non-expiring and key-version revocable. Legacy v1
  expiry remains binding. Coordinator and worker support must be deployed before
  re-signing legacy profiles with `init` and installing new worker credentials.
- Kernel locks survive process death without manual file removal. Candidate CLI
  binaries run a bounded version check before replacement and retain a rollback
  binary. Source builds do not self-update. Idle logs are limited to once/minute.
- Added scoped worker logs, local profile labels, a signed public pricing catalog,
  and README uninstall/storage-billing instructions.

Coordinator: **172 tests passed**. Miner regression suite, Rust formatting and
strict all-target Clippy were checked locally, including crash recovery, slow
reference deadlines, terminal failures, zero W, emergency stop, deletion fencing,
signed unauthenticated discovery, labels and updater rollback. No paid jobs or
cloud lifecycle mutations were used. macOS/Linux release CI and a real hardware
worker rollout remain release gates; these results are not production evidence.

## v0.4.0 public worker and website verification — 13 October 2026

The published v0.4.0 CLI and signed worker update were exercised through the
normal public commands on an existing owner miner (SN117 UID 127). The same VM
was retained: CLI update, provider-key setup, worker-update discovery and apply,
fresh hardware-verified attestation, explicit model offers, resume and doctor.
The coordinator's shared public upgrade implementation passed 158 local tests.

Two clearly labeled operator-funded requests were submitted by clicking Generate
on app.everyframe.studio using an authorized test account:

| Path | Accepted output | Website completion time |
|---|---|---:|
| H3 Max Turbo, portrait, 5 seconds, 768p | 768×1344 H.264/AAC, 5.184 seconds | about 12.6 seconds |
| Grok Imagine Image, portrait | 720×1280 JPEG | about 15.4 seconds |

Both completed through the miner, with accepted signed receipts and verified
worker-output hashes, without direct fallback. Website delivery can remux video
or resize images after acceptance; derivative hashes are not receipt hashes.
These timings are completed-delivery observations, not first-frame latency or
an SLA. They do not validate all 27 priced contracts, all settings, or all miners.

An earlier video attempt used direct fallback and is **not** a miner pass. It
exposed obsolete owner-specific test isolation/deadline guards. Those restrictions
were removed for this owner only; other miner restrictions, TEE checks and the
daily budget guard were retained. Green readiness alone does not prove delivery.

The three website requests consumed 66 credits; the conservative provider estimate
was $0.42, not a reconciled provider invoice. No outputs were published to the
gallery, and these tests were not marked reward-eligible. No alpha payment was
verified. The miner was returned to its previous paused state and the temporary
offers were withdrawn; the pre-existing VM remained running and billable.

**Known billing issue:** a 10% bid reduced the miner serving value from $0.20 to
$0.18, but the successful video still cost the customer 30 credits rather than
27. Customer-credit pass-through remains unresolved.

## Historical validation — 30 September 2026

The sections below are dated experiment records, not current release status.

## v0.1.1 release fix

- Both v0.1.0 macOS release jobs failed during lifecycle test setup with
  `unsafe_path`; no public release assets were published. Both Linux builds passed.
- Temporary test roots now resolve OS aliases (macOS `/var` to `/private/var`)
  before creating fixtures. Production path validation is unchanged.
- Added regression tests for symlinked temporary roots and continued rejection
  of credential/profile ancestor symlinks. **48 local tests pass**, along with
  strict release Clippy and formatting checks.
- Regular CI now includes macOS Apple Silicon and Intel, using the same toolchain
  as releases. Corrected native Mac runs remain pending until these changes are
  pushed. Version metadata and release instructions now use v0.1.1, without moving
  the existing v0.1.0 tag.

## CLI installer and self-update

- Added a version-pinned shell installer and native `everycli update`, separate
  from deployed-worker updates. No Rust setup or miner login is required.
- Local suite: **46 tests passed**, including real native-binary installation
  with mocked downloads, repeat installation, checksum/download failure,
  symlink rejection, path quoting, atomic replacement, and unchanged profiles.
- Strict release Clippy, formatting, and shell syntax checks passed.
- The release workflow targets Linux x86-64/ARM64 and macOS Intel/Apple Silicon.
  Only Linux x86-64 has been tested locally; the other targets require successful
  GitHub Actions runs before publication. No release has been published here.
- Release checksums trust the official GitHub repository and HTTPS; they are not
  independent publisher signatures. See SECURITY.md for the trust boundary.

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
