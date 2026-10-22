# Release checklist

## Publishing prebuilt CLI downloads

`.github/workflows/release.yml` builds and tests native Linux x86-64/ARM64 and
macOS Intel/Apple Silicon binaries on matching GitHub-hosted runners. It uses
Rust 1.97.1, validates the tag against Cargo.toml, and publishes only after every
platform passes. This is separate from worker image admission.
The release also gates on clean worker/supervisor Docker builds and a network-
isolated idle supervisor smoke test. Container builds use the reviewed pinned
Rust 1.98.0 image; native CLI builds use Rust 1.97.1. This job neither pushes images
nor substitutes for Phala attestation validation.

After reviewing and committing the release changes, the maintainer can publish
the corrected version (these commands create an external release when pushed):

```sh
git tag v0.7.1
git push origin v0.7.1
```

For subsequent releases, bump Cargo.toml and Cargo.lock first, then tag the
matching `vMAJOR.MINOR.PATCH`. Prerelease tags are not supported by this workflow.
Protect release tags and restrict who can push them. If a publication fails after
draft creation, inspect and recover that draft manually; the workflow refuses to
overwrite existing releases. Do not move a published tag or replace its assets.

v0.5.0 adds the local browser setup and management UI (`everycli ui`), lifecycle
reconciliation controls, truthful readiness, native Vivix W-credit display,
emergency stop, deletion, labels/logs and safer CLI updates/locks. Browser tests
run against an isolated loopback server with mocked cloud command results.
Node/Playwright are development-test dependencies only; miners install one native
binary. They do not need Node, Python or Rust.

Non-expiring v2 delegates are opt-in through the verified signed deployment's
`hotkeyDelegationVersion: 2`. Do not enable it until both coordinator and worker
support v2. Existing deployments keep compatible v1 creation/verification.
The public catalog endpoint is also coordinator-dependent; its absence produces
an actionable error, not fabricated pricing. Publishing this CLI does not deploy
coordinator changes or update/admit any worker image.

v0.3.6 adds `everycli health` and default human-readable `status` tables. Both
commands support `--json`. Health is read-only: all green means the current
readiness checks passed, not that provider balances or future generations are
guaranteed. Historical accepted jobs are shown separately. This CLI release does
not change a deployed worker, release provider keys or resume paid work.

v0.2.4 added Vivix API key management and the direct W1 candidate adapter. The CLI
release does not update running workers or enable coordinator routing. W1 still
requires matching coordinator pricing/contracts, a signed deployment permitting
`VIVIX_API_KEY`, and an admitted worker image with canary verification. Do not
describe publishing the CLI as a completed W1 production rollout.

The first `v0.1.0` attempt did not publish: both macOS jobs failed because test
fixtures used a symlinked OS temporary directory. `v0.1.1` resolves the temporary
root inside the test harness while preserving production symlink restrictions.
Keep the old tag for traceability. Re-running its job would run the old tests;
commit and push the fix, then publish the new matching version tag instead.

Release assets include four `everycli-<target>` binaries, individual `.sha256`
files, a version-pinned `everycli-installer.sh` with its checksum, and project
license notices. The installer URL serves the latest published release; local
source changes are not distributed until a new release is published. No GitHub token or miner login is needed to install from a
public repository. Private repositories are not supported by the public installer.

The v0.1.2 attempt did not publish because macOS requires mutable null pointers
for the terminal-input test's `openpty` arguments. v0.1.3 fixes that portable test
call without changing terminal-input behavior. Preserve the failed tag.

For hotkey authentication, publish the v0.1.3 CLI and a separately reviewed worker
image. A v2 deployment configuration replaces `tokenHash` with `authMode: hotkey-v1`
and `keyVersion`, and replaces `MINER_TOKEN` with `MINER_AUTH` in both the compose
environment and `allowed_envs`. Configure the coordinator's approved hotkey binding
and exact new image measurements before activation. Do not remove an old worker's
token while it still runs a token-only image. Binding migration must be drained
and idle; it invalidates the old token and sessions. It is not performed by
`everycli update` or by publishing a GitHub release.

## CLI release verification (v0.6.0)

CLI release signing is postponed. The installer and updater verify SHA-256
checksums over restricted HTTPS and use locked, atomic replacement. They do not
fetch detached signatures, use OpenSSL or need a release signing secret.
Checksums detect corruption and mismatched assets; they are not independent
publisher authentication. Protect the GitHub repository, workflow and release tags.

The publish job retains the `release` environment; configure required reviewers
for approval. Older-line hotfixes do not become the latest release. Reintroducing
publisher signing requires a separately reviewed trust/bootstrap/rotation design
and old-client plus fresh-install tests, not a runtime bypass switch.

This postponement does not disable signed deployment configurations, hardware TEE
attestation, session proofs, input/output hashes or signed job receipts.

`cargo install` builds are never replaced by `everycli update` or auto-update:
when cargo's install records (`.crates.toml` / `.crates2.json` in the install
root) list `everycli`, the update stops with `cargo_install_update_disabled`
and tells the user to rerun `cargo install`. Binaries placed in `~/.cargo/bin`
by the official installer are not in those records and keep updating.

## Worker and distribution review

1. Include the Apache-2.0 `LICENSE` and applicable third-party notices in release
   distributions, and publish a verified private security contact.
2. Run formatting, strict Clippy, tests, and a release build with `--locked`.
   Audit Rust dependencies and preserve applicable dependency notices.
3. For CLI distribution, build for each supported OS/architecture, publish binary
   checksums (see "CLI release verification" above).
4. For worker distribution, review `config/release.json`, coordinator/chain pins,
   the registry, and source changes. Public trust pins are not credentials;
   changing them requires a separately reviewed worker release.
5. Build using reviewed **digest-pinned** Rust and runtime images with compatible
   libc versions. No credentials, invitation, wallet, or live profile may enter
   the build context. The `.dockerignore` uses an explicit allowlist.

```sh
docker build --target worker \
  --build-arg RUST_IMAGE='rust@sha256:REVIEWED_BUILDER_DIGEST' \
  --build-arg RUNTIME_IMAGE='debian@sha256:REVIEWED_RUNTIME_DIGEST' \
  -t everyframe-miner-rust:reviewed .
```

Use `--target cli` for a standalone CLI container; the default target is `worker`.
Use `--target updater` to build the separate offline supervisor. Never use its
digest as the generation worker digest. Both artifacts must be built from the
reviewed source and recorded separately.
Final images contain only their
native executable and the chosen base runtime—no Node.js or Python is added.

6. Push only when explicitly approved; record the immutable image digest.
7. Stage the digest-pinned image and reviewed OS/KMS policy in the coordinator's
   public release configuration. Self-service enrollment derives each miner's
   exact compose/app binding from that configuration. Never whitelist all images,
   trust unreviewed measurements from a new VM, or bypass admission.
8. Test in a separately approved canary with a stated total budget. Verify real
   dstack quotes, admission, activation, job completion, provider accounting,
   recovery, and shutdown before production promotion.

Publishing a CLI does not change the worker selected by an existing signed
deployment. Worker admission and automatic-update opt-in are separate operations.

## v0.7.0 offline updater publication gates

1. Merge the reviewed miner branch and the matching coordinator changes. Run
   native CI for all four release platforms; do not infer macOS/ARM success from
   Linux tests. No tag should be pushed before the release review is complete.
2. Deploy the tested coordinator recovery, bounded fleet lease, replacement-VM
   fencing and explicit proxy routes before exposing the bootstrap package.
3. Build and record separate immutable worker and supervisor image digests. The
   supervisor's digest must remain unchanged during ordinary automatic updates.
4. Validate the exact final supervisor artifact and signed bootstrap against the
   approved Phala OS/KMS policy in a separately budget-approved canary. The prior
   live upgrade/rollback experiment verified the recorded v0.6.3 artifacts, not
   the new v0.7.0 Kling build. Delete the new canary VM/storage and retain proof.
5. Publish the signed two-container bootstrap, then publish the CLI downloads.
   Recheck anonymous installer access, checksums, discovery and opt-in messaging.
   Keep existing miners opted out; no bulk enrollment or provider jobs are part
   of publishing. Manual bootstrap/enrollment requires the miner's consent.

See AUTOMATIC-WORKER-UPDATES.md for trust boundaries and validation limitations.
