# OpenRouter video routes

Miners can offer Wan 3.0 and Grok Imagine Video through their OpenRouter API key, alongside the existing Seedance and Veo routes. Use a normal OpenRouter API key with funds available for generation.

| Offer | Supported output | Reference modes |
| --- | --- | --- |
| `studio/openrouter/alibaba/wan-3.0` | 2–30 seconds; 480p, 720p, 1080p | Text; first frame; first and last frames |
| `studio/openrouter/x-ai/grok-imagine-video` | 1–15 seconds; 480p, 720p | Text; first frame |
| `studio/openrouter/bytedance/seedance-2.5` | 4–30 seconds; 480p, 720p | Existing keyframe and multimodal reference modes |

Install the latest CLI, then update the existing worker with `everycli worker-update`. Updating the CLI alone does not change the worker's supported models. Review `everycli offers` and explicitly accept each model's pricing and discount with `everycli offer`; offers for Fal do not automatically become OpenRouter offers. Use `everycli help offer` for the flags. Existing offers stay unchanged.

Compatible Studio requests can use these OpenRouter offers. Native provider routes remain first; the original Fal route remains available. Routing requires an enabled offer, a compatible attested worker, current pricing and sufficient budget. A selected provider must be confirmed not to have accepted a generation before another provider can be tried.

Model versions and settings must match. OpenRouter Seedance does not accept 1080p in this integration. Wan's reference-image mode is not enabled here; only its documented keyframes are accepted. New Wan and Grok contracts accept prompts up to 4,000 characters. Frame-guided cross-provider routing requires an exact supported source aspect ratio. Unsupported requests retain their original route.

H3 Max/Turbo, Kling 4, FLUX 3 Max and HeyGen are not redirected to similarly named models. H3 Max's OpenRouter seed and prompt controls differ from the existing Fal contract, and HeyGen requires separate price-tier and prompt-control handling. Catalog discovery alone does not make these routes interchangeable.

A miner discount is applied once to the selected route's base work value. Provider prices are planning estimates; an offer does not guarantee traffic, provider billing totals or alpha earnings.

Sources: [OpenRouter video API](https://openrouter.ai/docs/guides/overview/multimodal/video-generation) and [video model catalog](https://openrouter.ai/api/v1/videos/models).
