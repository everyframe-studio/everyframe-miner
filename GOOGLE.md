# Google direct media provider

The Google provider uses `GEMINI_API_KEY` and the Gemini Interactions API.
Its current direct contracts are:

| Model ID | Preset |
|---|---|
| `direct/google/gemini-omni-flash` | Gemini Omni Flash 1.1, five seconds, 720p, 16:9 |
| `direct/google/nano-banana-2.1` | Nano Banana 2.1, 1K square JPEG |
| `direct/google/nano-banana-2-lite` | Nano Banana 2 Lite, 1K square JPEG |
| `direct/google/nano-banana-2` | Nano Banana 2, 1K square JPEG |
| `direct/google/nano-banana-pro` | Nano Banana Pro, 1K square JPEG |
| `direct/google/lyria-3.5` | Lyria 3.5, full song, MP3 |
| `direct/google/gemini-3.8-flash-tts` | Gemini 3.8 Flash TTS, Kore voice, 24 kHz WAV |
| `direct/google/gemini-3.8-flash-lite-tts` | Gemini 3.8 Flash-Lite TTS, Kore voice, 24 kHz WAV |

These are text-only presets. They do not enable reference inputs, search tools,
voice cloning, multi-speaker output or arbitrary provider parameters.

The provider saves the interaction at Google so a restarted worker can resume
polling the same ID. References bind the input hash and provider response ID.
A lost submission response remains uncertain and is never automatically
submitted again. Output decoding checks the media type, dimensions, sample rate
and duration before acceptance.

Account availability is checked through the authenticated Google model catalog.
Saving a key does not guarantee paid-tier access. Models missing from that
account remain unavailable; a failed catalog check does not retire them globally.

Offers start disabled and use the dashboard's existing neutral 0% default. A
compatible signed worker release and its coordinator authorization are required
before a miner can offer these contracts. Updating only the CLI is insufficient.

Pricing is reviewed against Google's Standard tier. Lyria uses the per-song
price. Token-billed models use conservative bounds for the fixed input/output
budgets, including thinking, rather than claiming a headline image/video price
is an exact bill. The coordinator's quote expiry and signed pricing hash still
apply. Existing providers and bids retain their contracts and pricing.

Sources reviewed November 2, 2026:

- https://ai.google.dev/api/interactions-api
- https://ai.google.dev/gemini-api/docs/omni
- https://ai.google.dev/gemini-api/docs/image-generation
- https://ai.google.dev/gemini-api/docs/music-generation
- https://ai.google.dev/gemini-api/docs/speech-generation
- https://ai.google.dev/gemini-api/docs/pricing
