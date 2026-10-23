# everycli v0.7.2

## Offline worker updates

The signed bootstrap installs a separate, isolated supervisor in the existing
Phala VM. After explicit enrollment, compatible signed worker updates can run
even when the miner's management computer is offline.

Updates drain work, keep the same VM, preserve the miner's paused/accepting state,
and require fresh TEE admission before restoring provider keys. Failed admission
can trigger a signed rollback. Uncertain cloud operations are reconciled instead
of blindly repeated. The supervisor itself remains pinned during routine updates.

## Upgrade and opt in

```sh
everycli update
everycli worker-update --check
everycli worker-update
everycli doctor
everycli worker-auto-update --enable
```

The manual worker update is required once to install the bootstrap. Wait for
fresh admission before enabling automation. Existing miners are not opted in
automatically. Enrollment asks permission to transfer the workspace-scoped Phala
management token and bounded credentials to the isolated supervisor; use a
dedicated Phala workspace. Wallet secrets and billing-only keys are not transferred.

The browser UI has the same controls under **Worker → Automatic worker updates**.

Read remote state with `everycli worker-auto-update`; disable future automatic
stages with `everycli worker-auto-update --disable`. Disabling does not stop VM
billing or erase an unfinished recovery. Provider-key changes need explicit
re-enrollment; the offline supervisor cannot read new keys from your laptop.

## Model availability

The coordinator supports 14 additional direct-provider models:

- **MiniMax:** Hailuo 02.
- **BFL:** FLUX.2 Max, Flex, Klein 4B and Klein 9B.
- **Replicate:** FLUX.2 Pro.
- **Runway:** Veo 3.1 and Veo 3.1 Fast.
- **OpenRouter:** Veo 3.1 Fast and Veo 3.1 Lite.
- **ElevenLabs:** v4, v4 Turbo, v3 and Flash v2.5 with Rachel.

See [formats and setup](PROVIDER-EXPANSION.md). Configure the corresponding
provider key, install the compatible signed worker update, and set a discount
for each model you want to serve. Existing bids are preserved.

## Routing compatibility

After the signed worker publication, the existing routing policy prefers eligible
v0.7 workers and falls back to eligible v0.6 workers when needed. Workers on v0.5
must update to receive new jobs. Updating only the local CLI does not change the
deployed worker's routing version. Existing in-flight jobs are not canceled.

## Validation

See [the exact-artifact test scope](REAL-MINER-TEST-v0.7.2.md) and the release's
`canary-validation.json` attachment for cloud recovery and cleanup evidence.
The bootstrap canary covered automatic updates and recovery without submitting
provider generations. The subsequent coordinator model activation passed 322
tests, including per-request speech pricing and safe failover.
