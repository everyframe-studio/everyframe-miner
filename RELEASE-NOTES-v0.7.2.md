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

The 15 new provider-model candidates from v0.7.1 remain gated pending funded
provider validation and pricing approval. No missing provider accounts are
required for this update, and existing offers are not changed by installation.
CLI version, signed worker release and live model readiness are separate states.
The coordinator recognizes candidate IDs in worker capability reports without
granting model permissions or enabling unpriced bids.

## Routing compatibility

After the signed worker publication, the existing routing policy prefers eligible
v0.7 workers and falls back to eligible v0.6 workers when needed. Workers on v0.5
must update to receive new jobs. Updating only the local CLI does not change the
deployed worker's routing version. Existing in-flight jobs are not canceled.

## Validation

See [the exact-artifact test scope](REAL-MINER-TEST-v0.7.2.md) and the release's
`canary-validation.json` attachment for cloud recovery and cleanup evidence.
The canary submits no provider generations; it does not certify the 15 gated
model candidates as production-ready.
