# Automatic offers

Automatic offers use provider price checks to adjust bids for models you explicitly
select. The feature is off by default. Your worker performs the checks, so it
continues when the dashboard or your computer is closed. Manual offers remain
available for every supported provider.

In **Models → Automatic offers**, configure Fal or OpenRouter, select models and
enter your maximum discount, cost buffer, rolling 24-hour estimated USD limit and
per-job estimated USD limit. **Preview prices** reads prices without changing
anything or generating media. **Review and enable** shows the exact scope and limits
before saving the policy. A valid price above your limits starts paused and can
activate when a qualifying lower price appears. Missing or incomplete prices cannot
be treated as free generation.

The maximum discount is a ceiling. The cost buffer is a percentage added to an
estimated provider cost; it is not a guaranteed profit margin. The coordinator
admits each job only when its settings-specific estimated cost fits the policy.
Automatic offers use the deepest discount permitted by your reviewed ceiling
and cost buffer. Manual new offers still default to 0%. The policy does not change the network's base reward schedule.

The same actions are available in the terminal:

```sh
# Read policies, paused reasons and recent changes.
everycli auto-offers

# Example limits only: choose amounts appropriate for your own account.
everycli auto-offers --preview --provider fal \
  --models studio/media/fal/xai/grok-imagine-image \
  --max-discount-pct 20 --buffer-pct 20 \
  --max-daily-usd 1 --max-job-usd 0.10

# Use --enable instead of --preview to review and save the policy.
# This requires current Miner Terms acceptance.
everycli auto-offers --disable --provider fal
```

No scope or spending limit is chosen automatically. USD limits accept at most six
decimal places. Maximum discount accepts 0–99.9%, including 0%. Read-only status and
previews do not require accepting new terms; enabling does.

## Price sources

- **Fal:** normal generation-key pricing and unit-price estimate endpoints. Account
  pricing returned by those endpoints is used. Only reviewed billing units are
  eligible. A zero-price compute placeholder is not a free generation quote. Jobs
  with extra reference/input components whose current price is not supplied by the
  API pause under automation; their manual offers remain available.
- **OpenRouter:** the public video-model catalog. The policy checks all relevant
  reviewed resolution, audio, token and reference components and uses a conservative
  upper ratio for the requested settings. Private discounts absent from that catalog
  are not assumed. A new unrecognized billing component pauses automation.

Normal generation keys stay in their existing local profile and attested worker.
No billing, administrative, hosting or hotkey credential is added to the worker.
Only fixed-point price observations and a credential fingerprint are reported;
provider keys and raw provider responses are not included.

## Pauses and manual control

Prices refresh about once a minute and expire after five minutes. Missing prices,
provider failures, changed contracts, incomplete billing units and exceeded limits
pause new automatic work. A renewed worker session must supply fresh observations.
These checks happen both when assigning work and immediately before granting a
provider submission. They never change an already-started job's agreed price.

Disabling a policy withdraws only the models it still manages. A manual offer or
withdrawal removes that model from automation. Turning a provider's offers off also
withdraws waiting or paused managed models, preventing automatic reactivation.

The rolling limit reserves estimated exposure before submission. Uncertain and
possibly billed failed submissions keep their reservation; only proven non-submission
releases it early. Disabling/re-enabling or renewing a session does not clear
exposure. After an unconfirmed policy request, read `everycli auto-offers` before
another change; the CLI never blindly replays it.

These are estimated limits for jobs admitted under this policy, not a provider-side
billing cap. Manual offers, other account use and hosting are excluded. Provider
prices can change after a check. Work value is accounting and an alpha-value target,
not cash received, a fixed USD payout or guaranteed profit. Provider permissions and
commercial arrangements remain the miner's responsibility under the Miner Terms.

## Upgrade and availability

This source change is prepared for the unreleased v1.0.0 integration. It requires the
matching coordinator changes, CLI/UI and a compatible signed worker update. Once
that release is published, update the CLI, reopen the UI and run
`everycli worker-update`, then configure the policy explicitly. Updating never opts
existing miners into automatic offers. Older workers and manual offers keep their
existing behavior.
