# Everycli v0.9.0 — Provider catalog and Google media

- Adds reviewed pricing for 30 more catalog models across Fal, Kling, MiniMax,
  OpenRouter, Black Forest Labs, Replicate, Runway and ElevenLabs.
- Restores the four Vivix base offers after the reference promotion ended.
- Keeps retired models out of new offers.
- Adds direct Google support for Gemini Omni Flash, Nano Banana 2.1, Nano
  Banana 2 Lite, Nano Banana 2, Nano Banana Pro, Lyria 3.5, Gemini 3.8 Flash
  TTS and Gemini 3.8 Flash-Lite TTS.
- Adds selectable sizes, aspect ratios, reference inputs, video editing and
  extension, speech voices and multi-speaker output, and native audio formats.
- Checks model access using your Google API key.
- Resumes Google requests by their saved interaction ID after interruption.
- Uses the existing provider offer switches, neutral 0% defaults and explicit
  model pricing.

See [Google formats and setup](GOOGLE.md). Google media requires a compatible
signed worker update, a Google API key with access to the selected model, and
an enabled offer. Updating the CLI alone does not upgrade your deployed worker.

Run `everycli update`, reopen `everycli ui`, then use **Worker software →
Update worker** (or `everycli worker-update`). Once the worker is ready, refresh
Models and enable the offers you want. Provider switches preserve saved
discounts; newly enabled offers default to 0%.
