# MiniMax H3 Image

Separate offers for H3 Image Standard and Highspeed, including text-to-image
and editing with up to 16 ordered references. MiniMax direct is preferred for
compatible requests; OpenRouter and Fal follow with their own keys and offers.
Older Image-01 and WaveSpeed H3 contracts keep their original identities.

| Provider | Standard | Highspeed |
| --- | --- | --- |
| MiniMax | `direct/minimax/h3-image` | `direct/minimax/h3-image-highspeed` |
| OpenRouter | `studio/native/openrouter/minimax/h3-image` | `studio/native/openrouter/minimax/h3-image-highspeed` |
| Fal | `studio/native/fal/minimax/h3-image` | `studio/native/fal/minimax/h3-image-highspeed` |

The Studio also has `studio/media/fal/minimax/h3-image/text-to-image` and
`studio/media/fal/minimax/h3-image/highspeed/text-to-image`. Compatible jobs try
MiniMax direct, then OpenRouter, then the original Fal route. Every route keeps
the selected variant, exact dimensions, prompt enhancement and reference order.

## Settings

Native requests support exact `size: "WIDTHxHEIGHT"`, 655,360 to 20,000,000
pixels, and an aspect ratio between 1:3 and 3:1. MiniMax and OpenRouter also
support `size: "auto"`, `area: "1K" | "2K" | "4K"` and `aspect_ratio`. Exact
sizes must not be combined with area or aspect controls. Fal requires exact
dimensions, so requests for automatic framing retain their compatible routes.

`n` supports 1–10 images; `output_format` supports JPEG, PNG and WebP.
MiniMax also accepts `output_compression` (0–100) for JPEG/WebP. An explicit
compression choice keeps the MiniMax route because the other reviewed APIs
do not expose that control. Image edits always use prompt enhancement.
Seeds, masks and transparent backgrounds are not supported by these APIs.

Studio exposes 1K, 2K and 4K for both variants and accepts 16 reference images.
Exclusions entered in Studio become instructions in the signed prompt; the
native API does not have a negative-prompt parameter.

Synchronous submissions are sent once with a response timeout above the
provider's 600-second maximum. Fal queue jobs recover by polling their bound
request. An uncertain submission is not replayed or treated as a free rejection.
Content-safety rejections stop routing. Image outputs are decoded and checked
against their requested dimensions and format before acceptance.

Pricing uses published text, reference-image and output-image token rates with
explicit planning allowances. It does not represent a provider invoice, account
discount or profit calculation. Temporary launch discounts are not assumed in
the long-lived pricing policy.

## Miner setup after the compatible release

Update the CLI, then run `everycli worker-update` to install the signed worker.
Configure `MINIMAX_API_KEY`, `OPENROUTER_API_KEY` or `FAL_KEY` for the providers
you intend to serve. Review prices and enable their offers explicitly.
Updating the CLI alone does not change a running worker or activate new offers.

References: [MiniMax image guide](https://platform.minimax.io/docs/guides/image-generation),
[MiniMax pricing](https://platform.minimax.io/docs/pricing/overview),
[OpenRouter image API](https://openrouter.ai/docs/guides/overview/multimodal/image-generation),
[Fal Standard](https://fal.ai/models/minimax/h3-image/text-to-image/api),
[Fal Highspeed](https://fal.ai/models/minimax/h3-image/highspeed/text-to-image/api).
