# everycli v1.3.1

Inspect provider failures in `everycli jobs` and **Jobs → Details**. See the provider’s recorded code and message, including account errors such as exhausted credit. Each message stays with the attempt that reported it, even if the job was reassigned. Sensitive values are redacted.

The worker now records HTTP status on supported provider failures. The CLI help also shows the 512-model limit for an atomic offer batch.

1. Run `everycli update`.
2. Close and reopen `everycli ui` if it is open.
3. Run `everycli worker-update` when the compatible signed update is offered to record HTTP status on new failures.

View a full attempt with `everycli jobs --job-id JOB_ID` or add `--json` to read the structured diagnostic.
