# Google direct media provider

The Google provider uses `GEMINI_API_KEY` and the Gemini Interactions API.
The defaults below are starting settings; callers can change them for each job.

| Model ID | Supported controls |
|---|---|
| `direct/google/gemini-omni-1.2-flash` | 360p–4K; landscape/portrait; 3–15 whole seconds; six images, three short video references (one for 11–15 seconds); first/last interpolation up to 10 seconds; editing and extension up to 30 seconds total. See [Omni 1.2](GEMINI-OMNI12.md). |
| `direct/google/gemini-omni-flash` | 360p–4K; landscape/portrait; 3–10-second generation; image/video references; first/last frames; video editing and extension up to Google's 40-second total |
| `direct/google/nano-banana-2.1` | 1K/2K/4K; 14 aspect ratios; up to 14 image references; video/PDF inputs; Search grounding |
| `direct/google/nano-banana-2-lite` | 1K; aspect-ratio selection; image/video/PDF inputs |
| `direct/google/nano-banana-2` | 512px/1K/2K/4K; 14 aspect ratios; image/video/PDF inputs |
| `direct/google/nano-banana-pro` | 1K/2K/4K; 10 aspect ratios; up to 14 image references; Search grounding |
| `direct/google/lyria-3.5` | Full songs; text and up to 10 image references; MP3 or WAV |
| `direct/google/gemini-3.8-flash-tts` | Voice/language selection; custom voices; expressive speech turns; two-speaker dialogue; six audio formats |
| `direct/google/gemini-3.8-flash-lite-tts` | The same speech controls with the Flash-Lite model |

## Request settings

Use the coordinator's normal job input, with the Google model ID, `prompt`, and
optional `resolution`, `aspect`, `duration` (video seconds), `seed`, `references`,
`turns`, and `options`. `options.system_instruction` supplies optional generation guidance.
`aspect: "auto"` lets the provider choose. Defaults are
5-second 720p landscape video, 1K square JPEG images, MP3 music, and 24 kHz WAV
speech with Kore. Reference jobs do not force a square or landscape aspect.

`options.response_format` accepts the model's native media fields: `delivery`,
`image_size`, `aspect_ratio`, `duration`, `resolution`, `mime_type`, `sample_rate`,
and `bit_rate`, as applicable. An array containing a text and media format keeps
both the accompanying text and generated media.

For image output, omit `delivery`. The Gemini endpoint returns image bytes
inline by default and rejects an explicit `delivery: "inline"`. The worker
omits that field from the outgoing image request, including text/image format
arrays. Existing signed job inputs, hashes and recovery references are kept
intact. Explicit `delivery: "uri"` requests are not converted to inline and
may be rejected by the provider. Video and audio delivery settings are unchanged.

`options.generation_config` accepts `seed`, `stop_sequences`, `thinking_level`,
`thinking_summaries`, `max_output_tokens`, `video_config`, `speech_config` (or
`structured_speech_config`), and `tool_choice`. Google's deprecated
`temperature` and `top_p` are also passed through when explicitly supplied.
No output-token cap is forced by the default contract; an explicit cap is useful
when a caller wants a smaller spending bound.

For example, a portrait clip from first/last image references:

```json
{
  "model": "direct/google/gemini-omni-flash",
  "prompt": "Move smoothly between these frames.",
  "resolution": "1080p",
  "aspect": "9:16",
  "duration": 8,
  "references": ["<first-image manifest>", "<last-image manifest>"],
  "options": {"generation_config": {"video_config": {"task": "image_to_video"}}}
}
```

Replace each manifest placeholder with an object containing `url`, `sha256`,
`bytes`, `mime`, `role`, `width`, `height`, `durationMs`, and `frameCount`. Roles
are `image`, `first`, `last`, `video`, or `document`. Reference URLs must be owned
EveryFrame attachments. The worker verifies their bytes before uploading them
to Google. The selected Google model determines valid media types, reference
counts, sizes and durations.

Set `options.generation_config.video_config.task` to `edit` or `extend` for an
uploaded source video. For continuation of an existing Google generation, use
`options.previous_interaction_id` from that same Google account. First/last
frames use the first and second images respectively. Speech `turns` contain
`text`, optional `speaker`, and optional `style`; their text forms the prompt.
Google supports two prebuilt voices in one multi-speaker request. Custom voice
IDs are accepted for single-speaker requests.

Speech output formats are `audio/wav`, `audio/mp3`, `audio/ogg_opus`,
`audio/l16`, `audio/alaw`, and `audio/mulaw`. Raw audio remains in its requested
format; validation uses a temporary decoding wrapper. Google Search is enabled
with `options.tools: [{"type":"google_search"}]` on supported image models.
Multiple output media and text are preserved in an
`application/vnd.everyframe.google+json` bundle with base64 media parts and
text. Single-media jobs retain their native output bytes.

## Offers, recovery and pricing

Account access is checked through the authenticated Google model catalog.
Saving a key does not guarantee paid-tier access. Models missing from that
account remain unavailable; a failed catalog check does not retire them globally.

Offers start disabled with the dashboard's neutral 0% discount. The Models page
shows the model's supported settings. A compatible signed worker and its
coordinator authorization are required; updating only the CLI is insufficient.

Each interaction is stored at Google so a restarted worker can resume the same
request. Saved references bind the input hash and interaction ID. A lost
submission response remains uncertain and is never automatically submitted
again. Output validation follows the requested media format, dimensions,
sample rate and duration.

Pricing uses Google's Standard tier: Lyria per song, and conservative token
bounds for other models. Each job is repriced from its actual settings,
references, output-token budget and grounding. The displayed base reward is an
example, not an invoice. Without an explicit token budget, the reservation uses
the model's full output allowance. Grounding has a separate query charge.
Quote expiry and signed pricing hashes apply. Existing provider contracts and
bids are unchanged. Google API limits and EveryFrame's transport and media
validation bounds still apply.

Sources reviewed November 2, 2026:

- https://ai.google.dev/api/interactions-api
- https://ai.google.dev/gemini-api/docs/omni
- https://ai.google.dev/gemini-api/docs/image-generation
- https://ai.google.dev/gemini-api/docs/music-generation
- https://ai.google.dev/gemini-api/docs/speech-generation
- https://ai.google.dev/gemini-api/docs/pricing
