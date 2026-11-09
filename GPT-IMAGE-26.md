# GPT Image 2.6 provider contracts

The v1.0.0 source adds separate Sunburst and Flare offers. Direct OpenAI is
preferred for compatible requests, with independently configured Fal and
OpenRouter offers as fallbacks. Existing GPT Image 2 and 2.5 contracts are retained.

| Provider | Sunburst contract | Flare contract |
| --- | --- | --- |
| OpenAI | `direct/openai/gpt-image-2.6-sunburst` | `direct/openai/gpt-image-2.6-flare` |
| Fal | `studio/native/fal/openai/gpt-image-2.6/sunburst` | `studio/native/fal/openai/gpt-image-2.6/flare` |
| OpenRouter | `studio/native/openrouter/openai/gpt-image-2.6-sunburst` | `studio/native/openrouter/openai/gpt-image-2.6-flare` |

Studio's standard image workflow also has separate
`studio/media/openrouter/openai/gpt-image-2.6-sunburst` and
`studio/media/openrouter/openai/gpt-image-2.6-flare` adapters. They request one
medium-quality PNG; compatible miner routes are selected before the original
OpenRouter adapter. Advanced contract inputs use the six offers above.

## Settings

All six contracts support `quality` (`auto`, `low`, `medium`, `high`, `xhigh`,
`max`), `complexity` (`low`, `medium`, `high`), one to ten images, and up to
16 ordered image references. `background` supports `auto`, `opaque`, and
`transparent`; transparency requires PNG or WebP. JPEG and WebP support
`output_compression` from 0 to 100. Optional `moderation` supports `auto` and `low`.

OpenAI and Fal support explicit `size` values, `auto` sizing, masks and PNG markup
overlays. Static formats are PNG, JPEG and WebP. GIF generation and reference
edits support 2–16 frames and 20–2000 milliseconds per frame. GIF cannot use a
mask or custom output compression. SVG supports generation with one result,
without references or custom output compression. Fal also exposes `svg_model`.

OpenRouter publishes aspect buckets instead of exact pixel sizing. Its reviewed
2.6 endpoint supports raster images and GIF, without custom frame controls,
masks, markup or SVG. Unsupported settings never disappear during fallback.

Synchronous OpenAI/OpenRouter requests are submitted once. An uncertain submission is
never repeated automatically. Fal jobs resume polling the recorded
request after a worker restart. Outputs are bound to the input hash; SVG is
validated as static, self-contained vector content and GIF is fully decoded.

## Miner setup after release

Install the compatible signed worker with `everycli worker-update`. Configure
`OPENAI_API_KEY`, `FAL_KEY` or `OPENROUTER_API_KEY` for the providers you intend to
serve, then explicitly enable the corresponding offers and review their pricing.
Updating the CLI alone does not update a deployed worker or enable new offers.

Provider references:
[OpenAI image generation](https://developers.openai.com/api/docs/guides/image-generation),
[Fal Sunburst](https://fal.ai/models/openai/gpt-image-2.6/sunburst/text-to-image/api),
[Fal Flare](https://fal.ai/models/openai/gpt-image-2.6/flare/text-to-image/api), and
[OpenRouter image discovery](https://openrouter.ai/api/v1/images/models).
