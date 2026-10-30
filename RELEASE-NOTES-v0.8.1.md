## Miner authorization and API balances

- Miner authorizations do not expire. An outdated setup configuration refreshes automatically before management changes, using the existing authorization and preserving the worker.
- Reconnecting a legacy enrollment now enables non-expiring authorization without changing its VM or offers.
- The API Keys page shows provider balances, status and the last check time. Refreshing balances preserves key drafts, and saved keys can be checked before miner initialization.

Run `everycli update`. No worker update is required for these changes.

If your profile still reports a legacy authorization, run `everycli reconnect` with the same hotkey, followed by `everycli apply-api-keys` once to install the non-expiring authorization on the running worker.
