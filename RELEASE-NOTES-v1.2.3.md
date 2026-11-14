# Everycli v1.2.3 — Automatic worker-update recovery

- Restore the original accepting/paused state and prior automatic-update policy if enrollment fails before a VM restart.
- Check signed server status after a lost enable/disable response, so a committed change can be reported correctly without repeating it.
- Keep interrupted enrollment recoverable across CLI restarts. An uncertain VM restart is never repeated automatically.
- Show specific coordinator errors in the CLI and dashboard. The Worker page offers **Reconcile enrollment** when needed.

Run `everycli update`, then reopen `everycli ui` if it was running. These fixes do not require `everycli worker-update`.

For a worker left draining by an older CLI failure, use `everycli resume --yes` first if you want it accepting work, then `everycli worker-auto-update --enable --yes`. A manual resume disables automatic updates, so keep that order. For a saved recovery in the new CLI, run `everycli reconcile` first.
