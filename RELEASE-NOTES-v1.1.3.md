# everycli v1.1.3

Fixes Google Nano Banana image requests rejected with “Image delivery mode is not supported.”

- Uses Google’s default inline image output for Nano Banana 2.1, Nano Banana 2 Lite, Nano Banana 2 and Nano Banana Pro, including combined text/image formats.
- Preserves signed inputs, job hashes and stored interaction IDs for safe recovery.
- Retains the detailed job and network diagnostics from the current signed worker.

## Upgrade

```sh
everycli update
everycli worker-update
```

Google miners need the worker update for this fix. Updating the CLI alone does not update the deployed worker. Reopen `everycli ui` afterward. Existing offers, discounts and VM identity are retained.
