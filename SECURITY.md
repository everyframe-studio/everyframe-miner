# Security and operational boundaries

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
