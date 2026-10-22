# everycli v0.7.1 — More models from existing providers

This release expands the source model registry and improves provider handling.
**CLI/source release only: the 15 new model contracts are not yet enabled for
live mining.** Existing workers, bids, credentials and automatic-update opt-in
settings are unchanged.

## Added model contracts

- MiniMax: Hailuo 02.
- BFL: FLUX.2 Max, Flex, Klein 4B and Klein 9B.
- Replicate: FLUX.2 Pro.
- Runway: Veo 3.1 and Veo 3.1 Fast.
- Luma: Ray 2 Flash.
- ElevenLabs: Eleven v4, v4 Turbo, v3 and Flash v2.5 with the fixed Rachel voice.
- OpenRouter: Veo 3.1 Fast and Veo 3.1 Lite.

These are fixed text-input contracts, not every upstream format or reference
mode. The original 47 base contracts and their request hashes are preserved.
See [supported contract limits](PROVIDER-EXPANSION.md).

## Fixes and verification

- Recognizes terminal OpenRouter video failures even when the provider has not
  assigned a model field yet; request identity checks remain enforced.
- Accepts Luma's documented `storage.cdn-luma.com` output host without sending
  provider credentials to it or allowing arbitrary redirects.
- Includes the expanded registry in the worker/updater build context.
- Adds deadline, uncertain-submission, contract-preservation and speech-recovery
  regressions, plus an explicitly opt-in paid output test harness.
- Local Rust suite: 299 passed; two live probes intentionally ignored. Companion
  coordinator suite: 316 passed. These are not proof of funded provider access
  or final worker attestation. Publication also requires native CI and container
  build/smoke checks.

## Updating

```sh
everycli update
```

This updates the CLI, not the deployed worker. Run `everycli offers` to inspect
actual model availability. Do not redeploy, bypass attestation or replace keys
just to enable a registry entry. New models require independently reviewed
pricing, funded generation/output tests and a compatible signed worker release.

No new production worker or offline-updater bootstrap is published with this
release. Existing manual worker-update behavior remains unchanged. Kling 4.0
is not part of v0.7.1.
