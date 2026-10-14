# everycli v0.5.5

A cleaner miner dashboard, jobs by model, and faster atomic model-offer batches.

## Dashboard and terminal

- Hide unintegrated catalog entries in CLI/UI. Integrated models that need a
  provider key, current pricing or a worker update retain actionable labels.
- Overview shows all-time jobs by model. Use `everycli jobs` in the terminal,
  or `everycli jobs --json` for structured output. Completed counts mean distinct
  accepted jobs; failed/cancelled columns count attempts, not wallet payments.
- Overview and `everycli earnings` show readable serving-fee tables, not nested
  JSON. Serving fees are accounting, not proof of alpha or cash received.
- Models and All models display badges for newly integrated model IDs after the
  first catalog snapshot. Opening All models acknowledges them in this browser
  and profile; no credentials are stored with these acknowledgments.

## Faster batch editing

- One reviewed batch replaces repeated per-model submissions. Pricing and offer
  revisions are checked atomically: a conflict rejects the entire batch.
- Terminal: `everycli offer-batch --models ID,ID --discount-pct 10` (or `--withdraw`).
  Select model IDs from `everycli offers`. Up to 128 distinct models per batch.
- Confirmation binds the full changes and price revisions. Uncertain submissions
  are never automatically retried; refresh offers before deciding what to do next.
- Results can be dismissed, clear on navigation, and expire after 12 seconds
  (30 for errors). Expiration preserves unsaved discount input drafts.

## Upgrade

```sh
everycli update
everycli --version
everycli ui
```

Close an already running UI before reopening it. The SN117 coordinator has the
matching catalog, job-count and atomic-batch support. This release updates the
local CLI/UI only: it does not change worker images, provider keys, existing
offers, activation or routing state. No worker redeployment is needed.

Includes the v0.5.4 deleted-workload recovery fixes. A miner recovering a deleted
VM still needs to reconnect with its hotkey, deliberately deploy, then complete
attestation and activation; updating the CLI does not perform those actions.
