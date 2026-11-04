# Miner Terms acceptance

Read the [Miner Terms of Service](miner-terms.md) before joining. You are responsible
for obtaining and maintaining provider permissions for commercial resale and serving
third-party requests, and for negotiating approvals and discounts directly with
providers. A catalog entry, successful key check or accepted miner does not grant
these rights.

In `everycli ui`, select and check your hotkey, then open **Accept the Miner Terms**.
Read the full terms and check the agreement box before selecting **Accept Miner
Terms**. Creating the miner profile stays disabled until acceptance is recorded.
Existing profiles see an acceptance reminder. Acceptance is shared between the UI
and CLI in the same profile.

In a terminal, `everycli terms` shows the complete current terms and acceptance
status. Interactive enrollment and actions that enable new work prompt when a
current acceptance is missing. After reviewing the current version, unattended
operators can run:

```sh
everycli terms --wallet my-wallet --hotkey default
everycli terms --wallet my-wallet --hotkey default --accept-terms 2026-11-04
everycli init --wallet my-wallet --hotkey default --json
```

Alternatively, pass `--accept-terms 2026-11-04` to `init` after reviewing the terms.
Use the same `--network` and `--state-dir` throughout when selecting a different
profile. `--yes` alone never accepts terms. There is no environment variable that
auto-accepts them.

The private `terms-acceptance.json` file records the terms version and SHA-256,
public hotkey, network, acceptance time and CLI version. Acceptance is local to
this device and profile. A different hotkey, network, terms version or document
hash requires a new acceptance. Existing records are preserved. Change the terms
version whenever changing the agreement text.

Missing acceptance blocks enrollment, deployment, starting, key activation,
resuming and publishing or updating offers through this CLI. It does not stop
an already-running worker or prevent reading status, recovery, stopping, deleting,
removing credentials or withdrawing offers. Acceptance itself makes no provider or
cloud request and does not change keys, offers or worker state.

This feature is a CLI/UI acceptance gate. Older clients and existing workers do
not enforce it. Coordinator-side acceptance registration and admission enforcement
would be a separate coordinated migration; this change does not deploy one or
claim to hold a server-side acceptance record. No new worker image is required
for the local acceptance flow.
