# v0.6.3 final-build Phala test — 21 October 2026

Runtime source: `86f8ab043aaf743f440b41d1d427295be9b4beac`.
This is a disposable operator canary, not a customer or reward-eligible miner.

## Exact artifacts

Registry: `ghcr.io/everyframebuilder/everyframe-subnet-worker`.

| Artifact | Immutable manifest digest |
| --- | --- |
| Final worker | `sha256:3c2c36b19dbb6998e729883e5f2f09d684e244063520514138066e6c6ec7a101` |
| Final supervisor | `sha256:3f8eb666adb6d35c52968b2fdb2a86b5528d117e032c3a9ea56ff8c33004564d` |
| Existing public worker used as transition target | `sha256:c3ef40d852d2253cca7b4de1c8e460de6b84f5414c05366cc1cc238e1d47bb0e` |

Both candidate manifests were anonymously readable and their config digests
matched the final locally reviewed images. The VM used `tdx.small` and the
approved `dstack-0.5.12` OS; the supervisor image stayed fixed throughout.
Promote these exact manifests, or repeat exact-artifact admission checks if a
rebuild produces different digests. CLI release binaries are built separately.

## Real-cloud checks

- Final worker/supervisor bootstrap passed hardware attestation.
- The real CLI activated the test configuration and enrolled the supervisor.
  Only a dummy, unusable generation key was present. The Phala management token
  was transferred only through the explicitly consented encrypted enrollment.
- With no local update controller running, the in-VM supervisor discovered a
  signed target and changed the worker image on the same VM. Fresh attestation
  before and after activation passed, and the original paused state returned.
- A manual reset between test cycles lost its cloud response. The harness
  reconciled the observed image and fresh admission without repeating that PATCH.
  This is harness reconciliation evidence, not a claim that the CLI handled
  that particular lost response.
- A canary-only session-admission fault prevented the next image from being
  admitted. The shipping 15-minute timeout was not shortened. The supervisor
  restored the signed predecessor autonomously, obtained fresh attestation,
  completed reactivation/readmission, and preserved the paused state. One
  rollback admission was recorded and the recovery record was consumed.
- Zero generation attempts and zero model offers throughout; database guards
  explicitly prevented assigning work or offers to this identity.
- The next normal discovery check returned `worker_auto_update_release_blocked`
  with no pending update; the restored worker remained running and admitted.
- The real CLI disabled the remote updater. Remote policy confirmed disabled
  while the same VM remained running on the restored image.
- The disposable VM and attached storage were deleted. A subsequent complete
  cloud listing confirmed its absence; local temporary credentials were removed.
  The cleanup timer was stopped only afterward. Fault/guard triggers were removed,
  the test miner disabled, and zero automatic opt-ins remained.

The matching coordinator hardening was installed only at an idle window. The
synthetic scheduler was briefly paused and restored; customer routing stayed
enabled. Public release `mainnet117-public-v061-os0512-20261021` was not changed.
No existing miner was enrolled or updated by this test.

## Scope and remaining release gates

To avoid promoting an untested public release, this canary started on the final
worker and transitioned to the existing signed public worker, then recovered to
the final worker. It verifies signed-image transition/recovery and final-artifact
admission, not semantic-version-forward ordering or public v0.6.3 publication.

Accepting-state restoration, active-job draining, lost-response state-machine
cases, tampering, capsule fencing and lease expiration have automated regression
coverage; this live canary deliberately remained paused without jobs. The final
suites were rerun: 290 Rust tests passed, one live hotkey probe intentionally
ignored, and all 294 coordinator tests passed.

No new public hotkey registration, paid provider generation, alpha rewards,
native macOS/ARM release CI or official installer download was tested in this
canary. Cross-platform release CI, signed bootstrap publication and public
download/checksum verification remain publication gates.

Approved cumulative VM budget: $1, with $0.20 reserved for the earlier canary and
$0.80 available for this run. Quoted compute: $0.058/hour; a 90-minute cleanup
guard was installed before provisioning. Confirmed-absence time was 23:14:57 UTC,
55.94 minutes after the recorded start. Compute estimate: $0.0541 for this run,
or $0.1102 together with the earlier canary, before storage. These are estimates
from duration and the quoted rate, not independently attributed provider invoices.
The conservative reservations remained below the original $1 approval.
