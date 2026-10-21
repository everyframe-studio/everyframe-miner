# Automatic worker updates (implemented, unreleased)

This feature is not released or enabled on production miners yet.

## Offline operation

Updates must work while the miner's management computer is offline. A local
timer is therefore not the deployment architecture. An opt-in, separately
digest-pinned updater container runs inside the existing Phala VM. It has a
private persistent volume, no inbound listener and no Docker socket. The
generation container cannot mount this volume or read its management token.

Initial enrollment requires one manual signed worker update to install the
updater, fresh hardware attestation of the complete compose, and explicit
consent to transfer a workspace-scoped Phala management token, console/worker
delegates and permitted generation keys into the updater. Wallet secrets,
coldkeys and billing-only credentials must never be transferred. Use a
dedicated Phala workspace to limit the management token's reach.

The updater image stays pinned during ordinary worker updates. Updating the
updater itself is a separate reviewed/manual migration. This lets a known-good
controller recover even if the replacement generation container cannot start.

## Update and recovery contract

1. Require remote opt-in for the exact miner and VM, plus the local consent
   record. Existing miners default to manual updates.
   A capsule-bound rollout lease serializes automatic upgrades across the fleet.
   Its one-hour deadline cannot be extended by progress reports. A controller
   that loses its lease cannot continue protected mutations automatically.
2. Discover a coordinator-signed, same-VM release. Reject hardware, disk, OS,
   name, KMS or updater-image changes. Never create another VM or change bids.
3. Obtain a bounded recovery authorization for the exact previously admitted
   compose/policy/models before draining or changing the cloud workload.
4. Drain atomically; offered, starting, submitted and unknown attempts all
   block updates. Never interrupt a paid submission merely to meet a timeout.
5. Journal each cloud mutation before sending it. A lost response is observed
   through reconciliation, not retried as a fresh mutation.
6. Update the generation image with its provider keys disabled. Require fresh
   hardware attestation, restore keys, require fresh post-activation attestation,
   then restore the miner's previous accepting/paused state.
7. If admission fails after the bounded wait and the cloud mutation is known
   to have landed, request recovery from the coordinator. The recorded previous
   image must still be authorized. Recovery also requires fresh attestation
   before keys or work resume. Block the failed release from automatic retry.
8. If recovery is expired/revoked, cloud state is uncertain or rollback fails,
   remain paused and report a specific actionable failure. Never bypass TEE
   admission, recreate a VM or silently resume.

## Credentials and manual control

Manual pause/resume disables automatic updates so automation cannot undo the
operator's choice. Re-enabling an unchanged enrollment does not restart the VM.
Each encrypted credential enrollment has a coordinator-selected ID. Old capsules
cannot replace a newer enrollment or overwrite an unfinished update journal.
Changing provider keys requires explicitly enrolling the new credentials; the
updater does not silently discover keys saved on a management computer. If its
local deployment is behind an automatic update, reconnect that profile first.
The recovery controller's own image remains pinned across these changes.

Enrollment preflights active jobs, unfinished manual updates, uncertain cloud
journals and capsule size before changing policy or exporting credentials. Only
the signed configuration, minimal deployment fields and allowlisted credentials
are exported, never arbitrary local profile metadata. Replacement VMs need a new
explicit capsule; stale pending reports from a deleted VM do not follow them.

## Release gates

- Signed bootstrap template and strict two-container isolation validation.
- Encrypted opt-in provisioning and persistent journal across VM restarts.
- Remote disable and status from a fresh management device.
- Unit, lifecycle and browser tests, including signature tampering, in-flight
  jobs, uncertain cloud responses, revocation and failed rollback.
- An explicitly budget-approved disposable Phala test covering successful
  automatic update and failed-admission recovery with the laptop disconnected.
- Verify VM and storage cleanup before publishing the signed bootstrap release.

Do not advertise this as available based on unit tests alone.

## Validation on 2026-10-21

The full Rust suite, Clippy, formatting, browser regressions (including mobile),
and the coordinator suite passed. The latter included 293 tests at validation
time; this is a suite total, not a count of newly added tests.

A disposable Phala canary independently completed an upgrade inside its VM,
without a running management updater. The same VM then completed automatic
rollback after a canary-only admission failure and the real 15-minute timeout.
Fresh admission and settings restoration were checked; the original paused state
was preserved. No generation jobs or offers were created. Restoring an accepting
state and waiting for active jobs are covered by lifecycle tests, not this paid
canary.

The next normal check reported `worker_auto_update_release_blocked` without
another restart. The disposable VM was deleted and confirmed absent; temporary
local credentials were removed. The canary-only fault and opt-in were disabled,
customer routing remained enabled, and the briefly paused synthetic schedule
was restored. The test remained within its approved $1 ceiling.

That first canary controller predates final capsule-size, re-enable compose-check,
enrollment preflight, metadata filtering, bounded lease, replacement-VM and TLS
hardening. A second disposable Phala canary exercised the exact final images and
matching deployed coordinator, including unattended signed-image transition and
timed rollback. See REAL-MINER-TEST-v0.6.3.md for pins, live evidence, cleanup
status and coverage limits. The final rerun passed 290 Rust and 294 coordinator
tests, with one live hotkey probe intentionally ignored.

This is not certification of an official downloadable binary. Public distribution
still requires native release CI, signed bootstrap publication and CLI release.
Existing miners have not been opted in automatically.
