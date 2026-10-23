# v0.7.2 exact-artifact validation — 23 October 2026

This document records artifact pins, test scope and local validation. Publication
requires the completed live recovery and cleanup evidence; source availability
alone does not mean the signed bootstrap is live. The release's
`canary-validation.json` attachment records the final cloud outcomes.

Runtime source: `2ac8f64376a0d5aa01c72e977b59e4cbc4d117ba`.
Documentation-only changes after that commit do not change these images.

Registry: `ghcr.io/everyframebuilder/everyframe-subnet-worker`.

| Artifact | Immutable manifest digest |
| --- | --- |
| Worker | `sha256:6e88fa3971efaf3461083d4fa3585b350771175033d4b2597f5a013e81cd3ecf` |
| Offline supervisor | `sha256:d8220320a797abac89fbc2a3b8941db3ecd81c094a98448c891dc6cfd1a76db4` |

Both manifests are anonymously readable and their config digests match the
reviewed local builds. The supervisor is idle without enrollment in a read-only,
network-isolated container. The Phala canary uses approved `dstack-0.5.12` on
`tdx.small`, at a quoted $0.058/hour, with a $0.80 reservation and 90-minute cleanup
guard within the approved validation budget. No real generation key is supplied;
database guards forbid assigning jobs or creating offers for the test identity.

Local checks: 299 Rust tests passed, two opt-in live probes skipped; formatting,
strict Clippy and desktop/mobile browser regressions passed. Coordinator tests:
317 passed, no skips or failures, including registry parity and capability-only
reports that cannot authorize or price candidate models. Real hardware admission and initial
activation passed, followed by an unattended signed-image transition and fresh
post-activation admission. The original paused state was preserved, on the same
VM, with no management-side update controller running. A manual reset between
test cycles lost its cloud response; observed cloud state and fresh admission
confirmed it without repeating the PATCH. That is harness reconciliation
evidence, not a claim that the CLI handled that particular lost response.

## Required live recovery and cleanup gates

- Deny successor admission only for this disposable identity, wait through the
  shipping 15-minute timeout, and verify autonomous signed-predecessor rollback.
- Require fresh admission before and after reactivation, preserve the paused
  state, consume the recovery record and block automatic retry of the bad target.
- Verify remote opt-out while the same VM remains running, then delete the VM
  and its storage, confirm cloud absence and remove temporary credentials.
- Confirm zero generation attempts/offers, no customer enrollment, no leftover
  canary fault triggers and mandatory TEE/customer routing still enabled.

The test transitions from the final worker to an existing signed worker and
recovers to the final worker, keeping the exact final supervisor pinned. It
verifies signed-image transition and recovery, not semantic-version-forward
ordering. Active-job draining, accepting-state restoration, tampering and
uncertain-operation state-machine cases have automated regression coverage;
this live canary intentionally stays paused without paid generations.
