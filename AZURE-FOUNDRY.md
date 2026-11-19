# Microsoft Azure AI Foundry

Deploy MAI-Image-2.7, MAI-Image-2.7-Flash, or both in your Foundry resource.
Copy the resource name from your `<resource>.services.ai.azure.com` endpoint,
the API key for that resource, and the exact deployment names you chose in
Foundry. [Microsoft Foundry deployment guide](https://learn.microsoft.com/en-us/azure/foundry/foundry-models/how-to/use-foundry-models-mai)

1. Run `everycli worker-update` when a compatible signed release is available.
2. In `everycli ui`, open **API Keys**, find **Microsoft Azure AI Foundry**,
   and enter your resource, key, and deployment names. Use the same panel to
   replace credentials when any of the details change.
3. Apply the saved keys to the worker, then review the available offers in
   **Models**.

In the terminal, run `everycli set-api-keys --provider azure-foundry`, then
paste a single JSON line into the hidden prompt. Use your own resource, key,
and deployment names. Leave out the `standard` or `flash` entry if you only
deployed one model.

```json
{"resource":"your-foundry-resource","apiKey":"YOUR-FOUNDRY-RESOURCE-KEY","deployments":{"standard":"your-mai-27-deployment","flash":"your-mai-27-flash-deployment"}}
```

Then run `everycli apply-api-keys`. Saving checks the configuration locally.
Verify deployment access and Azure charges in Foundry. The CLI uses resource
API keys and lists only the MAI variants configured on the worker.

Both contracts return one PNG. Generation accepts width and height of at
least 768 pixels each and up to 4,194,304 total pixels. Standard also supports
transparent backgrounds for generation. Edits accept up to five JPEG or PNG
references. Foundry chooses the edit output size.

## Release integration

The companion coordinator update adds the provider, both matching contracts,
and their preview work-value estimates. For each reviewed signed worker
release that supports Foundry, include both model IDs and add
`AZURE_FOUNDRY_CREDENTIALS` to the worker environment and
`allowed_envs`. Measure and sign the resulting compose. Existing profiles
receive this environment change through manual worker update discovery.
