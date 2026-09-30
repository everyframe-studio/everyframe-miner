# Compatibility

Ported from `codex/github-preparation/everyframe-miner`, worker protocol v1 and
Python `everycli` 0.5.0. The original repository remains separate and unchanged.
Package versions are independent of the original CLI. v0.1.3 adds hotkey-backed
authentication and local onboarding commands; publishing a CLI still does not
admit a new worker image.

- All 45 immutable model hashes match `tests/fixtures/compatibility.json`.
- Canonical signatures sort object keys by UTF-16 code units, matching JavaScript.
- Ed25519 signatures and SPKI keys match the original implementation.
- Job encryption uses X25519, HKDF-SHA256, AES-256-GCM, and session/nonce AAD.
- Phala environment encryption uses its separate direct-X25519/AES-GCM format.
- Reports use the same domain-separated SHA-512 attestation binding.
- Mainnet/testnet pins, provider credentials, CLI flags, profile paths, state-file
  schema, public/private image policies, and lifecycle journal phases are retained.
- Provider POSTs and cloud mutations are never automatically retried. An ambiguous
  synchronous speech response is not regenerated after restart.

Intentional differences: native Rust CLI instead of Python, compact or pretty JSON
display instead of Python's human formatting, stricter malformed Base64/key
validation, rejection of JSON with unpaired Unicode surrogates, and fail-closed
handling of malformed credential-file syntax. Ordinary UTF-8 prompts and existing
signed invitations are supported. Error wording is redacted and may differ.

Hotkey deployments use `everyframe-miner-deployment-v2`, `authMode: hotkey-v1`,
and a positive `keyVersion` instead of `tokenHash`. Their allowed environment
contains `MINER_AUTH`, not `MINER_TOKEN`. A hotkey-capable coordinator and worker
are required. Legacy v1 profiles are retained for staged migration; mixing the two
environment/authentication formats is rejected. New wallet flags only read local
hotkeys and do not introduce blockchain transaction signing.

`register-hotkey` stores a separate public `registration.json` after a finalized
chain lookup. It is not an authorization credential. `set-api-keys` and
`remove-api-key` update the existing private credential store without changing
delegates or a running worker. `init` can now use saved credentials instead of a
required dotenv import; supplied entries merge rather than erase omitted keys.
`apply-api-keys` explicitly reuses the attested activation/restart flow for an
already activated workload. Existing signed configurations, legacy imports, and
lifecycle intent phases remain supported.

The worker's trust pins and model registry are compiled into the executable.
Editing a runtime-mounted JSON file cannot alter them. Rebuild and obtain approval
for every change. Existing image digests and TEE measurements cannot be reused.

Local tests do not prove live provider availability, live Phala compatibility, or
production TEE acceptance. Those require a separately approved canary rollout.
