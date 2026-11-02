## Worker updates wait for status recovery

- Temporary gateway errors, including HTTP 502, during status checks no longer end an otherwise progressing worker update. The command waits and continues from its saved stage.
- Temporary heartbeat, session and attestation delays are checked again before restoring work.
- Completion requires confirmation of the intended accepting/paused state. If the wait limit is reached, progress stays saved for later continuation.
- Status retries preserve the completed VM update and provider-key activation.

## Update

```sh
everycli update
```

Restart `everycli ui` to load the updated behavior. No new generation-worker image is required for this CLI fix. If a worker update is already pending, run `everycli worker-update` on the same profile to continue it.
