# Everycli v0.6.1

## FLUX 3 Max

- Added the Fal-backed FLUX 3 Max video contract: 5–20 seconds, 720p/1080p,
  landscape/portrait/square formats, generated audio, and first/last images.
- Uses FLUX-specific fields and low reasoning effort, not H3 request defaults.
- Reference video/audio and unordered image sets are rejected before billing.
- Requires a matching signed worker update, a Fal key, and a current model offer.
  Updating everycli alone does not update the running worker or activate a bid.
- Promotional rates expire automatically on the coordinator; when pricing changes,
  review and renew the discount rather than silently accepting a new rate.
- Expanded the bounded Fal catalog lookup budget so one unavailable endpoint can
  still be isolated as the catalog grows.

Update the CLI with `everycli update`, then use `everycli worker-update` when
the coordinator offers the signed worker package. Review `everycli doctor`
and `everycli offers` before accepting new work. Existing jobs and bids are
not rewritten, provider keys are preserved, and TEE admission stays mandatory.
