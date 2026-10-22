# everycli v0.7.0 — Offline worker updates and Kling direct API

This minor-version release includes the offline updater implementation and a
new direct provider adapter, alongside CLI/UI controls and TLS updates.

**CLI release, not automatic production enablement:** the signed offline-updater
bootstrap and direct Kling mining are not yet publicly enabled. Existing miners
keep their current worker and manual update path. Kling still requires reviewed
pricing and a paid, attested generation test before routing can be enabled.

## What's new

- Kling developer API key setup in CLI and UI, with a free read-only key check.
- Candidate Kling 3.0 direct text-to-video adapter: 3–15 seconds, 720p landscape,
  silent single shot. Separate from Fal-hosted Kling. Provider errors are safely
  categorized, task IDs and output hosts checked, and uncertain paid submissions
  never blindly retried. Live routing is pending pricing and attested validation.
- Explicit unsupported USD balance diagnostics for Kling; delayed resource-pack
  quantities are not presented as live cash balances.

- Opt-in automatic worker updates that run inside your Phala VM, even when your
  management computer is offline. Manage them from the Worker page or with
  `everycli worker-auto-update`.
- Signed updates wait for jobs, require fresh hardware attestation and preserve
  your previous accepting/paused state. Failed admission can restore the signed
  previous worker; ambiguous cloud operations stay paused for reconciliation.
- A separately pinned supervisor retains recovery state across restarts. Failed
  releases are blocked from repeated automatic attempts.
- Shows safe, specific coordinator update/recovery errors instead of reducing
  every rejection to a generic HTTP conflict. A busy rollout waits its turn.
- Fixes the container build context to include the embedded provider availability
  registry, while keeping private state and credentials excluded. Ordinary Docker
  builds still default to the generation worker, not the supervisor.

## Security dependencies

Updates the locked TLS dependencies to rustls 0.23.46, rustls-webpki 0.103.16 and
webpki-roots 1.0.10. These address published advisories, including the distrusted
intermediate certificate checks in
[RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html) and trust
store change in [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381.html).
Certificate verification remains mandatory; no insecure TLS fallback is added.

## Before enabling

Your CLI update does **not** turn this on. Once the matching approved bootstrap
is published, install it using `everycli worker-update`, then choose
`everycli worker-auto-update --enable`. Until then, keep the manual update path;
do not redeploy or bypass attestation to enable these controls.
Enrollment requires supported non-expiring delegates and explicit consent to
store your Phala management token and permitted provider keys in the isolated
supervisor. Use a dedicated Phala workspace. Wallet secrets are never transferred.

`everycli worker-auto-update --disable` prevents further automatic stages; it
does not stop VM/storage charges or erase an unfinished recovery journal. Manual
pause/resume disables automation. Changed provider credentials require explicit
re-enrollment. The supervisor itself is not automatically replaced.

Availability depends on the matching signed bootstrap and coordinator release.
Existing miners remain opted out. See AUTOMATIC-WORKER-UPDATES.md and RELEASING.md
for validation evidence and publication gates.
