# LTX 2.5

v1.1.3 includes 14 explicitly reviewed routes:

| Provider | Variants | Inputs | Key |
| --- | --- | --- | --- |
| LTX / Lightricks | Fast, Pro | Text, first/last image, audio with optional images | `LTX_API_KEY` |
| Fal | Fast, Pro | Text, first/last image, audio with optional first image | `FAL_KEY` |
| WaveSpeedAI | LTX 2.5 | Text, first/last image | `WAVESPEED_API_KEY` |

Run `everycli update`, `everycli worker-update`, then set the relevant key in the UI or with `everycli set-api-keys`. The LTX account dashboard supplies native API keys and credits. This integration does not invent a USD balance endpoint for LTX.

The native API supports 720p through 4K, five aspect ratios, 24/25/48/50 FPS and automatic duration. Fast permits up to 20 seconds at 720p/1080p and 24/25 FPS; other combinations permit up to 10 seconds. Fal Pro exposes 720p/1080p and 24/25/50 FPS; its audio endpoint exposes fewer controls. WaveSpeed accepts 5–20 seconds and four resolutions. Model details display each route's exact schema. Retake and extend are not LTX 2.5 endpoints according to Lightricks' support matrix.

References must be owned, content-hashed attachments, bound to each request. Native and Fal jobs store the returned job ID and resume with GET requests after a restart. An uncertain submission is never automatically repeated. Downloads use fixed provider output hosts and carry no API credentials. Native LTX polls are cached for at least five seconds.

Each route has independent public list-price estimates checked 2026-11-11, valid through 2026-11-24. Text/image quotes use requested output duration. Automatic duration reserves the maximum supported length; audio quotes use verified input audio duration. Actual provider bills and account discounts can differ. WaveSpeed does not identify its model as Fast or Pro, so no cross-provider equivalence is assumed for it. LTX/Fal conversion is limited to exactly representable settings; a native square request, for example, stays native.

Sources: [LTX models](https://docs.ltx.io/models/ltx-2-5/), [LTX OpenAPI](https://docs.ltx.io/openapi.json), [LTX prices](https://docs.ltx.io/pricing/), [Fal](https://fal.ai/ltx-2.5), [WaveSpeed](https://wavespeed.ai/models/wavespeed-ai/ltx-2.5/text-to-video).

The integration is covered by offline contract, pricing, routing and transport tests. No paid LTX generation was included in the v1.1.3 Google validation budget.
