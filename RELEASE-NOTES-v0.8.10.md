# Everycli v0.8.10 — Google direct media

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
