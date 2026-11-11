# Job diagnostics

Workers send a bounded debug report with a failed job when the coordinator
advertises `workerDiagnostics: 1`. This is automatic and requires no provider-key
permission changes. Older coordinators and workers remain compatible.

Inspect one attempt with `everycli jobs --job-id <48-character-job-id> --json`,
or use the admin diagnostic panel described below. The debug report identifies the exact worker
error, failure stage, provider, HTTP status when available, and the provider task
ID for an input-bound native reference. It also states whether the worker had
already observed provider completion.

The report travels in the signed terminal failure message, so a missing final
heartbeat does not discard it. It does not authorize another generation, change
failure classification, approve payment, or prove provider billing. Reports never
contain prompts, request or response bodies, media, credentials, or signed URLs.

This requires a worker build containing the diagnostics feature. Updating the
CLI alone does not update an already deployed worker. Historical failures cannot
be reconstructed if the old worker never sent the underlying error; use that
worker's retained logs or the provider's task history.

## Coordinator-requested traces

The coordinator may queue a diagnostic request for an exact job and attempt.
Failed and uncertain submissions queue one automatically. A superadmin can also
open a failed generation in **Admin → Generations → Miner diagnostics**, choose
**Request miner diagnostics**, and refresh its status. The private control API
is `POST /control/jobs/:jobId/diagnostics` (optional `attemptId`, `refresh`) and
`GET /control/jobs/:jobId/diagnostics`; both require the existing control token.

Workers advertising `diagnosticPolicy: 1` collect the request on their next idle
claim and reply with a signed `diagnostic_report`. The request is bound to the
assigned miner, job, attempt and input hash. It cannot execute a command, read
arbitrary files, query another miner, change payment, or submit a new generation.
The active generation is never interrupted. Failed delivery is retried by lease.

A worker keeps at most 128 recent jobs in memory, at most 64 sanitized events per
job and 32 KiB per trace, for up to 72 hours. A worker restart clears this cache.
Trace events include generation phases, retries, provider completion, safe error
codes, HTTP status when known, and input-bound provider task IDs. No prompts,
URLs, credentials, raw provider responses or output bytes are collected. The
coordinator expires unanswered requests after 24 hours and retains completed
records for up to seven days. `not_retained` is explicit and never reconstructed.

The published worker image has this feature; it is based on v1.1.2 and preserves
its provider budget safeguards. Existing deployments need `everycli worker-update`
(or their already-enabled worker auto-update). A CLI-only update is insufficient.
Old jobs that never had an in-memory trace remain unavailable after upgrading.

### Rejected destinations

With `destinationDiagnostics: 1` negotiated by the coordinator, a network failure
also reports `destination`: normalized hostname, scheme, effective port, initial
or redirect stage, the failed validation rule, allowed hostnames, presence flags
for userinfo/query/fragment, and a SHA-256 fingerprint of the complete URL.

The URL path, userinfo, query values, fragment and raw URL are never retained.
This distinguishes an unexpected CDN hostname from malformed URLs, non-HTTPS
URLs, credential-bearing URLs, blocked ports, fragments, DNS failures, and private
addresses without leaking a signed download link. Validating or inspecting this
metadata does not fetch a URL or relax download restrictions. Each job resets
its thread-local destination metadata; old traces cannot acquire these new fields.
The worker image must be updated to produce this extension.

Coordinator deployment: the destination validator is deployed before advertising
the new worker image. Existing workers keep their previous schema, and upgraded
workers only attach destination metadata when the signed session advertises the
extension. No independent validator changes or CLI version bump are needed.

### Extended execution details

The `executionDiagnostics: 1` session capability enables one bounded execution
summary on failed jobs and in the retained trace of successful jobs. It includes:

- model, prompt length, safe generation settings, and up to eight reference
  descriptors (type, byte count, dimensions and duration; no asset URLs);
- worker package/build ID, OS/architecture, available CPU count, process RSS,
  available memory and free scratch space at observation time;
- overall elapsed time and up to 12 HTTP observations, preserving the first and
  most recent observations and reporting the count omitted;
- request method/host/size, timeout and response-size limits; response status,
  content type/length, bytes read, a bounded request ID and numeric Retry-After;
- DNS, response-header and total request timings; safe transport error code;
- response fingerprint for successful responses up to 1 MiB, JSON parse-error
  category, JSON top-level type and whitelisted key names, and output count;
- final output byte count and fingerprint, plus image format/dimensions when
  detectable without running extra decoding or provider requests.

These are observations, not provider invoices or evidence of payment. Some
fields are null when unavailable. No extra provider calls are made. Resource
numbers describe the worker, not the provider's inference machines. HTTP data is
collected only inside an active job; no account credentials, request/response
bodies, full URLs, environment variables or cookies are included.

The existing 32 KiB per-trace and three-day / 128-job memory limits still apply;
process restarts erase the local retained trace. The coordinator retains the
signed reports and existing progress events it already received. A killed worker
cannot retrospectively report an unobserved OOM reason.
