# Video models added in v0.7.4

These models use your existing **Fal API key**. Set discounts independently in
**All models**, or use `everycli offers`. Existing offers are unchanged.

| Model | Clip length | Resolutions | Inputs |
| --- | --- | --- | --- |
| Kling 4.0 | 3–30 seconds | 720p, 1080p | Text, first/last frames, images and videos |
| HeyGen Video | 5–15 seconds | 480p, 768p, 1080p | Text, first frame, image/video/audio references |
| Gemini Omni Flash 1.1 | 3–10 seconds | 360p, 720p, 1080p, 4K | Text, first/last frames, image/video references |
| Vidu Q4 Preview | 3–16 seconds | 540p, 720p, 1080p | First frame or image references, optional voice references |

Vidu requires an image. Its voice references must be MP3, 3–12 seconds each,
with up to three clips. It accepts up to 12 reference images. Reference mode
includes generated audio. First-frame mode follows the source image framing.

Gemini supports up to ten image references and three video references, each at
most ten seconds. First/last frames form a separate mode. The selected aspect
ratio is sent explicitly. Gemini does not accept a seed on these endpoints.

HeyGen accepts up to nine images, three videos and three audio references.
References and first-frame guidance are separate modes. Prompt expansion maps
to `turbo`, or `disabled` when disabled. Safety screening remains enabled.

Worker reference limits also apply: 12 MiB per image, 64 MiB per video, 20 MiB
per audio file, and 128 MiB combined. Sources are verified against their signed
hashes before paid submission. Provider keys are never sent to reference or
output download hosts.

## Install the worker adapter

```sh
everycli update
everycli worker-update --check
everycli worker-update
everycli offers
```

Enrolled automatic worker updates can install the signed package when idle.
Updating the CLI alone does not change a deployed worker. Wait for fresh
attestation, then choose the new models and set their discounts.

Kling's reference-video pricing uses both input and output duration and its
minimum billable duration. HeyGen's promotional price ends on October 31,
2026; the price fingerprint changes afterward, requiring a current offer.
See [Kling formats and limits](KLING4.md).
