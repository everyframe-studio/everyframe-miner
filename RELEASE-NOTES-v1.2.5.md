# everycli v1.2.5

- Fixes Phala KMS lookup failures when applying provider keys or updating an existing worker after an OS migration.
- Verifies the app’s encryption key before draining and returns a failure status when key application fails.
- Keeps the environment allowlist in signed order when applying a worker update so Phala starts the approved compose.

Run `everycli update`. If a previous attempt left new provider keys saved locally, run `everycli apply-api-keys` in the same profile. After the VM reattests, run `everycli resume` to accept work. A healthy miner with no pending key changes can continue serving after the CLI update.
