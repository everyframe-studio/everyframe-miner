# Compatibility

Ported from `codex/github-preparation/everyframe-miner`, worker protocol v1 and
Python `everycli` 0.5.0. The original repository remains separate and unchanged.
The Rust package starts at 0.1.0; this is a different implementation, not a claim
to be an already published or admitted release.

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

The worker's trust pins and model registry are compiled into the executable.
Editing a runtime-mounted JSON file cannot alter them. Rebuild and obtain approval
for every change. Existing image digests and TEE measurements cannot be reused.

Local tests do not prove live provider availability, live Phala compatibility, or
production TEE acceptance. Those require a separately approved canary rollout.
