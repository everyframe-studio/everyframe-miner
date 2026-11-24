# everycli v1.3.2

v1.3.2 adds miner credentials and worker support for five Seedance 2.5 text-to-video providers. They are VidLab, Higgsfield, Kie.ai, PiAPI, and Revana AI. Each route generates a five-second, silent landscape clip. Higgsfield uses 480p and the other four providers use 720p.

Run `everycli update` and reopen `everycli ui` if it is running. To serve the new routes, run `everycli worker-update --check`. When a compatible signed worker is offered, install it with `everycli worker-update`, apply your provider keys, and review the offers in Models. Route availability depends on the coordinator rollout and your provider account.
