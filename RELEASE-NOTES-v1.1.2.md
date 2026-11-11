# everycli v1.1.2

OpenRouter spending-limit protection and safer miner failover.

- Checks the generation key’s own limits and organization budgets, separately from account credit.
- Pauses affected OpenRouter routes until a fresh check confirms recovery; other providers stay available.
- Retries another eligible miner only for confirmed pre-submission budget rejections, within the original price and deadline limits. Ambiguous submissions and content refusals are never replayed.
- Shows spending-limit problems in Models and retains support for generated outputs up to 128 MiB.

## Upgrade

```sh
everycli update
everycli ui
```

If serving OpenRouter models, also update the existing worker:

```sh
everycli worker-update
```

Existing offers and discounts are retained. Updating the CLI alone does not update the deployed worker. A capped key or organization budget must be raised or reset before OpenRouter work can resume.
