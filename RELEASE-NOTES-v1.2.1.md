# everycli v1.2.1

Offer Veo 3.1 jobs with the settings used in EveryFrame Studio.

- Adds `studio/openrouter/google/veo-3.1`: 4, 6 or 8 seconds, 720p or 1080p, landscape or portrait, generated audio and first/last frames.
- Uses the existing OpenRouter adapter and signed model catalog. Workers already running v1.2.0 can receive the definition between jobs without another binary update.
- Keeps the expanded contract separate from existing fixed-setting Veo offers. Review the new model and choose your discount on the Models page.

## Upgrade

```sh
everycli update
```

If your deployed worker predates v1.2.0, also run `everycli worker-update` once for signed-catalog support. Reopen `everycli ui` after updating.

Veo requires an OpenRouter generation key, a compatible worker, model authorization and an active offer. A catalog update does not create offers or resume a paused miner. Generic reference images remain outside this contract.
