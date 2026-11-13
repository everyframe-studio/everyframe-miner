# Veo 3.1 negative prompts

The versioned `studio/openrouter/google/veo-3.1-negative` and
`studio/fal/fal-ai/veo3.1-negative` contracts carry a required `negative` string
alongside the positive prompt. The string is part of the signed input and is
forwarded unchanged, including whitespace and Unicode. The existing Veo
contracts, their hashes and their offers remain unchanged.

OpenRouter maps the string to
`provider.options.google-vertex.parameters.negativePrompt`. Fal maps it to
`negative_prompt` on the text, first-frame, or first/last-frame endpoint.
These contracts retain the reviewed 4, 6 and 8 second, 720p/1080p settings and
native audio. Negative-prompt support does not change output pricing.

This is an execution capability upgrade. A signed catalog cannot teach an old
worker how to transmit a new field. The `negativePrompt` policy flag prevents
older templates from adopting these contracts. Deploy the compatible worker,
then explicitly offer the new contract. No existing bid is widened or enabled
automatically. An old worker never receives a job with a field it would ignore.

The backend chooses this contract only for Veo requests with negative prompts.
Provider alternatives must retain the complete text, settings and references.
Frame routes cross providers only when source and requested aspect match.

Provider references, checked 2026-11-13:
- https://openrouter.ai/docs/cookbook/video-generation/provider-specific-video-options
- https://openrouter.ai/api/v1/videos/models
- https://fal.ai/models/fal-ai/veo3.1/api
- https://fal.ai/models/fal-ai/veo3.1/image-to-video/api
- https://fal.ai/models/fal-ai/veo3.1/first-last-frame-to-video/api
