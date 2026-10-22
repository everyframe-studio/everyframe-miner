# v0.7.0 publication checklist

Release branch: `feat/kling-direct-v070`. CLI publication and signed worker
activation are separate. Neither this document nor CLI publication authorizes
new model routing, automatic updater enrollment or a new measured image.

Includes the previous offline updater work and Kling direct API support. The
v0.6.3 checklist and real-miner report are historical: their test counts, source
commit and image digests do not certify this new binary or Kling integration.

Read-only live check: configured Kling key returned HTTP 200, outer code 0 and
inner data code 0 on `/account/costs`. Its package list was omitted; authentication
is verified, but funding and generation access are not. No generation was sent.

CLI publication gates:

1. Pass Rust, formatting, Clippy, browser tests and four-platform native CI.
   Coordinator contract tests must pass locally; deployment is a separate gate.
2. Review/commit, push a branch, PR and merge. Tag v0.7.0 only after PR checks pass.
3. Require all native release builds and clean worker/supervisor build/smoke gates.
4. Verify official installers, checksums and version; release notes and docs must
   state that bootstrap deployment and direct Kling routing remain pending.

Before signed worker activation (not claimed by the CLI-only publication):

1. Pass Rust, coordinator, formatting, Clippy and native-platform CI checks.
2. Verify a funded Kling developer key and run an approved-budget live generation.
   Confirm actual output host, format, resolution, duration and terminal statuses.
3. Review direct Kling API pricing independently. Do not inherit Fal prices or
   convert delayed package units into a reported live USD balance.
4. Deploy scoped coordinator changes: provider contracts, duration support,
   balance provider validation, signed allowlist and optional-key upgrade logic.
   No production config or signed release was changed by this source patch.
5. Build final immutable worker/supervisor images and repeat exact-artifact TEE
   admission and recovery verification. Previous v0.6.3 canary pins are not v0.7.0.
6. Publish only the verified signed worker configuration. Update hosted docs
   when deployment instructions match the public worker release.

Do not enroll existing miners automatically. CLI update is separate from worker
update; offline worker automation remains explicit opt-in.
