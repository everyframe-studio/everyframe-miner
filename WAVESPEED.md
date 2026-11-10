# WaveSpeedAI in v1.1.0

This release adds WaveSpeedAI as a separate provider, with 85 reviewed endpoints:
38 image, 44 video and 3 audio routes. They include GPT Image 2.5/2.6, Gemini
Omni 1.1/1.2, Nano Banana 2.1, Grok, Seedance, Kling 4, Wan 3, FLUX, Ideogram,
MiniMax H3 and speech/music. WaveSpeed-hosted Z-Image, Qwen Image 3, Seedream,
Hunyuan Video 1.5 and H3 variants are also included.

The exact IDs, request fields, enum choices and reference limits are in
[config/wavespeed-models.json](config/wavespeed-models.json). Each endpoint has
its own `studio/wavespeed/…` ID and independent offer. Existing provider offers
and discounts are unchanged.

## Miner setup after release

1. Update everycli and reopen its UI.
2. Run `everycli worker-update` to install the compatible signed worker on the
   existing VM. The coordinator must first have the v1.1.0 integration and a
   reviewed deployment allowing `WAVESPEED_API_KEY`.
3. Save the key in the UI's WaveSpeedAI provider settings, or run
   `everycli set-api-keys --provider wavespeed`. Apply it to the verified worker.
4. Check the key/balance and review the WaveSpeed models and prices. Explicitly
   enable the offers you want. An upgrade never opts you in to paid work.

The balance check uses the generation key and reports USD. Read-only catalog
checks can narrow the reviewed model list; they cannot load new code, schemas,
model IDs or prices into the worker. Native providers remain first in the UI.
Exact compatible WaveSpeed Grok video requests can use native xAI first; other
WaveSpeed requests retain their explicit provider contract. H3 and H3 Max are
not treated as interchangeable models.

## Pricing and execution

WaveSpeed public pricing formulas are compiled offline into bounded arithmetic
ASTs and checked against the selected duration, resolution and verified reference
metadata. The coordinator locks that work-value quote before a job. Account
promotions, negotiated discounts and actual provider charges may differ from
public estimates; the displayed work value is not a provider invoice or profit.
WaveSpeed offers are manual in this release. Automatic offers continue to support
reviewed Fal and OpenRouter profiles, including the 10 added Fal video profiles.

Generation is submitted once. The stored task ID is bound to the signed request
and survives worker restart. Result polling is limited to one request every two
seconds, including outages. Only reviewed WaveSpeed output hosts are downloaded;
keys are never sent to delivery hosts. Output formats and media are validated by
the coordinator. Timeouts and server failures do not prove that no paid job was
created and never trigger a blind submission retry.

## Provider references

- [Authentication](https://wavespeed.ai/docs/api-authentication)
- [Model catalog](https://wavespeed.ai/docs/list-models)
- [USD balance](https://wavespeed.ai/docs/check-balance)
- [Submit task](https://wavespeed.ai/docs/submit-task)
- [Result polling](https://wavespeed.ai/docs/get-result)
- [Price query](https://wavespeed.ai/docs/pricing-api)
- [Error codes](https://wavespeed.ai/docs/error-codes)

The catalog was reviewed on 2026-11-10. Each entry carries its public model-page
URL and source hash. `scripts/import-wavespeed-catalog.py` imports a saved,
reviewed public catalog; it does not run fetched pricing code. Schema or pricing
updates require a new review and the cross-language fixture tests.
