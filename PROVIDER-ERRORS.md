# Provider failure messages

Workers report the existing failure category, an optional recognized refusal
code and the provider's own error code/message. Unfamiliar wording is retained;
it does not require a new refusal rule before it can be inspected.

Example signed action payload:

```json
{
  "attemptId": "example-attempt",
  "reason": "provider_invalid_request",
  "providerError": {
    "code": "NewProviderRule",
    "message": "The reference needs another anchor."
  }
}
```

The provider code is bounded to 128 UTF-8 bytes and message text to 4,096 UTF-8
bytes. Truncation is marked. Worker credentials, credential-shaped values,
credential labels, URLs and terminal control characters are removed before
reporting. Request bodies, echoed input objects and HTTP headers are not copied
into the message. Only provider error fields are selected.

Messages are diagnostics. They never decide retries, charging, moderation
bypass or fallback. A provider response can be recorded on an uncertain
submission while its outcome remains uncertain. Such an attempt must still be
reconciled; forwarding a message does not permit another paid submission.

The coordinator stores the diagnostics with the affected attempt/job and
returns them to the private order API. The backend exposes them with the failed
render/tool result, and the website renders them as plain text. Existing
historical internal errors keep their generic customer message.

After publication, install the compatible signed worker using
`everycli worker-update` to enable richer reporting. Updating the CLI alone
does not replace a deployed worker. An older coordinator may decline the new
field; the worker can still report the original category and recognized refusal
without submitting the provider job again.
