# everycli v1.3.5

Before a first deployment, everycli compares the local worker configuration with the coordinator and refreshes it if the compose hash changed. The CLI verifies the refreshed configuration before asking you to approve hosting.

If a previous first deployment is stuck at `provision_intent` after a configuration change, run `everycli update` and then `everycli reconcile --json` in the original profile. Recovery checks the coordinator and the Phala workspace, then archives the interrupted preparation while preserving your keys. When it reports `resolved: true` and `phase: "not_deployed"`, run `everycli reconnect` with the same hotkey.

This is a CLI update. If the dashboard is open, close it and restart `everycli ui`.
