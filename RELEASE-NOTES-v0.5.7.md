# everycli v0.5.7

## Provider failure reporting

- Reports fixed moderation, invalid-request, billing, timeout, provider and output
  failure categories instead of treating every terminal error as generic.
- Preserves existing Vivix moderation/parameter errors and Fal exhausted-balance
  errors through the worker's signed failure report.
- Retries retain the same attempt and failure category. Timeouts and generic HTTP
  errors never trigger a reasonless legacy fallback or another paid generation.
- Structured error codes/types take precedence over message text. Classification
  ignores echoed input, prompts, request objects and validation context. Generic
  resource shortages are not automatically classified as exhausted credits.
- Only fixed categories are reported; raw provider messages remain private.

## Longer-job sessions

Renewal before claiming work now accounts for the coordinator's 12-minute maximum
job deadline plus a 20-second margin. A compatible coordinator must issue sessions
longer than that horizon. This does not increase model duration limits or prices.

## Upgrade

```sh
everycli update
everycli worker-update --check
```

The native CLI release includes the worker source, but does not replace running
VMs. The worker fixes require a separately verified signed worker release. When
the check offers a compatible update, review and confirm `everycli worker-update`.
The coordinator must accept categorized `failed` reports before rollout; this
worker does not silently downgrade to the legacy reasonless format.

Local regression tests do not establish production attestation or live-provider
coverage. Existing miners, API keys and model offers are not automatically changed
by installing this CLI release.
