# everycli v1.2.6

Adds a verified encryption key refresh for existing Phala dstack 0.5.15 workers that report `encryption_key_changed`.

1. Run `everycli update`.
2. From the same miner profile, run `everycli reconnect --refresh-encryption-key` and review the prompt. The CLI checks the attested VM and Phala’s signature before saving the new pin locally. The worker can keep serving during this step.
3. Apply your saved provider keys with `everycli apply-api-keys`. After fresh attestation, run `everycli resume`. If you were checking a worker update, you can run `everycli worker-update --check` after the pin refresh.
