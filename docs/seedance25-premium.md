# Seedance 2.5 Premium through BytePlus

The `direct/byteplus/seedance-2.5-premium` contract uses
`dreamina-seedance-2-5-premium-260915` on BytePlus ModelArk. It is a separate
model from Standard Seedance 2.5, Seedance 2.0 and Flash. It uses the existing
`BYTEPLUS_API_KEY` provider configuration and requires BytePlus account approval.

Normal final renders use `resolution: "4k"` and run for 4–30 seconds. `duration:
-1` lets the provider choose the length. The reviewed BytePlus ComfyUI registry
lists 4K as Premium's normal output; lower-resolution Draft generation is a
separate workflow, not a final render option on this contract.

```json
{
  "model": "direct/byteplus/seedance-2.5-premium",
  "prompt": "A cinematic tracking shot across a quiet lake",
  "parameters": {
    "duration": 8,
    "resolution": "4k",
    "aspect_ratio": "16:9",
    "generate_audio": true,
    "omni_reference_task_type": "reference",
    "output_format": "mp4"
  },
  "references": []
}
```

Supported inputs include text, a first frame, first/last frames, and up to 50
references: 30 images, 10 video clips and 10 audio clips. Audio-only references
are supported. Each video/audio category has a 30-second total duration limit.
References must be verified EveryFrame attachments. Frames cannot be combined
with the separate omni reference mode. First/last frames require `aspect_ratio:
"adaptive"` because the provider derives framing from the first image.

`omni_reference_task_type` supports `auto`, `reference`, `edit` and `extend`.
Edit/extend require a video and an adaptive aspect. Editing also requires
`duration: -1`; its source must be at least four seconds long. Extension returns
only the requested newly generated segment. With a video and `auto`, use both
`duration: -1` and `aspect_ratio: "adaptive"`, because prompt intent may select
editing. Choose `reference` explicitly to keep a requested duration.

MP4 and MOV output are preserved through the worker's structured media bundle.
The coordinator checks actual decoded 4K dimensions, aspect, length and audio.
Existing task IDs survive worker restart; recovery polls the same task rather
than submitting another generation. Result downloads use the reviewed BytePlus
hosts and carry no provider API credentials.

## Access and pricing

This source addition does not publish a release or activate the model. Premium
has no verified public price in this integration. It is listed as a candidate,
requires an account quote, and cannot receive an offer or unpriced job. Standard
Seedance rates are never reused. The app exposes it in pending discovery rather
than the list of selectable, priced models.

Before production activation, verify BytePlus approval and the Premium account
rate, add a reviewed quote bound to this exact contract, validate a real approved
job, and publish a signed worker release. A paid test needs a separate spend
approval. Changing an environment variable alone does not bypass the quote or
signed-worker checks.

Sources:

- [BytePlus model configuration](https://github.com/byteplus-sa/ComfyUI-BytePlus-ModelArk/blob/main/nodes/models_config.py)
- [BytePlus request and reference handling](https://github.com/byteplus-sa/ComfyUI-BytePlus-ModelArk/blob/main/nodes/nodes_seedance2.py)
- [BytePlus MCP API reference](https://github.com/byteplus-sa/ark-mcp/blob/main/docs/api-reference.md#4-seedance_create_task)
