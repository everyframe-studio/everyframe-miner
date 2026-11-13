# everycli v1.2.2

Update workers with the dashboard open, recover unfinished updates more clearly, and serve Veo 3.1 jobs with negative prompts.

- Automatic-update status polling no longer takes the profile mutation lock. Worker-update checks and changes are serialized to avoid competing operations.
- Saved manual updates retain **Continue worker update** and **Cancel worker update** controls even after a failed status check.
- The dashboard directs you to finish a saved update before resuming work or enabling automatic worker updates.
- Actions blocked before submission show **Not started**, while the original operation remains available for recovery.
- Available models no longer show an account warning merely because no per-model access check is available. Reported access and readiness problems remain visible.
- Adds `studio/openrouter/google/veo-3.1-negative` and `studio/fal/fal-ai/veo3.1-negative`, preserving negative-prompt text alongside 4, 6 or 8-second video, 720p/1080p settings, audio, and first/last frames.

## Upgrade

```sh
everycli update
```

Close and reopen `everycli ui` to load the fix. These dashboard changes require no new worker deployment.

If a manual update is unfinished, choose **Worker → Continue worker update**, or run `everycli worker-update` in the same profile. The saved operation checks the existing VM, requires fresh attestation, and restores its previous accepting/paused state. Do not delete profile files or deploy a replacement to clear a pending update.

To serve the new Veo negative-prompt contracts, install the compatible worker with `everycli worker-update`, then review and explicitly offer the new contracts on the Models page. An older worker cannot gain this execution capability through a catalog refresh alone. See [Veo negative prompts](docs/veo-negative-prompts.md).
