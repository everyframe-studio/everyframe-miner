## Fal model availability recovery

On 29 October 2026 Fal retired `fal-ai/ltx-2/text-to-video`. Its public catalogue now answers 404 for any batch that names it, and the worker's availability check then ran into Fal's rate limit. Every Fal model was marked temporarily unavailable, so v0.8.1 and older workers stopped offering H3, Wan, Grok, Kling 4.0, Flux 3, Nano Banana, Ideogram, the audio routes and the other Fal models.

- LTX 2 Pro (`fal-ai/ltx-2/text-to-video`) is recorded as removed by the provider.
- A rate limit, outage or missing endpoint during a Fal check no longer voids the whole provider. Models already confirmed stay available; only unresolved models wait for the next check, and the worker stops sending requests after a rate limit.
- Workers adopt the coordinator's signed list of retired provider models from each claim reply. The list can only stop a worker offering a model; it cannot enable, add or reprice one. Future provider retirements no longer need a worker release.

This release changes the worker. Run `everycli update`, then `everycli worker-update` from your original management profile (automatic worker updates apply it for you). Offers, discounts and API keys are unchanged.
