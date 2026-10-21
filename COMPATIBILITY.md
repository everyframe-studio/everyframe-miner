# Compatibility

## v0.6.3 offline automatic updates (release candidate)

Requires the matching coordinator's signed recovery, capsule-bound enrollment,
bounded single-flight lease and explicitly exposed updater routes. A one-container
deployment cannot enroll: first install the signed two-container bootstrap with
`everycli worker-update`, then explicitly enable automation. Legacy expiring
delegates must be renewed to supported v2 authorizations before enrollment.

CLI updates alone do not enable automation or install the supervisor. The
supervisor uses the same VM and preserves its own independent image pin, OS,
resources and KMS. Fresh hardware attestation is mandatory before restoring
provider credentials and accepting work. A stale, disabled or replaced capsule
cannot report progress or take over a newer deployment. No model contract,
offer, credit price or validator accounting policy changes in this feature.


The v0.6.0 worker reports an output the coordinator refuses twice (HTTP
400/413/422 on `complete`) as `failed` with reason `output_invalid`, instead of
resending it until the deadline. The coordinator already accepts that reason
(GENERATION_FAILURES). A 401/409 on `complete` now ends the attempt as
`reconcile`. Claims may omit a provider's models for 15 minutes after a
billing, credential or missing-key refusal. No protocol fields were added.

The coordinator prefers eligible workers on the published major/minor
(0.6.x once the v0.6.0 worker release is published), ignoring patch versions.
If no current-minor worker can serve a particular job, the previous published
minor (0.5.x) is used as fallback, without a fixed expiry. Availability includes
the model, current offer/pricing, attested session, provider readiness and idle
capacity. Routing returns to the current minor as soon as eligible capacity is
available; already assigned jobs are not interrupted. This policy is server-side:
updating the local CLI does not change the VM's worker version.

The v0.5.8 worker adds optional signed `modelAvailability` rows to claim payloads.
Deploy the compatible coordinator before admitting this worker image. Old workers
remain supported without those rows; they are not claimed to have account-level
checks. Reports only narrow the existing model allowlist and contain fixed status
codes, contract IDs and timestamps. Existing contract bodies/hashes remain intact,
including retired contracts needed to validate historical jobs and receipts.
The coordinator must also accept `provider_model_unavailable` as a definitive
pre-submission rejection: the catalog guard has not contacted a paid endpoint,
so another eligible miner may serve the same job without duplicate submission.

Current v0.4.0 adds 11 settings-bound Studio video contracts and 12 Studio media
contracts alongside the 46 legacy contracts. Their IDs, signed settings and
explicit offers are separate from the legacy routes; a legacy text-only offer
does not authorize reference, image or audio jobs. Updating the CLI alone does
not change a deployed worker's permissions. Public compatible updates preserve
the existing VM and require fresh verified attestation before permissions expand.
See [current live verification and limitations](VALIDATION.md#v040-public-worker-and-website-verification--13-october-2026).

The notes below describe the original port and incremental compatibility changes.

## v0.5.7 worker update: categorized failures

The failure-reporting worker requires a coordinator that accepts a fixed `reason`
on the signed `failed` action. Reports never contain provider response text or
echoed inputs. Retries preserve the same attempt and category; a timeout or a
generic HTTP error never downgrades the report to the legacy reasonless format.
An incompatible or unreachable coordinator leaves the operation for reconciliation
at the job deadline, without resubmitting the provider generation. Update the
coordinator before rolling out this worker; publishing only the CLI does not
update existing VMs.

The renewal horizon covers the coordinator's 12-minute maximum job deadline plus
20 seconds. Coordinator sessions must outlive that horizon. Local regression
tests are not evidence that a new worker image has passed production attestation.

## Port history

Ported from `codex/github-preparation/everyframe-miner`, worker protocol v1 and
Python `everycli` 0.5.0. The original repository remains separate and unchanged.
Package versions are independent of the original CLI. v0.1.3 adds hotkey-backed
authentication and local onboarding commands; publishing a CLI still does not
admit a new worker image.

v0.1.4 makes `init` self-service on SN117. It obtains a signed deployment using
a fresh off-chain ownership proof and finalized membership check; `--invitation`
is now only an optional legacy import. Deploy/reconcile bind the exact app to the
approved release policy automatically. Existing managed profiles are not migrated.

- All 45 immutable model hashes match `tests/fixtures/compatibility.json`.
- Vivix W1 adds a separate `direct/vivix/vivix-w1` contract and `VIVIX_API_KEY`
  without changing those original hashes. Its 5-15 second text-to-video adapter
  requires a matching coordinator contract and a newly admitted worker image;
  existing signed deployment allowlists remain authoritative. No paid canary or
  live TEE validation is implied by the local adapter tests.
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
