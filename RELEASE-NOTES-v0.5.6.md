# everycli v0.5.6

## Recovery

- Recover an interrupted initial VM preparation with `everycli reconcile`, also
  available as **Re-sync operation** in the browser UI.
- Requires a verified, unbound, idle coordinator and a complete successful Phala
  inventory without the signed VM name or prepared app ID.
- Archives the abandoned preparation before clearing it. Keeps credentials,
  registration, label and signed configuration. Performs no cloud mutation.
- Refuses ambiguous states, replacements, pending updates and commit intents.
  Never delete the operation journal to force a second deployment.
- Adds actionable reconciliation error messages.

For the reported `provision_intent`, null `appId`, empty-workspace case:

```sh
everycli update
everycli reconcile
everycli set-api-keys
everycli reconnect
```

Run the remaining commands only after reconciliation succeeds. Review the
hosting quote when ready to deploy; reconciliation does not start a worker.

## Provider balances

- Includes `feat/all-provider-balances`: Fal balance reads using the normal
  generation key, plus BFL and Runway account credits converted to USD.
- Retains optional local Fal billing/admin credentials and native Vivix W-credit
  reporting. Billing-only and Phala keys are not deployed to workers.
- Uses bounded, read-only requests to fixed provider hosts; failed or malformed
  responses are unavailable, never an invented zero or exposed upstream body.
- Fal generation-key and Vivix balance endpoints are undocumented and may change.
- Other unsupported provider balance APIs remain explicitly unsupported.

## Distribution boundary

The CLI and worker source changes are included in this release. The four native
CLI binaries do not replace existing VMs. Worker telemetry changes require a
separately verified, signed worker release and `everycli worker-update`; use
`everycli worker-update --check` to inspect availability. This release makes no
claim that every deployed miner or every provider has been live-tested.
