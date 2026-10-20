# everycli v0.6.0

## Security
- **Signed releases.** Every release binary now carries an Ed25519 signature
  (`<asset>.sig`) over its tag, asset name and SHA-256. `everycli update`,
  auto-update and the installer verify it against the release key built into
  everycli and refuse to install anything unsigned or altered. See SECURITY.md
  for manual verification.
- Publishing a release now requires approval (GitHub `release` environment),
  and an older-line hotfix no longer becomes the "latest" release.
- A `cargo install` build is never replaced by auto-update; `everycli update`
  tells you to run `cargo install` again instead.
- The installer needs OpenSSL 1.1.1 or newer to check signatures (macOS:
  `brew install openssl@3`).

## Worker
- An output the coordinator refuses is reported within seconds instead of being
  resent until the job deadline (this is what turned validation problems into
  8–12 minute timeouts).
- A billing, credential or missing-key refusal takes that provider's models off
  your claims for 15 minutes, so an empty balance no longer loses job after job.
- Upload time for finished outputs grows with their size (3–10 minutes)
  instead of a fixed 3 minutes.

## Recovery
- `everycli reconcile` now settles an interrupted activation and a rejected
  worker-update request instead of leaving `stop --force` as the only way out.
- New `everycli worker-update --abort` cancels an unfinished update and keeps
  the release your VM is actually running. A signed release that expires during
  an update now stops with instructions instead of hanging.
- Clear messages for a locked profile, an expired hotkey authorization,
  reconciliation required and an unconfirmed update.
- `--max-hourly-usd` states its allowed range (above 0, at most 100), and
  quotes print prices as `$0.05/hour`.
- `everycli miner update` still works but points you to `everycli worker-update`.

## Job history and earnings

- `everycli jobs` lists your individual attempts with status/model filters,
  cursor pagination and verification, review and failure details. Use
  `--summary` for the previous all-time counts by model. Older coordinators
  explicitly fall back to those counts.
- The browser UI adds **Job history** and **Earnings** pages, including attempt
  details, filters and Previous/Next navigation.
- Earnings separates pending review, approved and marked reward-eligible work
  from actual payments. Estimated provider exposure uses job quotes, not an
  invented invoice; uncertain and unpriced submissions are counted separately.
  Actual provider bills, hosting, alpha received and net profit remain unmeasured.
- These signed, private, read-only views do not expose customer prompts or media,
  approve work, alter validator accounting or require a worker update.

## Batch offers

- An advertised batch-offer endpoint returning 404 now identifies the coordinator
  deployment problem and asks you to check current offers. The CLI never retries
  an uncertain write or falls back to partially applying individual offers.
- The mainnet public gateway now forwards the atomic batch endpoint with a
  separate 64 KiB limit, matching the coordinator. This server-side fix also
  restores batch offers for existing v0.5.8 clients; no worker update is needed.

## Update
Run `everycli update`, then `everycli worker-update`. Jobs go only to workers
on the newest minor release: once v0.6.0 is published, 0.5.x workers keep
receiving jobs for 3 days, then stop until they update.
