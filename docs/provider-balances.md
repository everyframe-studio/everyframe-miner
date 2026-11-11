# Provider balance checks

Balance queries and API-key checks allow up to 30 seconds per read. Supported balance queries run concurrently (at most eight at a time). They do not submit generations or retry cloud mutations. A timeout is an unavailable reading, not a zero balance or proof of a bad key. Unsupported balance queries appear after supported providers in the CLI and dashboard.

Local balance and key checks use the installed CLI. Deployed workers receive changes to their automatic balance telemetry only through a signed worker update.

A successful lookup does not prove an account has enough funds. A zero or negative Phala credited balance produces a visible credit alert in the dashboard and CLI. Phala postpaid credit limits are not included, so the alert does not claim that the VM must stop. Stale readings are identified as last reported values; positive amounts below one cent display as `<$0.01` rather than rounding to zero.

Unsupported balance lookups, including Google, are informational and do not cause an overview warning. Failed or stale supported lookups and Phala credit alerts still produce warnings.

## Google / Gemini

The Gemini API key integration has no documented prepaid USD balance endpoint. The CLI keeps the amount unknown; it does not infer prepaid credit from token usage, quota or a successful model-list request.

For a Prepay account, view credits and configure optional automatic top-ups in [Google AI Studio Billing](https://aistudio.google.com/billing). Auto-reload purchases more credit when your configured balance trigger is reached; the CLI does not enable it.

For spending notifications, configure [Google Cloud Billing budgets and alerts](https://docs.cloud.google.com/billing/docs/how-to/budgets) on the relevant billing account/project. Those are spending alerts, not a live Gemini prepaid-balance feed. [Programmatic budget notifications](https://docs.cloud.google.com/billing/docs/how-to/budgets-programmatic-notifications) use Pub/Sub and separate Google Cloud IAM permissions; a Gemini API key alone is insufficient. Billing data can be delayed, so budget alerts should not be treated as a real-time balance or a hard spending cap.

Sources: [Gemini billing documentation](https://ai.google.dev/gemini-api/docs/billing) and [Cloud Billing Budget API](https://docs.cloud.google.com/billing/docs/how-to/budget-api-overview).


## OpenRouter generation-key limits

Account credit and spending permission are separate. An OpenRouter account can
have credit while its generation key, member, or workspace has exhausted a daily,
weekly, monthly, or non-resetting budget.

The worker checks the deployed generation key using the read-only
[`GET /api/v1/key`](https://openrouter.ai/docs/api/api-reference/api-keys/get-current-api-key)
endpoint before work and at most once per minute during normal operation. No
management key or test generation is needed. A rejected generation without
structured budget metadata triggers a fresh read-only check to protect subsequent
work, but remains final for that job. Arbitrary provider messages and a later
balance observation cannot prove the cause of an earlier rejection. Unavailable or malformed checks pause new
OpenRouter submissions until a check succeeds.

A confirmed exhausted budget blocks that miner's OpenRouter routes. The
coordinator keeps the block across sessions and restarts. Updating the account
balance, refreshing the page, or restarting a worker cannot clear it. A fresh,
attested positive budget check clears it after the limit resets or the miner
fixes the key/member/workspace budget in OpenRouter. Other providers and the
miner's existing offer prices remain unchanged.

Only a definitive rejection before provider acceptance can move a job to a
different eligible miner under the original deadline and price limit. Accepted
or uncertain submissions are never automatically repeated. Both the coordinator
extension and a signed worker containing this check are required for persistent
blocking and automatic recovery; older coordinators receive the existing billing
failure code. Installing the CLI alone does not update a running worker.
