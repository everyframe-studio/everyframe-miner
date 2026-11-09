# Gemini Omni 1.2 Flash

Prepared for v1.0.0. These additions do not publish a CLI release, approve a
worker image, enable offers or change an existing worker deployment.

| Provider | Separate contract | Generation settings |
|---|---|---|
| Google direct | `direct/google/gemini-omni-1.2-flash` | 3–15 whole seconds; 360p, 720p, 1080p, 4k; 16:9 or 9:16; native audio |
| Fal | `studio/fal/google/gemini-omni-flash/v1.2/text-to-video` | Same durations and tiers; text, first/last frames or image/video references |
| OpenRouter | `studio/openrouter/google/gemini-omni-1.2-flash` | Same durations; catalog spelling `4K`; text or first/last frames; audio and seed retained |

Google uses `GEMINI_API_KEY`; Fal and OpenRouter use their existing provider
keys. Defaults are five seconds, with the requested settings retained in each
signed job. The Google contract uses asynchronous, stored Interactions and URI
delivery so polling can resume after a worker restart without resubmitting a
paid generation.

Google accepts up to six reference images and three reference videos of up to
three seconds each. For 11–15 second output, at most one reference video is
supported. First/last interpolation is 3–10 seconds; a single first frame can
animate for up to 15 seconds. Audio reference inputs are unsupported.

Google editing accepts a source video up to 15 seconds. Extension adds 3–10
seconds per turn, with a 30-second total output bound. Use
`options.generation_config.video_config.task` for an explicitly verified edit
or extension, and optionally `options.previous_interaction_id` for a stored
interaction. System instructions, temperature, top_p and stop sequences are
rejected for this model as documented by Google.

Fal's reviewed reference endpoint accepts ten images and three videos of up to
15 seconds each. The adapter selects text-to-video, image-to-video or
reference-to-video without dropping media. Its separate edit, extend and
standalone upscale endpoints are not added by this generation contract.
OpenRouter's entry covers the reviewed video catalog's frame-image interface.
The generation tiers include the provider's 1080p and 4K upsampling.

The coordinator prefers the exact Omni 1.2 native Google contract for compatible
Fal/OpenRouter generation requests. It preserves duration, aspect, resolution,
reference manifests and OpenRouter seed. A reference that cannot qualify for
Google keeps the requested provider route; it is never dropped or truncated.
Fallback remains subject to the existing submission-certainty checks.

Omni 1.1 IDs, canonical inputs, pricing hashes and offers stay separate. New
models require an approved signed worker advertising the exact contract and a
new accepted offer. After publication, miners opting into these models must
run `everycli worker-update`, apply their provider key if needed, then review
`everycli health` and `everycli offers`. A CLI-only update does not upgrade a VM.

Provider schemas and rates were reviewed on 2026-11-07:

- [Google Omni guide](https://ai.google.dev/gemini-api/docs/omni)
- [Google pricing](https://ai.google.dev/gemini-api/docs/pricing#gemini-omni-1.2-flash)
- [Fal generation schema](https://fal.ai/models/google/gemini-omni-flash/v1.2/text-to-video/api)
- [Fal reference schema](https://fal.ai/models/google/gemini-omni-flash/v1.2/reference-to-video/api)
- [OpenRouter video catalog](https://openrouter.ai/api/v1/videos/models)
