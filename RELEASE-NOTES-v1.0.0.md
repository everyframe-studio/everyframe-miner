# v1.0.0 — A redesigned miner dashboard

`everycli ui` has a new layout and a new EveryFrame Miner icon.

The miner card at the top of Overview now shows your health ring on the left
and your subnet and everycli version above the miner name. When the dashboard
opens, reloads or re-checks health, the right side of the card lists every
check up front and runs them one by one: profile and keys, coordinator, Phala
worker and readiness, plus provider balances and model offers.
Each row shows its result and how long it took. Click the ring to jump to the
full health checks.

`everycli health` also prints which stage it is on while it runs. JSON output
is unchanged.

Update with `everycli update`, then run `everycli ui`. Your miner profile,
deployed worker, offers and discounts are unchanged, so `everycli
worker-update` is not needed for this release.
