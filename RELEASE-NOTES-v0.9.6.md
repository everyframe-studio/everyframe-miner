# v0.9.6 — More OpenRouter video models

Add 14 OpenRouter video offers: Grok Imagine Video 1.5 and 1.5 Lite, H3 Max,
Wan 3.0 Prime, Runway Gen-4.5, HappyHorse 1.0 and 1.1, Kling 3.0 Pro and
Standard, Kling O1, Hailuo 2.3, Wan 2.7 and 2.6, and FLUX 3 Video.

Each offer uses the model's supported durations, resolutions and keyframes.
Kling 3.0 includes audio in its rate; Wan 2.6 accounts for image-guided pricing;
Grok 1.5 accounts for image inputs. Models with fixed duration choices reject
unsupported lengths before submission.

Update with `everycli update`, then `everycli worker-update` from the existing
management profile. Review the new offers in `everycli ui` or `everycli offers`
and accept the ones you want to serve. Your existing bids and discounts remain
unchanged. See [the OpenRouter guide](OPENROUTER.md) for supported settings.

This release also includes the BFL polling and task-recovery improvements from
v0.9.5.
