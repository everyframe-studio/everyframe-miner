# Provider balance checks

Balance queries and API-key checks allow up to 30 seconds per read. Supported balance queries run concurrently (at most eight at a time). They do not submit generations or retry cloud mutations. A timeout is an unavailable reading, not a zero balance or proof of a bad key. Unsupported balance queries appear after supported providers in the CLI and dashboard.

Local balance and key checks use the installed CLI. Deployed workers receive changes to their automatic balance telemetry only through a signed worker update.

## Google / Gemini

The Gemini API key integration has no documented prepaid USD balance endpoint. The CLI keeps the amount unknown; it does not infer prepaid credit from token usage, quota or a successful model-list request.

For a Prepay account, view credits and configure optional automatic top-ups in [Google AI Studio Billing](https://aistudio.google.com/billing). Auto-reload purchases more credit when your configured balance trigger is reached; the CLI does not enable it.

For spending notifications, configure [Google Cloud Billing budgets and alerts](https://docs.cloud.google.com/billing/docs/how-to/budgets) on the relevant billing account/project. Those are spending alerts, not a live Gemini prepaid-balance feed. [Programmatic budget notifications](https://docs.cloud.google.com/billing/docs/how-to/budgets-programmatic-notifications) use Pub/Sub and separate Google Cloud IAM permissions; a Gemini API key alone is insufficient. Billing data can be delayed, so budget alerts should not be treated as a real-time balance or a hard spending cap.

Sources: [Gemini billing documentation](https://ai.google.dev/gemini-api/docs/billing) and [Cloud Billing Budget API](https://docs.cloud.google.com/billing/docs/how-to/budget-api-overview).
