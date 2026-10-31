# Pika H3 Flex

Provider: `pika`. Credential: `PIKA_API_KEY` from the
[Pika developer console](https://dev.pika.art/). This adapter calls Pika's own
H3 Flex model through `https://api.dev.pika.art` using `X-API-Key`.

Use `everycli set-api-keys --provider pika` or the API keys page in `everycli ui`.
The CLI tests the key with `GET /billing/balance` before saving it. `everycli
balances` and the UI show spendable API funds; active postpaid accounts show
their remaining cycle allowance. These checks do not generate media.

`studio/pika/pika-h3-flex` accepts whole-second durations from 5 to 15, a prompt
up to 8,000 UTF-16 units, a seed, an aspect ratio, and 480p, 768p, 1080p, 2K or
4K output. 480p and 768p are native; higher resolutions are upscaled from 768p.
The output is one MP4 with native audio. The existing worker artifact-size and
job-deadline limits also apply. Audio stems and webhooks are separate from this
single-video contract.

| Mode | Pika endpoint | Inputs |
| --- | --- | --- |
| Text | `/v1/media/pika/pika-h3-flex/text-to-video` | Prompt |
| Keyframes | `/v1/media/pika/pika-h3-flex/image-to-video` | First frame, optionally last frame |
| References | `/v1/media/pika/pika-h3-flex/reference-to-video` | Up to 9 images, 3 videos and 3 audio clips; at most 12 references |

Reference mode needs at least one image or video. Keyframes and general
references use separate modes. Each file is limited to 24 MiB and the complete
set to 48 MiB. Images are limited to 40 million pixels; videos to 16 million
pixels per frame. Video/audio clips must each be 2–15 seconds, with a combined
limit of 15 seconds for each type. Source attachments must be immutable
Everyframe URLs; the worker checks the source bytes and hash before submission.

The worker polls the exact returned job ID and recognizes terminal failures
immediately. It uses fixed error codes for billing, capacity, invalid input and
content rejection. A timeout or lost submission response remains uncertain and
does not trigger another paid submission. Provider messages and credentials
are excluded from reports. Output downloads accept only Pika's documented CDN,
without API credentials or redirects.

Pricing references:
[text/keyframes](https://dev.pika.art/models/pika/pika-h3-flex/text-to-video/pricing),
[reference video](https://dev.pika.art/models/pika/pika-h3-flex/reference-to-video/pricing),
and the free `/catalog/apis/{api_id}/quote` endpoint. Reference mode charges for
output seconds plus reference-video seconds. Quotes are estimates; terminal
job billing fields report the provider's settled charge.
