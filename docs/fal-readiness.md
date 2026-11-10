# Fal readiness in v1.0.1

Workers no longer query `api.fal.ai/v1/models`. That public catalog describes
models, not the access granted to the worker's key. Many workers behind one
outbound address could exhaust its request limit, making every Fal model appear
unavailable even though generation was working.

The worker now checks its normal Fal key using the same authenticated, read-only
account endpoint used when saving a key. It caches that result for 15 minutes,
independently of retries for other providers, and rechecks when the key changes.
The periodic check has a 10-second timeout. It never submits a generation.

- A successful key check permits the reviewed models to be advertised.
- A confirmed authentication rejection blocks Fal work until a successful check
  or a key change. A later timeout does not erase that rejection.
- A timeout, rate limit, server error or inconclusive response does not turn
  every Fal model into an availability failure.
- Retirements from the reviewed release or signed coordinator list remain
  blocked. No new model, price or permission is inferred from key validity.

The UI and CLI show “Available · no per-model check” without a warning for this
expected state. This is not a promise that every request will succeed. Provider
restrictions, billing and content decisions are still reported when they occur.

The coordinator also stops polling Fal's public catalog and ignores old
non-retirement catalog snapshots as readiness evidence. It preserves confirmed
retirements and still respects worker-reported authentication failures.
Existing workers must install the compatible signed update to stop their old
catalog polling. Updating the local CLI alone does not change their behavior.

Fal automatic-offer pricing and balance queries remain separate, authenticated
checks. Their prices, limits and failure handling are unchanged by this fix.
