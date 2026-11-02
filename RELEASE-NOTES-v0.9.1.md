# Everycli v0.9.1 — Enrollment reliability

- Gives enrollment checks time to finish when chain or deployment-catalog responses are slow.
- Shows gateway HTTP errors and specific enrollment failures instead of a generic error.
- Keeps setup recovery guidance on the same wallet and hotkey.

Run `everycli update`. If initialization previously failed, retry the original
`everycli init --wallet YOUR_WALLET --hotkey YOUR_HOTKEY` command.
No worker update or new subnet registration is needed for this fix.
