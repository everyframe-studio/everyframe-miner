# Kling 4.0 through Fal

Kling 4.0 uses your existing Fal generation key (`FAL_KEY`). It is separate from
Kling's direct developer API and does not use `KLING_API_KEY`.

The model contract is `studio/fal/kling/v4/text-to-video`. It selects Fal's
text-to-video, image-to-video, or reference-to-video endpoint from the signed
job's inputs:

| Setting | Supported |
| --- | --- |
| Duration | 3–30 whole seconds |
| Resolution | 720p, 1080p |
| Text/reference aspect ratios | 16:9, 9:16, 1:1, 21:9 |
| First/last frames | One first frame and optional last frame; output follows the first frame |
| Reference inputs | Up to 10 images and 5 videos, 15 files total |
| Reference video duration | 3–30 seconds per video, 30 seconds combined |
| Prompt | Up to 26,000 UTF-16 code units |

A first frame can accompany reference inputs and counts toward the 10-image
limit. An ending frame cannot accompany reference inputs. Audio-reference files,
voice elements, structured multi-shot prompts, and video editing are separate
provider features and are not part of this contract.

The worker retains its existing bounded attachment limits: 12 MiB per image,
64 MiB per video, and 128 MiB for the reference set. Images must be at least
300×300; videos must be 700–4553 pixels per dimension. Reference aspect ratios
must be between 0.4 and 2.5. A job has a 20-minute processing deadline, with the
usual deadline check before paid submission and no automatic resubmission after
an uncertain provider response.

## Install and offer

```sh
everycli worker-update --check
everycli worker-update
everycli health
everycli offers
```

An enrolled automatic updater can install a compatible signed worker update when
idle. In `everycli ui`, open **All models**, select **Kling 4.0**, review the price,
and set your discount. Existing discounts are retained; a new model needs its own
offer. Everycli v0.7.2 and later can discover new coordinator models; updating the
CLI alone does not install a worker adapter.

## Pricing

Text, first/last-frame, and image-reference generation uses $0.157 per output
second at 720p or $0.235 at 1080p. With video references, the rate is $0.094 or
$0.141 per second respectively, applied to input plus output duration, with a
minimum of twice the output duration. The coordinator includes that minimum in
the quote and rounds up only to the nearest micro-dollar.

For example, a 10-second 720p output with a 3-second reference is priced for
20 seconds at $0.094, or $1.88 before the miner's chosen discount.

Sources: [Fal model and prices](https://fal.ai/models/kling/v4/text-to-video),
[reference-video pricing](https://fal.ai/models/kling/v4/reference-to-video),
[API schemas](https://fal.ai/models/kling/v4/reference-to-video/api).
