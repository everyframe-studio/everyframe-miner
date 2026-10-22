# More models from existing providers

Reviewed 2026-10-22 for v0.7.1. These source additions are included in the CLI
release but are not an admitted production worker or live model rollout.
Existing keys, discounts, authorizations and deployments remain unchanged.
No additional provider account type is introduced.

| Provider | Added models | Initial contract |
| --- | --- | --- |
| MiniMax | Hailuo 02 | Text to video, 6s, 1080p, 16:9 |
| BFL | FLUX.2 Max, Flex, Klein 4B, Klein 9B | One 1024×1024 PNG |
| Replicate | FLUX.2 Pro | One 1024×1024 PNG |
| Runway | Veo 3.1, Veo 3.1 Fast | Text to video, 4s, 720p, 16:9, audio |
| Luma | Ray 2 Flash | Text to video, 5s, 720p, 16:9 |
| ElevenLabs | Eleven v4, v4 Turbo, v3, Flash v2.5 | Rachel voice, MP3 44.1 kHz/128 kbps, up to 500 characters |
| OpenRouter | Veo 3.1 Fast, Veo 3.1 Lite | Text to video, 4s, 720p, 16:9, audio |

These are 15 additional fixed contracts, not every upstream mode. Reference
images, arbitrary voices, portrait formats and alternate durations are not
enabled by these contracts. Existing flexible Studio contracts stay separate.
FLUX.2 image models are not FLUX 3 Max video.

## Miner setup

Use the existing provider key setup, for example:

```sh
everycli set-api-keys --provider bfl
everycli set-api-keys --provider elevenlabs
everycli offers
```

Do not replace working keys just to add a model. Key presence is not proof of
account entitlement or funds. Existing provider balance checks remain in use;
a model addition does not create a separate balance or turn a web subscription
into API credits. Read the actual offer/readiness state before enabling bids.

## Implementation and release boundaries

- `config/provider-expansion.json` adds new IDs without changing the 47 existing
  base registry entries or their signed request hashes.
- The additions use the existing provider adapters, submission-deadline guards,
  terminal-state handling, receipt binding and media validation.
- Luma downloads additionally allow its documented `storage.cdn-luma.com` host,
  without credentials or redirects. Arbitrary hosts remain prohibited.
- Coordinator discovery includes each addition once, without widening a signed
  allowlist or enabling an unpriced offer. No price is copied from Fal or an
  older model. The CLI/UI use that authenticated catalog.
- Ambiguous paid submissions are not retried. Synchronous speech is input-bound
  and is not silently regenerated after a worker restart.
- OpenRouter failures with a null model field now report a terminal failure,
  matching its documented early-failure response, rather than a model mismatch.
- Before live routing: review provider-specific prices, run funded paid tests,
  validate real outputs, build the final immutable worker and verify TEE
  admission, then publish the signed worker/model allowlist.
- Updating everycli alone does not activate these models on deployed workers.
  Worker updates and existing automatic-update opt-in requirements still apply.

## Upstream evidence

Local verification on 2026-10-22: the full Rust release-profile suite passed
299 tests (including 33 focused provider/protocol/catalog/packaging tests), with
two intentionally ignored live probes requiring dedicated authorization
(hotkey identity and paid provider tests).
The full companion coordinator suite passed 316 tests. Pinned-toolchain Clippy
with warnings denied, formatting and diff checks passed. Provider HTTP is
mocked in these suites; public documentation, BFL schemas and the authenticated
OpenRouter video catalog were checked read-only. The native Linux release build
passed. One approved live OpenRouter test submission was rejected with HTTP 402;
the account balance check confirmed insufficient funds. No video was generated
and no submission was retried. No successful real-media/TEE canary, Docker
build or production deployment was performed during that local validation. The
CLI publication workflow separately gates on native CI and container checks. See
[release checklist](RELEASE-CHECKLIST-v0.7.1.md) for the remaining gates.

- [MiniMax video API](https://platform.minimax.io/docs/api-reference/video-generation-t2v)
- [BFL OpenAPI](https://api.bfl.ai/openapi.json): all four endpoints and their fields were checked directly.
- [Replicate schema](https://replicate.com/black-forest-labs/flux-2-pro/api/schema)
- [Runway official SDK schema](https://github.com/runwayml/sdk-node/blob/main/src/resources/text-to-video.ts)
- [Luma video API](https://docs.lumalabs.ai/docs/video-generation)
- [ElevenLabs models](https://elevenlabs.io/docs/overview/models) and [speech API](https://elevenlabs.io/docs/api-reference/text-to-speech/convert)
- [OpenRouter video API](https://openrouter.ai/docs/guides/overview/multimodal/video-generation), [Fast](https://openrouter.ai/google/veo-3.1-fast) and [Lite](https://openrouter.ai/google/veo-3.1-lite). Both IDs were also present in the public catalog on 2026-10-22.

Google Imagen is excluded: its [current documentation](https://ai.google.dev/gemini-api/docs/imagen)
says it is shut down. Additional Gemini API Veo previews are also excluded:
the [video guide](https://ai.google.dev/gemini-api/docs/video) announces their
2026-10-22 shutdown. New Gemini image/video APIs need a separately reviewed
adapter rather than the legacy Veo adapter. Historical contracts are preserved;
this does not assert that OpenRouter/Runway's separate Veo routes are shut down.
