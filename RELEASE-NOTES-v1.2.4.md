# everycli v1.2.4

- Adds native BytePlus Seedance 2.5 Flash worker support for 480p and 720p video, automatic duration, and image, video and audio references.
- Adds Microsoft Azure AI Foundry as a direct provider. MAI Image 2.7 and MAI Image 2.7 Flash support PNG generation and editing with up to five references. See [Foundry setup](AZURE-FOUNDRY.md).
- Lets BytePlus workers download results from the ModelArk Singapore output bucket when the signed media policy is unavailable or expired.
- Verifies the signed manifest format for dstack 0.6 deployments. Shows the approved Phala OS in deployment confirmation and status and flags a VM on a different image.

Run `everycli update` to install the CLI. BytePlus and Foundry miners can use `everycli worker-update` when their profile offers the compatible signed worker. Foundry miners also need to apply their Foundry credentials to the updated worker. Review the new contracts and place offers from the Models page when they are available there.
