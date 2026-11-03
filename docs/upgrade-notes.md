# CLI upgrade history and required steps

`everycli update --check` previews every documented release in
`(installed version, latest stable version]`. `everycli update` includes the
same history and follow-up actions in its result after installation. Versions
are ordered numerically; releases that were never published are not invented.

Each entry states whether a new worker image is required, conditional on the
models/providers being used, recommended, or unnecessary. Steps such as legacy
authorization migration, continuing an interrupted update, reviewing new offers,
and reopening an existing dashboard are listed with their conditions. No step
is executed by the CLI updater.

Release publishers maintain `config/upgrade-notes.json`. The publish workflow
requires an entry for its tag and attaches `everycli-upgrade-notes.json` plus
its SHA-256 checksum. The updater checks that checksum, validates the bounded
catalog and command allowlist, and falls back to its packaged history when an
older release has no notes asset. Missing coverage is reported as unknown or
partial, never as evidence that no worker update is needed. A notes-service
failure does not prevent a checksum-verified CLI security update.

Successful manual and automatic updates retain their result in the global
updater state. Later checks do not erase it. `everycli auto-update` shows this
last successful update and its steps. An eligible interactive CLI invocation
announces an unseen background update once; JSON, help, version and redirected
commands remain quiet. The updater never reads provider keys or miner profiles.

The currently installed executable performs its own replacement. A v0.9.6
binary cannot acquire this new output retroactively while installing v1.0.0.
The feature applies once the new updater is installed. The v1.0.0 welcome or
What's new panel should cover that initial upgrade using its packaged notes;
do not claim that the old updater printed or saved a history receipt.

## v1.0.0 dashboard integration

This change intentionally leaves the UI assets to the dashboard redesign.
Existing CLI-update jobs expose the following additional JSON field:

```json
{
  "upgrade": {
    "status": "complete",
    "source": "release_asset",
    "from": "v0.9.3",
    "to": "v0.9.6",
    "workerUpdate": "conditional",
    "history": [
      {"version": "v0.9.5", "title": "BFL recovery", "changes": ["..."], "actions": [], "url": "..."}
    ],
    "actions": [
      {"kind": "worker-update", "command": "everycli worker-update", "requirement": "conditional", "conditions": ["If serving BFL..."], "versions": ["v0.9.5"]}
    ],
    "message": "",
    "releaseUrl": "https://github.com/everyframe-studio/everyframe-miner/releases/tag/v0.9.6"
  }
}
```

The global CLI auto-update status additionally exposes
`lastUpdate.result.upgrade` and `lastUpdate.notificationShown`. These fields
survive a later `lastAttempt` result of current, checking, or failed. The CLI
notification flag does not mean that a worker update or another step was done.
When no update is available, the update check exposes `currentRelease`; the
auto-update status always exposes it. This is the packaged entry for the
running CLI and lets the v1.0.0 welcome panel show What's new even when an old
updater did not save a receipt. It does not identify the previous CLI version.

Recommended UI presentation:

- Show the cumulative history before confirming a CLI update.
- Show required and conditional steps after it, including after background updates.
- Keep CLI version, installed worker release, and worker readiness distinct.
- Use the existing worker-update check/action and confirmation for any button;
  never execute the command string from release metadata.
- Keep unknown/partial history visible and link to the official release page.
- Preserve the existing operation ID and reconcile uncertain cloud operations.

The v1.0.0 entry combines this PR with the dashboard changes documented in
`feat/ui-redesign` at `ac518be`. Recheck that entry if the final UI scope changes. No version bump, tag,
release publication, worker deployment, or production restart is part of this PR.
