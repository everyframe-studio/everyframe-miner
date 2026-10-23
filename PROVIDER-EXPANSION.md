# More models from existing providers

The v0.7.2 worker supports 14 additional direct-provider contracts enabled on
the coordinator on October 23, 2026. Each uses the miner's key for that provider.

| Provider | Added models | Initial contract |
| --- | --- | --- |
| MiniMax | Hailuo 02 | Text to video, 6s, 1080p, 16:9 |
| BFL | FLUX.2 Max, Flex, Klein 4B, Klein 9B | One 1024×1024 PNG |
| Replicate | FLUX.2 Pro | One 1024×1024 PNG |
| Runway | Veo 3.1, Veo 3.1 Fast | Text to video, 4s, 720p, 16:9, audio |
| ElevenLabs | Eleven v4, v4 Turbo, v3, Flash v2.5 | Rachel voice, MP3 44.1 kHz/128 kbps, up to 500 characters |
| OpenRouter | Veo 3.1 Fast, Veo 3.1 Lite | Text to video, 4s, 720p, 16:9, audio |

These are fixed contracts. Existing flexible Studio contracts remain available
separately. Each new contract has its own price and discount offer.

## Setup

```sh
everycli update
everycli worker-update --check
everycli worker-update
everycli set-api-keys --provider bfl
everycli apply-api-keys
everycli offers
```

Replace `bfl` with the provider you use. Keep existing working keys. After fresh
worker admission, open **All models** in `everycli ui`, select the new models,
and set your discount. The CLI offers view shows the current price and readiness.
Models need an active offer and a configured provider key before receiving jobs.
Existing discounts are not copied to new models.

The v0.7.2 worker image already contains these adapters. Workers on that image
receive the expanded model authorization during fresh admission. Older workers
should install the compatible signed update using the commands above. Automatic
updates continue to respect the miner's enrollment preference.

## Pricing

Quotes use provider-specific list prices and the exact format shown above.
ElevenLabs work value is calculated from the submitted text length, up to 500
UTF-16 code units, with Rachel as the fixed voice. Provider invoices can differ
with account-specific plans. Review the current quote before setting a discount.

- [MiniMax prices](https://platform.minimax.io/docs/pricing/overview)
- [BFL prices](https://bfl.ai/pricing?category=flux.2)
- [Replicate FLUX.2 Pro](https://replicate.com/black-forest-labs/flux-2-pro)
- [Runway prices](https://docs.dev.runwayml.com/guides/pricing/)
- [OpenRouter Fast](https://openrouter.ai/google/veo-3.1-fast) and [Lite](https://openrouter.ai/google/veo-3.1-lite)
- [ElevenLabs prices](https://elevenlabs.io/pricing/api)

Existing submission recovery, deadline checks, signed receipts and TEE admission
apply to these contracts. An uncertain paid submission is reconciled before any
retry; provider credentials are never sent to output download hosts.
