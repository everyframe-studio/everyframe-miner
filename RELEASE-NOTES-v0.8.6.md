## Worker updates and setup progress

- A dedicated **Worker software** panel shows the installed release and update status, with controls to check, install, continue or cancel a worker update.
- Setup reaches **8/8** once accepting work is confirmed. Warnings about individual models remain visible separately.
- Continue a saved update directly from the UI or CLI while preserving its reconciled signed release.
- Recover updates where Phala reordered the allowed environment keys. Recovery requires a fresh signed configuration that matches the same VM and changes only that order; saved keys and offers are preserved.
- Clearer messages distinguish a stale provider catalog, an offline worker and a provider-key problem.

## Update

```sh
everycli update
```

Restart `everycli ui` to load the new dashboard. These CLI fixes do not require a new generation-worker image.

If an earlier worker update stopped with `unapproved_running_compose`, run `everycli reconcile`, then `everycli worker-update` to continue the saved update. Keep the existing profile and VM.
