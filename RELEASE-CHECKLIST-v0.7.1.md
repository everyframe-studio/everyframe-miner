# v0.7.1 release checks

Updated 2026-10-22. This checklist separates CLI publication from production
worker admission. See GitHub Actions and the release page for publication status.

## Scope

15 new fixed contracts across seven existing providers; see
[PROVIDER-EXPANSION.md](PROVIDER-EXPANSION.md). Existing contracts, signed
authorizations, pricing and customer routing are unchanged.

## Verified

- 33 focused Rust tests passed, including historical request hash compatibility,
  all-provider mock submission/polling, expired deadlines, uncertain submission
  handling and updater packaging.
- Companion coordinator: 316 tests passed, including exact registry parity and
  rejection of unpriced/unauthorized new offers.
- Final Rust release-profile suite on pinned toolchain 1.97.1: 299 passed,
  zero failures, two intentionally ignored live probes (dedicated hotkey and
  paid provider tests). This includes the 33 focused tests above.
- Release-profile Clippy passed for all targets with warnings denied.
- Pinned-toolchain formatting and whitespace checks passed.
- Linux x86_64 release build passed for all three native binaries before the
  version bump. The publication workflow tests the versioned source again.
- Authenticated read-only OpenRouter video catalog returned HTTP 200 and listed
  `google/veo-3.1-fast` and `google/veo-3.1-lite`. This does not prove generation
  access, funds or valid delivered output.

## CLI publication gates

- Four-platform build/CI and final worker/updater container checks.
- Branch PR, merge, matching version tag, checksums and installer verification.

## Separate live-mining activation gates

- Funded provider access and paid output validation for the new contracts.
- Provider-specific reviewed pricing and coordinator quote tests.
- Final immutable worker image and real TEE admission canary.
- Scoped coordinator deployment and signed worker/model policy publication.

Those gates are not completed by publishing the CLI. New contracts must remain
unavailable for routing until their production activation is independently
verified. Public source does not grant provider entitlements or signed admission.

### Live attempt and current blocker

The Rust adapter submitted one labeled Veo Fast test. OpenRouter rejected it
with HTTP 402; no accepted job reference was returned. The harness stopped
without retrying or submitting Veo Lite. Funded provider tests remain incomplete.

No temporary VM was created. No video was delivered, no production routing or
signed worker policy changed during this validation. CLI publication is separate
from funded generation tests, final image verification and TEE admission.

The opt-in `tests/live_provider_expansion.rs` harness uses the actual Rust
adapter, a create-new submission journal and output media checks. It is ignored
by default; it cannot reuse an existing submission intent. Its first local
setup attempt rejected an unsupported seed field before any HTTP submission;
that harness input was corrected before the single HTTP 402 attempt. Clippy
also passed for the new harness.

## Pricing review notes

Public list prices are evidence for review, not active miner reward quotes or
proof of the account's actual charge. Taxes, account terms and future provider
price changes are not included.

| New contract | Published unit rate | Derived fixed-job estimate |
| --- | --- | --- |
| OpenRouter Veo 3.1 Fast, 4s with audio | $0.10/second | $0.40 |
| OpenRouter Veo 3.1 Lite, 4s with audio | $0.05/second | $0.20 |
| Runway Veo 3.1, 4s with audio | 40 credits/second, $0.01/credit | $1.60 |
| Runway Veo 3.1 Fast, 4s with audio | 15 credits/second, $0.01/credit | $0.60 |

Sources checked on 2026-10-22:
[OpenRouter Fast](https://openrouter.ai/google/veo-3.1-fast),
[OpenRouter Lite](https://openrouter.ai/google/veo-3.1-lite), and
[Runway pricing](https://docs.dev.runwayml.com/guides/pricing/).
Do not reuse one provider's rate for another provider's endpoint.

BFL's [pricing documentation](https://docs.bfl.ai/quick_start/pricing) confirms
resolution-dependent FLUX.2 pricing, but the fetched table omitted numeric
rates. Exact BFL quotes, and the remaining provider prices, still need review;
no missing rate should be guessed or inherited from an older model.

## Rollout safety

Publish no new routable model until its price, permission and admitted worker
are all verified. Keep paid tests labeled as tests, do not retry ambiguous
submissions, and delete a temporary worker and storage after an approved canary.
CLI installation alone must not silently activate these contracts or opt an
existing miner into automatic worker updates.
