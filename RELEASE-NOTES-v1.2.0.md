# everycli v1.2.0

Receive compatible model additions without repeated binary upgrades, find new models you can offer, and improve provider result delivery.

- Adds signed coordinator model catalogs for existing native, Google, Studio video, image, audio and tool adapters. Definitions refresh between jobs and remain fixed for existing jobs and recovery.
- Keeps provider protocols, API origins and credentials within the reviewed worker. New provider protocols still require a worker update.
- Adds signed provider delivery policies so reviewed CDN changes can reach compatible workers without a new binary.
- Adds MiniMax H3 Image Standard and Highspeed contracts through direct MiniMax, OpenRouter and Fal, including reference editing.
- Adds Seedance 2.0 contracts through 4K with provider-specific reference and duration checks.
- Fixes Fal H3 Image result polling and classifies explicit MiniMax authentication and billing refusals.
- Improves Google TTS delivery of complete audio, including models added through the signed catalog, and MiniMax task recovery.
- Shows offerable models under **Not offered**, with persistent badges for new models and an explicit acknowledgment control.

## Upgrade

```sh
everycli update
everycli worker-update
```

Run the worker update once for catalog support and the provider fixes. Reopen `everycli ui` afterward. Updating the CLI alone does not update the worker. Existing VM identity, keys, offers, discounts and accepting/paused state are retained by the signed update workflow.

New model availability depends on coordinator enablement, compatible authorization, current pricing and your provider keys. New models do not create offers automatically; review them on the Models page. Existing automatic-offer selections remain unchanged.
