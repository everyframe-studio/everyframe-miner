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

Linux x86-64/ARM64 and macOS Intel/Apple Silicon downloads include the embedded UI.
No Node, Python or Rust installation is needed.

CLI upgrades do not change running workers, offers, routing or provider keys.
Worker safety changes in this source release require a separately admitted worker
image. Non-expiring authorizations require coordinator/worker support explicitly
enabled by a signed deployment; existing authorizations are not silently extended.
Public model discovery before setup requires the coordinator's catalog endpoint.
