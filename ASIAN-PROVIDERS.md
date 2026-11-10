# Alibaba Cloud, BytePlus and Volcengine

These direct-provider adapters run inside the EveryFrame worker. The worker's
signed release must permit each credential, and the coordinator must admit the
same model contracts before an offer can serve jobs. Installing a CLI alone does
not update an existing worker or grant new credential permissions.

| Provider | Region | Models | Generation credential |
| --- | --- | --- | --- |
| Alibaba Cloud Model Studio | Singapore | Wan 3.0, Wan 3.0 Prime, Qwen Image 3.0, Qwen Image 3.0 Pro | `DASHSCOPE_API_KEY` plus `DASHSCOPE_WORKSPACE_ID` |
| BytePlus ModelArk | Singapore | Seedance 2.5, Seedream 5.0 Pro, Seedream 5.0 Flash | `BYTEPLUS_API_KEY` |
| Volcengine Ark | Beijing | Seedance 2.5, Seedream 5.0 Pro, Seedream 5.0 Flash | `VOLCENGINE_API_KEY` |

BytePlus and Volcengine use distinct accounts, API keys, model IDs and billing.
Their credentials cannot be substituted for one another. Alibaba's workspace,
model access and API key must belong to the Singapore region. The workspace ID
is validated and inserted only into the official regional hostname; arbitrary
API hosts are not accepted.

## Setup

Use the local setup page, or save keys with hidden terminal prompts:

```bash
everycli set-api-keys --provider alibaba --workspace YOUR_SINGAPORE_WORKSPACE_ID
everycli set-api-keys --provider byteplus
everycli set-api-keys --provider volcengine
```

Use `everycli worker-update --check` to check release compatibility. Once a
compatible signed worker is installed, use the existing `apply-api-keys` flow
and review `everycli offers` before enabling the desired offers. Existing
workers are not restarted by saving a key.

Read-only ModelArk/Ark model-list checks can check authentication, but do not
prove every model is enabled or funded. Balance reporting returns **unsupported**
when a generation key has no supported billing endpoint; it never substitutes a
zero balance or assumes sufficient credit. Billing/admin credentials are not
required by these adapters.

## Supported requests

- Video: text, first frame, first and last frames, or reference images/video/audio.
  First/last-frame mode cannot be mixed with omni references. Seedance supports
  4–30 seconds; Wan supports 2–30 seconds. Wan's input plus output video duration
  must fit its 30-second limit. Wan reference video and audio clips are 1–15
  seconds each, with at most 15 seconds in total for each media type. Output
  options are 480p, 720p and 1080p, with or without generated audio.
- Images: text or reference-image edits, one output per job. Qwen supports up to
  three reference images; Seedream supports up to fourteen. This release uses
  explicit image sizes up to 2048×2048 and supports PNG/JPEG where the provider
  exposes that option. Layer decomposition and image batches are not requested.
- References remain content-addressed EveryFrame attachments. Their bytes and
  MIME types are checked before the provider request. Jobs cannot supply API
  keys, endpoint URLs, or provider-hosted asset IDs.
- Video jobs resume through their original task ID. A timed-out submission is
  not automatically replayed. Image responses are synchronous; a lost response
  cannot safely be regenerated automatically.
- BytePlus downloads accept the documented Singapore task-result host without
  sending provider credentials. Unrelated hosts and redirects remain blocked.
- Automatic-update capsules preserve Alibaba's workspace only when the signed
  policy permits both the workspace and its generation key. Asynchronous task
  failures retain sanitized provider codes and explanations, including unknown
  codes, without changing submission certainty or replay behavior.

The coordinator validates actual output dimensions, duration, codecs and audio.
Pricing estimates include input video duration where applicable. They are
planning values, not invoices. Alibaba estimates use Singapore list prices;
Volcengine converts CNY with a dated ECB snapshot and expires that conversion
after one week. Existing providers' rates and contracts are unchanged.

## API references

- [Alibaba video API](https://help.aliyun.com/en/model-studio/wan3-video-generation-api-reference)
- [Alibaba image API](https://help.aliyun.com/en/model-studio/qwen-image-generation-and-editing-api-reference)
- [BytePlus video API](https://docs.byteplus.com/en/docs/modelark/create-video-generation-task-api?redirect=1)
- [BytePlus image API](https://docs.byteplus.com/en/docs/modelark/image-generation-api)
- [Volcengine video API](https://docs.volcengine.com/docs/ark/create-video-generation-task-api?lang=zh)
- [Volcengine image models](https://docs.volcengine.com/docs/ark/seedream-5-0-pro?lang=en)
