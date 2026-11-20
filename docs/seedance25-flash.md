# Seedance 2.5 Flash

Prepared on 2026-11-15 after the announced 06:00 UTC API launch. This change is
not a release or an activation of any miner's keys, offers, or VM.

The independent native BytePlus contract is `direct/byteplus/seedance-2.5-flash`.
It sends `dreamina-seedance-2-5-flash-261115` to the existing Ark task API.
It does not alias flagship Seedance 2.5, Premium, or Seedance 2.0.

Supported: 480p/720p, 4–30 seconds or automatic duration, 24fps, text,
first/last frames, and up to 30 images, 10 videos and 10 audio references.
Audio-only references and explicit reference/edit/extend/auto task hints are
supported. Frame and editing/extension requests preserve the source aspect;
editing preserves duration. Draft mode, MOV selection, 1080p, 4K and offline
inference are not offered by this contract. Prompt-embedded parameter switches
are rejected so they cannot override the reviewed request.

BytePlus public pricing is $7/M output tokens without video input and $4.30/M
with video input. The minimum-token table for video inputs is not accessible to
the offline verifier. The coordinator therefore labels video quotes as
conservative reservations: full 30-second input allowance at the higher $7/M
rate, plus one output frame for rounding. Automatic duration reserves 30 output
seconds. These reservations are not invoices or measured provider costs.

## Sources

- [Released model detail](https://ai.byteplus.com/ark/region:ap-southeast-1/model/detail?name=dreamina-seedance-2-5-flash)
- [Create task](https://console.byteplus.com/ark/region:ap-southeast-1/docs/create-video-generation-task-api)
- [Pricing](https://console.byteplus.com/ark/region:ap-southeast-1/docs/model-pricing)
- [BytePlus client model contract](https://github.com/byteplus-sa/ComfyUI-BytePlus-ModelArk/blob/9ef2b0f4e57efa5cfcddd97aee0e506cb07db697/nodes/models_config.py)
- [BytePlus client task builder](https://github.com/byteplus-sa/ComfyUI-BytePlus-ModelArk/blob/9ef2b0f4e57efa5cfcddd97aee0e506cb07db697/nodes/nodes_seedance2.py)
- [BytePlus Flash transport tests](https://github.com/byteplus-sa/ComfyUI-BytePlus-ModelArk/blob/9ef2b0f4e57efa5cfcddd97aee0e506cb07db697/tests/test_core_style_seedance2.py)

The released Flash tab confirms duration, resolution, modalities and no draft.
The official client explicitly includes Flash in the 2.5 input/task family,
sets its own resolution list, and omits output_format. The generic API tables
alone are not the source of Flash-specific limits.

## Activation after review

A compatible signed worker release must ship the new Flash validation branch.
Older workers intentionally report `catalog_adapter_update_required` for this
contract; do not loosen signed-catalog compatibility to force the addition.
After that one compatibility update, compatible later model additions can still
arrive through the signed catalog. This change does not disable that mechanism.

Deploy reviewed coordinator/app source separately, then explicitly enable the
app's `SEEDANCE_2_5_FLASH=on` feature flag. App requests use BytePlus miners only;
no unverified reseller fallback is installed. Miners still choose whether to
supply a BytePlus key and offer the model. Check attestation, balances and model
readiness before opening traffic. A real paid canary has not been run.
