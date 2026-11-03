# Everycli v0.9.3 — More OpenRouter video routes

Add OpenRouter offers for Wan 3.0 and Grok Imagine Video. Compatible Studio requests can use these offers and the existing OpenRouter Seedance 2.5 route, with native providers first and Fal retained as the original route.

Each provider keeps its own model contract, pricing and offer. Existing Fal discounts are unchanged. Unsupported resolutions, reference modes and model versions are never silently substituted.

Update with `everycli update`, then `everycli worker-update` on the existing management profile. Review `everycli offers` before enabling the new offers. Updating the CLI alone does not update a running worker.

See [OpenRouter video routes](OPENROUTER.md) for supported settings and compatibility limits.
