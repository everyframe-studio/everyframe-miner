# everycli v0.5.8

- Distinguish Everyframe contract IDs from actual provider API IDs in the CLI
  and model dashboard. Do not paste `studio/` or `direct/` IDs into provider APIs.
- Separate candidates and retired models from supported models. Block new offers
  for removed/deprecated endpoints while preserving withdrawal and job history.
- Check OpenRouter's key-filtered model list, Vivix's workspace list, and Fal's
  public catalog before advertising worker capabilities. Other account access is
  explicitly unchecked; catalog presence is not a paid-generation guarantee.
- Cache catalog checks, handle timeouts/rate limits/incomplete responses safely,
  and avoid paid submissions when the model is unavailable.
- Allow safe cross-miner failover for a definitive catalog refusal before any
  paid request. Ambiguous submissions and moderation failures are not retried.
- Replace misleading "invalid integer" pricing errors with an actionable message
  before confirmation. Batch offers remain atomic: all apply, or none do.

## Update

Run `everycli update`, then reopen `everycli ui`. The worker changes require a
separately verified signed image: run `everycli worker-update --check`, then
review and confirm `everycli worker-update` when available. Updating the CLI
alone does not replace your deployed worker. No provider/model substitution,
broader permissions, changed discounts, or automatic redeployment is introduced.

The 19 October catalog review found two missing and eight deprecated Fal
endpoints. All seven configured OpenRouter API IDs were listed; account settings
can still narrow access. This review used read-only queries, not paid generation.
