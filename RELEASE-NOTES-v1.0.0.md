# everycli v1.0.0

A redesigned miner dashboard, clearer job diagnostics, automatic offer controls,
and expanded model support.

- Follow setup and live readiness checks from the new dashboard, manage workers
  and provider offers, and inspect each job's progress and verification outcome.
- See the changes across every skipped CLI version and the follow-up steps your
  installation needs. Update history remains available in the dashboard.
- Opt in to automatic offers for supported Fal and OpenRouter pricing. Choose
  models, maximum discounts, cost buffers, and estimated spending limits. Automatic
  offers are off by default and pause when current, complete prices are unavailable.
- Use Gemini Omni 1.2 Flash, GPT Image 2.6 Sunburst and Flare, and MiniMax H3 through
  the supported direct and intermediary routes. Compatible direct routes remain first.
- See redacted provider failure messages and private job timelines, including
  generation, output delivery and verification stages.
- Review and accept the Miner Terms before joining or enabling new work. Miners
  remain responsible for provider permissions and commercial arrangements.
- New Phala deployments handle the current provisioning API. Signed configuration,
  OS and workload identity checks remain enforced.

## Upgrade

1. Run `everycli update`, then reopen `everycli ui`.
2. Run `everycli worker-update` to install the compatible signed worker before
   using the new models, detailed worker diagnostics or automatic offers. The
   update uses the same VM and restores its previous accepting or paused state.
3. Review the Miner Terms and your model offers. Enable new models or automatic
   offers only after reviewing their prices, provider access and limits.

Updating the CLI does not update the deployed worker or enable new offers.
Existing manual offers and discounts are preserved. Estimated provider costs and
work value are not provider invoices, cash payments or guaranteed profit.
