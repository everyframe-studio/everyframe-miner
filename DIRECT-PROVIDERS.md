# Direct providers

Alibaba Cloud, BytePlus and Volcengine setup and supported models are documented in [ASIAN-PROVIDERS.md](ASIAN-PROVIDERS.md).


The dashboard lists direct providers first. Open the API keys page to save a
provider key, update the worker when required, and review readiness in Models.
Use each provider switch or individual model offers to choose the work you
accept. New offers default to a neutral 0% discount.

| Provider | Native model families | Key |
| --- | --- | --- |
| OpenAI | GPT Image 2.5 Sunburst and Flare, GPT Image 2, GPT Audio 1.5 | `OPENAI_API_KEY` |
| Google | Gemini Omni Flash, Nano Banana, Lyria, Gemini speech | `GEMINI_API_KEY` |
| xAI | Grok Imagine images, video, video edits and extensions, speech | `XAI_API_KEY` |
| Black Forest Labs | FLUX 1, FLUX.2, FLUX 3 images and video, Kontext, Fill | `BFL_API_KEY` |
| MiniMax | H3, H3 Highspeed, H3 Lite, H3 Max, Image 01, Speech | `MINIMAX_API_KEY` |
| Kling | Kling 4.0, 3.0, 3.0 Turbo, 3.0 Omni, O1, 2.6, 2.5 Turbo; Kling images | `KLING_API_KEY` |
| ElevenLabs | v4 dialogue, v4 Turbo, v3, v3 Conversational, Multilingual v2, Flash, sound effects, music | `ELEVENLABS_API_KEY` |
| Runway | Gen-4.5, Gen-4 Turbo, Gen-4 images, Muse, Aleph 2, Act-Two | `RUNWAYML_API_SECRET` |
| Luma Agents | UNI-1, UNI-1 Max, Ray 3.2 | `LUMA_AGENTS_API_KEY` |

Pika, Vivix and the older Luma API retain their existing integrations. Luma
Agents is a separate API and key. A model's presence does not grant your
provider account access to it; the Models page shows account and worker
readiness separately.

Model details show the available controls. Providers differ in image counts,
voices, resolution, duration, reference types and editing features. Each job's
quote uses its own settings and inputs. The displayed base reward is a reference
quote, not a provider invoice.

Fal, OpenRouter and Replicate remain explicit alternatives. The dashboard does
not resubmit a failed or uncertain generation through another provider.
