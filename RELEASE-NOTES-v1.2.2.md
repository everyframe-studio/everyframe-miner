# everycli v1.2.2

Update workers with the dashboard open and recover unfinished updates more clearly.

- Automatic-update status polling no longer takes the profile mutation lock. Worker-update checks and changes are serialized to avoid competing operations.
- Saved manual updates retain **Continue worker update** and **Cancel worker update** controls even after a failed status check.
- The dashboard directs you to finish a saved update before resuming work or enabling automatic worker updates.
- Actions blocked before submission show **Not started**, while the original operation remains available for recovery.

## Upgrade

```sh
everycli update
```

Close and reopen `everycli ui` to load the fix. These dashboard changes require no new worker deployment.

If a manual update is unfinished, choose **Worker → Continue worker update**, or run `everycli worker-update` in the same profile. The saved operation checks the existing VM, requires fresh attestation, and restores its previous accepting/paused state. Do not delete profile files or deploy a replacement to clear a pending update.
