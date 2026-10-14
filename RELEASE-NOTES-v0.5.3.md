# everycli v0.5.3

## Update

```sh
everycli update
everycli ui
```

Close the old UI process before opening the updated UI. Existing miner profiles,
provider keys, worker labels, and deployment records are kept.

## Setup and authorization

- **Select your hotkey** becomes **Your hotkey** once attached. Reopened setup
  shows the saved identity and its last verified UID; **Check registration**
  remains a read-only chain check.
- Deploy automatically refreshes coordinator-rejected authorization once using
  the matching attached hotkey, before any paid cloud request. Legacy profiles
  without a saved signer path can find the same hotkey in the standard wallet
  directory. Missing, unreadable, or mismatched files still require attention.
- Attaching the same hotkey again with `init` safely refreshes an existing public
  profile without discarding API keys or deployment history.
- Recovery validates signed identity and deployment bindings. Ambiguous cloud
  operations are never automatically retried, and deployment still requires
  confirmation of the live hosting quote.
- The UI's maximum compute-price field defaults to **$10/hour**. This is a price
  ceiling, not a total budget or a promise to spend $10/hour. It does not change
  the signed instance type; storage costs extra.

## Batch model offers

Select individual models or all editable models in any Models tab, then apply
one discount or withdraw selected offers. One review presents the exact CLI
quotes before applying changes. Unavailable new offers are excluded. Quotes
that change after review are not auto-approved. Results are reported per model;
batches are not atomic, and uncertain requests are not automatically retried.

## Cleaner overview

Four responsive cards: **Worker**, **Status**, **Active offers**, and **Jobs**.
Status distinguishes Ready, Working, Paused, Needs attention, and Unknown instead
of a bare Yes/No. Readiness and attestation details remain under Health checks.

## Scope

This release updates the native CLI and bundled browser UI for Linux x64/ARM64
and macOS Intel/Apple Silicon. Updating the binary does not deploy, replace,
restart, or change a running worker, and does not change offers or start jobs.
