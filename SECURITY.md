# Security and operational boundaries

## CLI distribution

`everycli update` is independent of miner state and uses only the fixed public
GitHub repository. HTTPS redirects are restricted to GitHub release hosts,
downloads are bounded, and checksums are verified before an atomic replacement.
It does not read miner profiles or provider credentials. Installation and update
share a per-install-directory lock. A stale `.everycli-update.lock` after a crash
must only be removed after verifying no installer or updater is running.

Release checksums are obtained from the same repository as the binaries; they
are not independent code signatures. GitHub account, release workflow, and tag
security remain part of the trust boundary. The installer executes a published
shell script and modifies shell PATH configuration unless explicitly opted out.
CLI self-updates do not alter or bypass signed worker image admission.

## Miner runtime

### Local onboarding and credential changes (v0.1.3)

`register-hotkey` queries fixed public Bittensor HTTPS RPCs, checks the pinned
genesis, and reads both hotkey-to-UID and UID-to-hotkey storage at one finalized
block. It relies on that RPC's response (not a locally verified storage proof).
It does not sign transactions, prove ownership, or grant coordinator admission.
Public-address mode never reads a wallet. Wallet mode reads only the specified
owner-only hotkey JSON, records its public address/path, and never uploads it.

`set-api-keys` accepts hidden terminal input or a bounded, explicit stdin pipe for
one provider; secret values are not accepted as command arguments. Blank terminal
input keeps existing values. Updates preserve unrelated keys/delegates and use
the existing private, atomic, locked state store. Profiles are plaintext on disk
with owner-only permissions, not an encrypted vault. Key presence is safe to show;
values are never printed. Local key removal is not upstream revocation.

Saved changes never mutate a running workload. `apply-api-keys` is a separate,
confirmed operation that reuses exact-policy admission, idle drain, pinned KMS
encryption, durable restart intent, and post-restart admission checks. It cannot
enable providers absent from the signed deployment configuration. Retiring the
last generation credential requires stopping the worker rather than applying an
empty provider configuration.

### Hotkey authentication (v0.1.3)

The CLI reads an owner-only, unencrypted sr25519 hotkey JSON locally during
initialization. It verifies the SS58 checksum and matches the derived public key
to the coordinator-signed deployment configuration. It does not access the
coldkey or submit chain transactions. It creates separate Ed25519 delegates for
console operations and worker enrollment/job operations, signed by the hotkey
with an application-specific domain, exact network/genesis/netuid, coordinator
audience, miner identity, key version, scope, and expiry (maximum 30 days).

Only the worker delegate is included in Phala's encrypted environment as
`MINER_AUTH`; neither the hotkey nor the console delegate is uploaded. A delegate
is still a secret: protect the CLI profile and encrypted workload environment.
Every authenticated request signs its method, path/query, body hash, timestamp,
and single-use nonce. The coordinator checks the approved hotkey binding and key
version on each request, rejects replay/stale requests, and separates console
from worker authority. Migrating a binding disables its legacy bearer token and
revokes existing worker sessions. TEE admission and job receipt verification are
still required; a registered hotkey alone does not approve an arbitrary image.

Deployment configuration signing and hotkey-to-miner binding remain operator
actions after registration/ownership checks. This change is authentication, not
permissionless on-chain registration or automatic TEE policy approval. Existing
token deployments retain compatibility until explicitly migrated. Keep their
tokens until the coordinator binding, worker image, and profile are migrated.

### Runtime isolation

The worker has no HTTP server, arbitrary signing endpoint, wallet seed, external
adapter loader, endpoint override, or provider fallback. Only the reviewed serial
worker state machine creates receipts. Provider output references are bound to
their contract and provider namespace.

HTTPS endpoints are allowlisted. DNS answers must be public and are pinned to the
connection, proxies and implicit retries are disabled, TLS uses bundled public
roots, and responses have size/time bounds. Media redirects are opt-in and strip
credentials before each next hop. TLS and provider authentication are not disabled
for testing.

Credentials are only read from owner-only regular files without symlink traversal.
State writes use private temporary files, fsync, and atomic rename. A per-profile
lock prevents concurrent lifecycle operations. A crash can leave `operation.lock`:
check the recorded PID and cloud/coordinator state before manually removing a
stale lock. Never remove a lock while an operation is active.

Cloud actions write their intent before mutation. A timeout can mean that a paid
operation succeeded remotely; run `reconcile` and consult the operator rather than
blindly retrying. Some ambiguous operations intentionally require manual review.
Stopping a VM is not deleting it, and storage can still be charged.

No provider key is released until fresh exact-policy attestation. Updating an image
disables provider keys and requires fresh admission and activation. Mainnet and
testnet signatures are not interchangeable.

Secrets necessarily exist in process memory while being used. This code does not
claim complete memory zeroization, protection from root, or a substitute for an
independent security audit. Do not expose core dumps, privileged debugger access,
container logs containing custom instrumentation, or host-level TLS interception.

Report vulnerabilities privately to the repository owner through an established
channel; do not post tokens, invitations, credentials, or exploit details publicly.
The owner must publish a verified private reporting contact before public release.
