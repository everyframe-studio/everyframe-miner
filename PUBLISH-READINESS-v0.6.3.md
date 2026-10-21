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

The latest bounded lease and replacement-VM/report fencing changes were deployed
at a natural idle window on 21 October. Only `miner-console.mjs` and
`worker-update-lock.mjs` needed replacement; exact-source guards and a backup were
used. Read-only comparison confirms all nine listed runtime files match tested
source. Do not copy the whole dirty coordinator tree for later deployments.

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
   Completed for the final canary; mandatory attestation/routing remain enabled.
3. Publish immutable worker and supervisor images only after approval; record
   both digests. The supervisor pin stays fixed in subsequent worker updates.
   Prefer promoting the exact tested manifests. If a rebuild changes either
   digest, repeat exact-artifact admission validation before signing that digest.
4. Validate the exact final supervisor/bootstrap in a budget-approved disposable
   Phala canary, including fresh attestation and recovery. These checks passed on
   final artifacts; see REAL-MINER-TEST-v0.6.3.md for exact pins, scope limits and
   final cleanup status. Earlier canary artifacts are not the final release pins.
5. Confirm VM/storage deletion, publish the signed bootstrap, publish CLI v0.6.3,
   verify anonymous downloads/checksums, then update public website docs.
   Disposable VM/storage deletion is confirmed. Signed bootstrap, official CLI
   publication, download verification and public website docs remain pending.

No real miners are opted in by these steps. Enrollment is a separate, explicit
operator choice; CLI updates do not install or enable the supervisor.

## Scope limitations

The final canary used candidate-only registry tags and its own temporary opt-in.
No paid generations, customer-routing changes, validator changes, alpha payments,
official CLI tags or enrollment of existing miners were performed. See the live
test record for disposal status. Native release CI and official distribution
remain separate gates; this checklist does not authorize publication.
