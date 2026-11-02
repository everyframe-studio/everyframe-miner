# Everycli v0.9.2 — Direct providers first

Direct providers now appear first in Models, provider switches, API keys and
setup. Fal, OpenRouter and other intermediaries remain available after them.

The worker adds native routes for OpenAI images and audio, xAI Grok images,
video and speech, Black Forest Labs FLUX models, MiniMax H3, images and speech,
Kling, ElevenLabs, Runway and Luma Agents. Model controls follow their provider's
formats and reference-input rules. ElevenLabs v4 uses dialogue generation;
Turbo uses its streaming protocol.

Provider selection stays explicit. Changing display order does not create an
offer or switch a submitted job to another provider. Existing offers and
discounts are preserved.

Update with `everycli update`, reopen `everycli ui`, and use **Update worker**.
Then review provider keys and refresh Models. Luma Agents uses its own
`LUMA_AGENTS_API_KEY`; existing Luma keys remain separate.

See the [direct provider guide](DIRECT-PROVIDERS.md) for the model families and
key names.
