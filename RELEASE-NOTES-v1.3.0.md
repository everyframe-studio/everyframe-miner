# everycli v1.3.0

A compatible signed worker can handle up to five generation jobs at once when the coordinator enables parallel scheduling for that miner. Each provider account has its own concurrency limit. For example, an account with two Fal slots can run two Fal jobs while the miner uses its three remaining slots for eligible jobs from other providers.

Run `everycli update` and reopen `everycli ui` if it is running. Use `everycli worker-update --check` to check whether the compatible signed worker is available for your miner, then `everycli worker-update` to install it.
