# everycli v0.6.3 — Offline worker updates

## What's new

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

Your CLI update does **not** turn this on. Install the approved bootstrap using
`everycli worker-update`, then choose `everycli worker-auto-update --enable`.
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
