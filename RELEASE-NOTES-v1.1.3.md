# everycli v1.1.3

Adds LTX 2.5 and fixes Google Nano Banana image requests rejected with “Image delivery mode is not supported.”

- Adds 14 LTX 2.5 routes: native LTX and Fal Fast/Pro text-to-video, image-to-video and audio-to-video, plus WaveSpeed text/image generation.
- Preserves each provider’s supported duration, resolution, frame-rate and reference controls. Compatible LTX routes try the native provider first, with safe fallback.
- Uses Google’s default inline image output for Nano Banana 2.1, Nano Banana 2 Lite, Nano Banana 2 and Nano Banana Pro, including combined text/image formats.
- Preserves signed inputs, job hashes and stored interaction IDs for safe recovery.
- Retains the detailed job and network diagnostics from the current signed worker.

## Upgrade

```sh
everycli update
everycli worker-update
```

Run the worker update for LTX support and the Google fix. Native LTX uses `LTX_API_KEY`; Fal and WaveSpeed use their existing provider keys. Updating the CLI alone does not update the deployed worker. Reopen `everycli ui` afterward. Existing offers, discounts and VM identity are retained.
