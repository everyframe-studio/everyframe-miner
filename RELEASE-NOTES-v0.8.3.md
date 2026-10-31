## Pika H3 Flex

- Adds the Pika API provider to everycli and the attested worker, including key validation and spendable USD balances in the CLI and browser UI.
- Supports 5–15-second text, first/last-frame and image/video/audio reference generation with native audio. 480p/768p are native; 1080p, 2K and 4K use Pika's upscaler.
- Uses Pika's exact aspect-ratio buckets and frame-grid duration when validating output and quoting miner work.
- Supports Pika video outputs up to 128 MiB, including larger 4K clips, with a bounded upload lane that keeps heartbeats responsive.
- Reports terminal Pika failures promptly, with clear billing, capacity and content diagnostics.
- Preserves the compatible update path for existing automatic controllers. The new CLI can apply the reviewed Pika configuration and controller migration on the same VM.

## Upgrade

```sh
everycli update
```

To add Pika, save the provider key locally and install the signed worker update:

```sh
everycli set-api-keys --provider pika
everycli worker-update
everycli doctor
everycli offers
```

If automatic worker updates are enabled, run `everycli worker-auto-update --disable`
before this one-time manual setup. After the updated worker passes fresh
attestation and you have reviewed its offers, run
`everycli worker-auto-update --enable` to enroll the updated controller and keys.
Automatic updates continue for existing providers without this Pika setup.

The update preserves the VM, wallet identity, offers and discounts. Provider
keys activate only after fresh workload attestation. New authorizations do not
expire; existing legacy authorizations can be migrated with `everycli reconnect`
and `everycli apply-api-keys` as documented in the mining guide.
