## Fal availability and worker recovery

- Restores Fal model availability after provider retirements. Confirmed models stay available when another catalog request fails or is rate-limited, and checks stop sending requests after a rate limit.
- Recognizes LTX 2 Pro as retired. Workers also receive a signed list of retired models, allowing future retirements to be handled without another worker release. This list can only disable models.
- Fixes stale-attestation errors for healthy workers with a valid admitted session and a fresh heartbeat. Expired sessions, workload binding and fresh attestation after a restart remain enforced.
- Detects an existing VM stopped outside everycli. The dashboard offers Start, and the CLI can start or reconcile that same VM without redeploying.
- Improves failed-start recovery and error messages. Rejected requests retain their reason; uncertain requests are not repeated. Reconciliation can recover a start that Phala confirms has failed, while keeping pending operations protected.

## Update

Run these commands from your existing management profile:

```bash
everycli update
everycli worker-update
```

The worker update installs the Fal availability fix. Already-enabled automatic worker updates apply it without a manual worker-update command. Existing offers, discounts and API keys are preserved.

If status reports a legacy expiring authorization, use `everycli reconnect` with the same hotkey, then `everycli apply-api-keys` once to install non-expiring authorization. Wait for fresh attestation before resuming work.
