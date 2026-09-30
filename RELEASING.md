# Release checklist

## Publishing prebuilt CLI downloads

`.github/workflows/release.yml` builds and tests native Linux x86-64/ARM64 and
macOS Intel/Apple Silicon binaries on matching GitHub-hosted runners. It uses
Rust 1.97.1, validates the tag against Cargo.toml, and publishes only after every
platform passes. This is separate from worker image admission.

After reviewing and committing the release changes, the maintainer can publish
the corrected version (these commands create an external release when pushed):

```sh
git tag v0.1.3
git push origin v0.1.3
```

For subsequent releases, bump Cargo.toml and Cargo.lock first, then tag the
matching `vMAJOR.MINOR.PATCH`. Prerelease tags are not supported by this workflow.
Protect release tags and restrict who can push them. If a publication fails after
draft creation, inspect and recover that draft manually; the workflow refuses to
overwrite existing releases. Do not move a published tag or replace its assets.

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

Checksums verify transfer integrity, not an independent publisher signature:
the installer/updater trust this GitHub repository and HTTPS. Follow the signing
and third-party notice review below before calling this an audited distribution.

## Worker and distribution review

1. Include the Apache-2.0 `LICENSE` and applicable third-party notices in release
   distributions, and publish a verified private security contact.
2. Run formatting, strict Clippy, tests, and a release build with `--locked`.
   Audit Rust dependencies and preserve applicable dependency notices.
3. For CLI distribution, build for each supported OS/architecture, publish binary
   checksums, and sign artifacts through the owner's existing release process.
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

The Rust source does not automatically replace the current Node worker or Python
CLI. An existing signed invitation for a Node image still deploys that Node image;
using the Rust CLI alone does not change the worker selected by the invitation.
