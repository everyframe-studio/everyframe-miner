# everycli v0.5.0

Set up and manage your miner in your browser:

```sh
everycli update
everycli ui
```

- Guided setup, API-key management, offers, health, balances and worker controls.
- Loopback-only server, single-use pairing, private key storage and explicit paid-action confirmations.
- Setup inputs survive background refresh. Pending starts/shutdowns have re-sync controls.
- Setup readiness follows current health; paused/offline miners are not shown as accepting work.
- Vivix balances display native W credits, including zero; balance failures show actionable reasons.
- Local worker labels, sanitized logs, emergency shutdown and exact-ID VM deletion from the CLI.
- Crash-safe profile locks and verified replacement binaries with rollback copies. Source builds are not silently replaced.
- Worker fixes: recognize terminal Fal failures, check the deadline immediately before paid submission, classify exhausted Fal balances for safe failover, and allow longer media transfers.
- Workers check Vivix W credits; known zero blocks W1 jobs. Coordinator scheduling also enforces one active Vivix job per shared API credential.
- Quieter idle logs with actionable failure codes, and a signed public model/pricing catalog before setup.
- Non-expiring, revocable hotkey authorization on compatible signed deployments. Existing signed expiry dates are never silently ignored.

Linux x86-64/ARM64 and macOS Intel/Apple Silicon downloads include the embedded UI.
No Node, Python or Rust installation is needed.

CLI upgrades do not change running workers, offers, routing or provider keys.
The matching v0.5.0 worker image and coordinator support are released separately
from the CLI executable. To apply the worker fixes on your existing VM:

```sh
everycli worker-update
everycli doctor
```

Use the original management profile. Do not redeploy a new VM. The update preserves
your offers and paused/running intent and requires fresh hardware attestation.

After the worker update, existing miners can migrate their old 30-day authorization
with `everycli init` (using their local hotkey), then `everycli apply-api-keys`.
The latter restarts the worker and leaves it paused; wait for fresh attestation and
run `everycli resume` when ready. New setups receive non-expiring authorization
automatically. Revocation, identity binding and fresh request proofs still apply.
