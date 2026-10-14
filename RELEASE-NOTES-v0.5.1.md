# everycli v0.5.1

Includes the complete v0.5.0 CLI and worker improvements, plus these setup fixes:

- Test API keys with free, read-only provider requests before saving in the UI.
  Rejected replacements leave existing keys untouched; restricted or unreachable
  checks are explicitly marked unverified, never falsely reported as accepted.
- Keep setup on the selected step after saving keys. Preserve drafts during
  background refresh, shorten long worker names, and allow local renaming.
- Remove expiry clutter from the dashboard without weakening authorization checks.
- List, delete, and replace a matching worker created on another computer in both
  the UI and CLI. Deletion has a durable journal and requires confirmed cloud
  removal; failed status requests never count as successful deletion.
- Replacement preflights coordinator support before spending on hosting. The
  coordinator uses exact previous-VM binding, drains work, invalidates old sessions,
  and requires fresh attestation. Existing offers and provider scopes are preserved.

Update with `everycli update`, then open `everycli ui`.
This release does not require replacing a healthy deployed worker: its runtime is
unchanged from v0.5.0. Key queries do not generate media. Deletion is irreversible;
stopping compute alone does not remove billable storage or provider subscriptions.
