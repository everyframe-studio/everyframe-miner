# Everycli v0.6.2

## FLUX 3 Max

- Adds the Fal-backed FLUX 3 Max video contract: 5–20 seconds, 720p/1080p,
  landscape/portrait/square formats, generated audio, and first/last images.
- Uses FLUX-specific fields and low reasoning effort, not H3 request defaults.
- Reference video/audio and unordered image sets are rejected before billing.
- Requires a Fal key, a matching signed worker update, and a current model offer.
  Updating everycli alone does not update a worker or activate a bid.
- Promotional rates expire automatically; review and renew offers when pricing changes.
- Expands the bounded Fal catalog lookup budget as the model catalog grows.

## Release reliability

- Fixes an intermittent copied-executable launch race in the CLI's test harness.
  Only pre-launch OS errors are retried; no generation or updater behavior changes.
- v0.6.1 was tagged but its installers were not published because that test failed.
  Its tag is unchanged. v0.6.2 includes the fix and supersedes that attempted release.
- The signed worker update uses the same attested v0.6.1 worker image that passed
  the live FLUX tests. CLI and worker patch versions can differ.

Run `everycli update`, then review `everycli worker-update --check` and install
the offered worker update. Use `everycli doctor` and `everycli offers` to check
readiness. Existing VMs, provider keys, and discounts are not changed by a CLI update.
