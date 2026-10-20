# Release checklist

## Publishing prebuilt CLI downloads

`.github/workflows/release.yml` builds and tests native Linux x86-64/ARM64 and
macOS Intel/Apple Silicon binaries on matching GitHub-hosted runners. It uses
Rust 1.97.1, validates the tag against Cargo.toml, and publishes only after every
platform passes. This is separate from worker image admission.

After reviewing and committing the release changes, the maintainer can publish
the corrected version (these commands create an external release when pushed):

```sh
git tag v0.5.0
git push origin v0.5.0
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
and `.sig` files, a version-pinned `everycli-installer.sh` with its checksum, and project
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

## Release signing (v0.6.0+)

Every binary asset is published with a detached Ed25519 signature,
`<asset>.sig`, next to `<asset>` and `<asset>.sha256`. The `.sig` file is one line:
the standard (padded) base64 encoding of the 64-byte Ed25519 signature, followed
by a newline. The signed message is exactly these UTF-8 bytes:

```text
everycli-release-v1\n<tag>\n<asset-name>\n<sha256-hex-lowercase>\n
```

For example, `everycli-release-v1`, newline, `v0.6.1`, newline,
`everycli-x86_64-unknown-linux-gnu`, newline, the 64 lowercase hex digits of the
binary's SHA-256, newline. Binding the tag stops an older signed binary from
being replayed as a newer version; binding the asset name stops one platform's
binary from being served for another. The verifier always hashes the bytes it
downloaded itself; it never signs or trusts the published `.sha256` text.

Release public key (raw 32 bytes, hex), embedded in `src/update.rs`
(`RELEASE_SIGNING_PUBLIC_KEY_HEX`) and, as an SPKI PEM, in the installer:

```text
a81643f1026fb20e85399c23fc804fee82bc2e58e570efeaf30a6571eab04149
```

The private key is the GitHub Actions secret `EVERYCLI_RELEASE_SIGNING_KEY`: an
Ed25519 PKCS#8 PEM, as produced by `openssl genpkey -algorithm ed25519`. Store it
as a secret of the `release` environment (not a repository-wide secret), and keep
an offline backup. The publish job writes it to a private temporary file
(umask 077), refuses to sign if its public key differs from the embedded constant,
signs each of the four binaries with `openssl pkeyutl -sign -rawin`, verifies each
signature, and deletes the key file. It never echoes the key.

The publish job runs in the GitHub `release` environment. Configure required
reviewers on that environment so that pushing a tag only builds and tests;
signing and publishing wait for an explicit approval. A release is marked
"latest" only when its version is greater than or equal to the highest existing
published release, so a hotfix tag on an older line never becomes the version
the installer URL and updater follow.

Verifying a release by hand (OpenSSL 1.1.1+; macOS LibreSSL cannot):

```sh
tag=v0.6.1 asset=everycli-x86_64-unknown-linux-gnu
printf '%s\n' '-----BEGIN PUBLIC KEY-----' \
  'MCowBQYDK2VwAyEAqBZD8QJvsg6FOZwj/IBP7oK8LljlcO/q8wplceqwQUk=' \
  '-----END PUBLIC KEY-----' > everycli-release.pem
sha=$(sha256sum "$asset" | awk '{print $1}')
printf 'everycli-release-v1\n%s\n%s\n%s\n' "$tag" "$asset" "$sha" > message
openssl base64 -d -A < "$asset.sig" > signature.bin
openssl pkeyutl -verify -pubin -inkey everycli-release.pem -rawin -in message -sigfile signature.bin
```

Transition: v0.5.x updaters check only the `.sha256`, so they still install
v0.6.0. v0.6.0 and later refuse any update (0.6.1+) whose `.sig` is missing or
invalid (`update_signature_missing` / `update_signature_invalid`), and the
v0.6.0+ installer refuses to install without a valid signature. The v0.6.0
release must therefore already be published by this workflow with `.sig` files,
which requires the secret before the tag is pushed.

Key rotation: the embedded key can only be changed by a release signed with the
current key (the new CLI then trusts the new key from its next update). If the
key is lost, existing v0.6.x installs cannot auto-update; users must reinstall
with a new installer. If it leaks, publish a rotation release immediately and
treat the leak as a security incident.

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
   checksums, and Ed25519 `.sig` files (see "Release signing" above).
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
