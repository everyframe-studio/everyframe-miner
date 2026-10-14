# everycli v0.5.4

Recover a stale coordinator binding after a VM was deleted directly in Phala,
without discarding the miner's identity or asking an operator to edit its database.

## Recovery

For a public hotkey profile with no local VM record, run `everycli reconnect` or
use **Worker → Reconnect profile**. The CLI checks the exact cloud app and workspace
catalog, then asks for explicit confirmation before signing a narrow retirement
request. A cloud 404 alone is not proof of deletion: verify the workspace and only
approve if you deleted the old VM. Fresh heartbeats and active/uncertain jobs block
server-side recovery. Old delegates are revoked; a replacement needs new attestation.

No VM is created, deleted or updated, no storage is deleted, and no work is resumed
by recovery. API keys and labels stay local. Existing local deployment journals and
uncertain cloud operations keep their existing reconciliation safeguards.

## Deployment and OS compatibility

- A fresh profile cannot create a paid second VM while another app is still bound.
- An unbound deployment can refresh a newer reviewed server release before any
  paid submission. Signed identity and exact OS hash validation remain mandatory.
- Production `dstack` versions are no longer hardcoded to `0.5.9`. Development
  and GPU image names remain disallowed; the server must review and approve the OS.
- Catalog outages, retired OS images, changed bindings and unfinished jobs have
  specific recovery messages instead of a generic setup failure.

Run `everycli update`, then reopen the UI. This release changes the local CLI/UI,
not the worker image or existing offers. Automatic CLI updates, setup attachment
and automatic authorization refresh do not silently retire a remote VM.
