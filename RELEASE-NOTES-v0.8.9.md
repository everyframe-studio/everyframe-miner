# Everycli v0.8.9 — Deleted worker recovery

Recover a deleted worker even when Phala no longer offers its original OS image.

- `everycli reconnect` checks that the old VM is gone, asks before retiring its
  old authorization, and fetches the current reviewed deployment.
- **Deploy worker** then creates a fresh VM using that approved OS image.
- Saved API keys, model offers, discounts and deletion history are preserved.
- Existing workers keep their current OS and deployment. Reconnect does not
  create or restart a VM, enable offers, or resume work automatically.

Update in the same OS user and miner profile:

```sh
everycli update
everycli reconnect
everycli doctor
everycli ui
```

Confirm retirement only for the old VM you deleted. Then review **Deploy
worker** in the dashboard. A worker software update is not required for this
CLI fix.
