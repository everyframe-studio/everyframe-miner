## More reliable miner setup and controls

- Batch discounts update eligible models together and name any skipped models. An unavailable existing offer can still be withdrawn, and a temporary worker check no longer prevents an authorized, priced bid.
- Guided setup includes Pika and direct Kling alongside the other providers.
- Worker labels follow the observed cloud state, and unavailable lifecycle actions stay disabled.
- API-key and model-discount drafts survive unrelated changes. Setup progress refreshes without clearing open forms.
- Interrupted UI requests retain their operation IDs, recover results without resubmitting changes, and display unconfirmed outcomes clearly.
- The Worker page shows automatic-update mode and readable update summaries. Emergency and destructive actions are grouped under Advanced.
- Model search and provider/type filters make large catalogs easier to manage.

## Update

```sh
everycli update
```

Close an already-running `everycli ui` process and start it again to load the updated dashboard. This release changes the local CLI/UI; it does not require a generation-worker update.
