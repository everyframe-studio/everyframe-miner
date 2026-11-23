# everycli v1.3.0

A compatible signed worker can handle up to five generation jobs at once when the coordinator enables parallel scheduling for that miner. Each provider account has its own concurrency limit. For example, an account with two Fal slots can run two Fal jobs while the miner uses its three remaining slots for eligible jobs from other providers.

In `everycli ui`, the overview groups models that need attention and links to a filtered list in Models. Confirmation dialogs keep the Confirm and Cancel buttons visible while you scroll through long batch reviews.

This release also includes the v1.2.7 provider additions for Vidu, Stability AI, and WaveSpeedAI.

Run `everycli update` and reopen `everycli ui` if it is running. Use `everycli worker-update --check` to check whether the compatible signed worker is available for your miner, then `everycli worker-update` to install it.
