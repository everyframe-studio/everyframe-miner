# v0.6.3 publication checklist

Candidate branch: `feat/automatic-worker-updates`.
This file records preparation, not an authorization to publish or enroll miners.

## Included

- CLI and Worker-page controls for optional offline automatic updates.
- Separately pinned in-VM supervisor, explicit credential consent, private state,
  signed same-VM updates, fresh attestation and journaled recovery.
- Enrollment preflight and metadata allowlisting, capsule replay protection,
  replacement-VM fencing and a bounded fleet rollout lease.
- Scoped, fixed coordinator diagnostics; no arbitrary remote messages exposed.
- Container default-target/context fixes and clean container CI gates.
- Patched TLS dependencies, updated release/compatibility documentation.

## Coordinator deployment required

The companion coordinator changes are separate from the miner repository:

`src/store.mjs`, `src/app.mjs`, `src/miner-console.mjs`, `src/hotkey-auth.mjs`,
`src/public-onboarding.mjs`, `src/public-worker-upgrade.mjs`,
`src/worker-update-recovery.mjs`, `src/worker-update-lock.mjs`,
`src/updater-compose.mjs`, and the explicit routes in `deploy/Caddyfile`.

Recovery support was exercised by the earlier live canary. The latest bounded
lease and replacement-VM/report fencing changes still require deployment. Use
exact file hashes and a backup; do not copy the whole dirty coordinator tree.
Read-only comparison on 21 October confirmed the other seven listed runtime
files already match production; only `miner-console.mjs` and
`worker-update-lock.mjs` differ.

## Verified locally

- 290 Rust tests passed; one live hotkey probe deliberately ignored.
- 294 coordinator tests passed with the scoped package test command.
- Formatting, strict all-target Clippy and installer shell syntax passed.
- Desktop/mobile browser regressions passed against the optimized Linux CLI.
- Optimized CLI, worker and supervisor native builds passed.
- Clean digest-pinned worker and supervisor container builds passed. The final
  supervisor stayed running in `not_enrolled` state with networking disabled,
  read-only root filesystem and dropped capabilities. Both images contain license
  notices. The disposable local smoke container was removed after verification.
- OSV queried all 202 locked registry packages after TLS dependency patches:
  no known advisories returned. This is not a guarantee of vulnerability-free code.
- Release workflow YAML parsed and publication depends on all native builds plus
  the new container build/smoke gate. Credential-pattern scan found no matches
  in candidate source files; the Docker context remains an explicit allowlist.

Local image IDs (not registry/admission digests):

- Worker: `sha256:79ea73596784fa2484a2099980ef0cd5e1a03e65f0ca4bac2df3d39f27c0d0cb`
- Supervisor: `sha256:08faf0acb993541647c1a5ab6941b5d645f10e2f7e5d16a68f89d927b454ae1e`

Native local binaries are source builds for validation. Official downloadable
CLI artifacts are rebuilt by CI with `EVERYCLI_RELEASE_BUILD=1`; do not upload
the local binaries as official release assets.

## Publication order and remaining gates

1. Review and merge the miner branch through a PR; run native Linux x86-64/ARM64
   and macOS Intel/Apple Silicon CI. Local Linux results do not certify these
   other platforms. The tag workflow cannot publish until all native jobs and
   the container build/smoke job succeed.
2. Deploy the matching coordinator hardening without interrupting active jobs.
3. Publish immutable worker and supervisor images only after approval; record
   both digests. The supervisor pin stays fixed in subsequent worker updates.
4. Validate the exact final supervisor/bootstrap in a budget-approved disposable
   Phala canary, including fresh attestation and recovery. Earlier live proofs
   predate final hardening/TLS patches and do not certify these exact artifacts.
5. Confirm VM/storage deletion, publish the signed bootstrap, publish CLI v0.6.3,
   verify anonymous downloads/checksums, then update public website docs.

No real miners are opted in by these steps. Enrollment is a separate, explicit
operator choice; CLI updates do not install or enable the supervisor.

## Scope limitations

No additional paid generations, customer-routing changes, validator changes,
alpha payments, registry pushes, tags or production enrollment are part of this
review. The prior canary VM was confirmed deleted and local credentials removed.
The live coordinator was checked read-only and had zero automatic-update opt-ins.
