Seedance 2.0 supports 480p, 720p, 1080p and 4K video, with 4–15 second
durations. It accepts text, an opening frame, first and last frames, or image,
video and audio references. Mini, Fast, Seedance 2.5 and Seedance 2.5 Flash
retain their separate model contracts.

The provider contracts are:

| Provider | Model contract | Key |
| --- | --- | --- |
| BytePlus | `direct/byteplus/seedance-2.0` | `BYTEPLUS_API_KEY` |
| OpenRouter | `studio/openrouter/bytedance/seedance-2.0` | `OPENROUTER_API_KEY` |
| Fal | `studio/fal/bytedance/seedance-2.0/text-to-video` | `FAL_KEY` |

Studio requests prefer BytePlus directly, then a compatible OpenRouter route,
then their original Fal route. Each provider has its own offer and discount.
OpenRouter fallback preserves the requested frame geometry; routes with
different published sizes remain separate. Existing fallback rules still
prevent retrying an uncertain or already-submitted generation.

Reference generation supports up to 12 files: 9 images, 3 videos and 3 audio
clips. Audio references require an image or video. Video and audio references
each have a combined 15-second limit. First/last-frame mode and reference mode
are separate. Fal video references must fit its published 480p–720p input
range, even when the requested output is 4K.

Fal does not publish a seed input in its current OpenAPI schema. That route
does not advertise reproducible seeds. BytePlus and OpenRouter retain their
own seed controls.

List-price estimates use each provider's published token rate and the
requested output geometry. For video references, the reservation covers the
full 15-second input allowance at the undiscounted rate, rather than assuming
a discount or an unverified minimum token charge. Estimates are not provider
invoices.

The app card is **Seedance 2.0 · 4K**, with all four resolution options and a
New badge. New worker contracts require the CLI and signed worker release
that includes this addition; existing workers and offers remain bound to
their existing contracts.

Provider references:

- https://console.byteplus.com/ark/region:ap-southeast-1/docs/create-video-generation-task-api
- https://console.byteplus.com/ark/region:ap-southeast-1/docs/model-pricing
- https://openrouter.ai/api/v1/videos/models
- https://fal.ai/models/bytedance/seedance-2.0/text-to-video/api
- https://fal.ai/models/bytedance/seedance-2.0/reference-to-video/api
