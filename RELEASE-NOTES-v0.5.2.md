# everycli v0.5.2

- Dashboard identity: miner UID from the last finalized registration check,
  shortened hotkey, full-address tooltip/copy and a read-only Refresh UID action.
  UID 0 is valid; missing or mismatched lookups are never presented as known UIDs.
- `everycli reconnect` and **Worker → Reconnect profile** recover superseded
  local miner authorization with proof from the same hotkey and confirmation.
  Exact signed identity/workload checks preserve API keys, labels and deployment
  history. A durable owner-only journal permits interrupted local commits to
  finish without repeating remote requests. Uncertain cloud operations remain
  blocked; no VM is created, deleted, updated or started by reconnect.
- Coordinator HTTP 401 now identifies rejected miner authorization separately
  from Phala credential errors, with clock-check and reconnect guidance.
- Initialization retains the verified registration receipt and signer path;
  refreshing registration by public address preserves the saved signer.
- UI startup now reaches the existing opt-out, six-hour automatic CLI-update
  check. Reopen the UI after an update; running processes and workers are not
  silently restarted. Source builds are never silently replaced.

Upgrade with `everycli update`, then restart `everycli ui`. To recover a saved
profile, run `everycli reconnect --wallet YOUR_WALLET --hotkey default`, approve,
then run `everycli doctor`. The saved signer can be used by omitting wallet flags;
`--hotkey-file` is also supported. Never share hotkey files or API keys.

This is a CLI/UI release. The deployed worker runtime remains v0.5.0; do not
replace a healthy VM just to upgrade the CLI. A reconnect does not install new
authorization into a running worker: review `everycli apply-api-keys` separately
when needed. For a deleted VM, reconnect preserves its deletion record so a
separately approved `everycli deploy --max-hourly-usd LIMIT` can replace it safely.
