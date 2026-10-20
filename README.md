# EveryFrame miner

Run an EveryFrame miner on Bittensor **SN117** with `everycli` and an attested
cloud worker. The worker receives jobs from the EveryFrame coordinator, calls
your configured generation providers, and returns signed completion receipts.
It does not run generation models locally, so you do not need a GPU.

With **everycli v0.2.0+**, commands are direct: `everycli doctor`, `everycli status`,
`everycli balances`, and so on. No command prefix is needed. Existing scripts using
the older namespace remain compatible. `everycli update` upgrades the CLI;
`everycli worker-update` updates the existing worker using its latest compatible
signed release (one-command flow in v0.3.7 and later).

New to mining? Run `everycli ui` for a guided setup page and status dashboard
in your browser (see [Set up in your browser](#set-up-in-your-browser-everycli-ui)).

This repository includes the worker, the CLI with built-in updates, 46 legacy
contracts, 11 flexible Studio video contracts, 12 Studio media contracts,
and ten provider adapters. Available jobs depend on the coordinator's
enabled models, current pricing, your credentials, and your active offers—not
every model in the registry is necessarily available for mining.

### In development: individual job history and clearer earnings

The next CLI build adds **Job history** and **Earnings** to `everycli ui`.
These changes are not part of the already-published v0.5.8 binary.

```sh
everycli jobs                            # newest 20 of your own attempts
everycli jobs --status failed --limit 20
everycli jobs --model direct/vivix/vivix-w1
everycli jobs --summary                  # existing all-time model counts
everycli earnings
```

History supports Previous/Next pages, status/model filters and attempt details:
timestamps, settings, output verification, review status and fixed failure codes.
Terminal output supplies a next-page cursor command; keep its model/status
filters. `--json` includes full timestamps and IDs. The signed private endpoint
does not expose prompts, media, customer references, provider request IDs,
credentials or other miners' attempts. Older coordinators fall back explicitly
to model counts; malformed signed history is rejected.

Earnings separates pending review, approved work and marked reward-eligible
value. The eligible value is a subset of approved work, not an extra payment.
A review flag does not prove finalized epoch inclusion or alpha received.
Provider exposure uses locked job quotes for confirmed/uncertain submissions,
including failed attempts; it is not an invoice. Unpriced and uncertain attempts
are counted separately. Actual provider bills, hosting costs, wallet payments,
alpha received and net profit are not invented from balances or work value.
These read-only reports never approve work, change offers, replay generations,
or alter validator accounting. No worker update is needed for these views.

### v0.5.8 model IDs and availability

The **Everyframe contract ID** is used with `everycli offer --model`. The
**Provider API ID** is the underlying provider's ID; for example,
`direct/openrouter/veo-3.1` maps to `google/veo-3.1`. Never paste a
`studio/` or `direct/` contract ID into the provider's API.

The model dashboard separates **Candidates / retired** from supported models.
Removed/deprecated models cannot receive new offers, but existing offers can
still be withdrawn and historical jobs remain readable. Provider catalog checks
run every 15 minutes; failed checks retry after one minute without paid requests.
OpenRouter uses the deployed key's filtered model list, Vivix uses its workspace
model list, and Fal uses its public model catalog (not proof of account access).
Other provider account access is explicitly unchecked. A listed model is not
proof of adequate funds, capacity, or successful generation.

These worker checks require the matching signed worker update; installing the
CLI alone does not enable them. Catalog checks never expand signed permissions
or silently switch the provider/model. Provider keys and raw errors are not
included in availability reports.

Unavailable offer pricing now explains the problem before confirmation; it never
changes part of a batch or reports the misleading "invalid integer" error.
See [v0.5.8 release notes](RELEASE-NOTES-v0.5.8.md).

### v0.5.4 deleted-workload recovery

Update with `everycli update`, then reopen `everycli ui`.

- If you deleted a VM directly in Phala and have no local deployment record,
  run `everycli reconnect` (or **Worker → Reconnect profile**). It checks your
  Phala workspace and asks before retiring the old binding with your hotkey.
  A 404 can mean the wrong workspace, so only approve if you deleted that VM.
- Recovery revokes the old authorization, preserves the miner identity and
  earnings history, and requires fresh attestation after deliberate redeployment.
  Active or uncertain jobs and a fresh worker heartbeat block retirement.
- New deployments can refresh an outdated unbound configuration before any
  paid request. Production OS versions come from the pinned coordinator's signed
  release; exact catalog hashes, TEE policy and development/GPU exclusions remain.
- A missing or unreachable OS catalog has a specific error. An unavailable OS
  must be reviewed server-side; the CLI never substitutes an arbitrary image.

Recovery does not create or delete cloud resources, end storage billing, or
resume jobs. Binary updates and automatic authorization repair do not silently
retire VM bindings. See [v0.5.4 release notes](RELEASE-NOTES-v0.5.4.md).

### v0.5.7 provider failure reporting

Worker failure reports preserve moderation, invalid-input and billing categories,
including Vivix and Fal errors. Retries never drop the reason or repeat the paid
generation. Structured error codes take priority over message text; echoed input
is excluded. Session renewal covers the coordinator's 12-minute maximum job.

Run `everycli update` for the CLI. These worker fixes require a separately verified
signed image: check `everycli worker-update --check`, then review and confirm
`everycli worker-update` when available. Updating the CLI alone does not change
your running VM. See [v0.5.7 release notes](RELEASE-NOTES-v0.5.7.md).

### v0.5.6 recovery and provider balances

Update with `everycli update`, then close and reopen `everycli ui` if running.
Fal balances now work with the normal generation key; BFL and Runway account
credits are also supported. Fal's generation-key balance endpoint is undocumented:
an unavailable or changed response is reported as unavailable, never zero.

An interrupted initial `provision_intent` with no app ID can now be recovered
with `everycli reconcile` (or **Re-sync operation** in the UI). The CLI requires
an unbound, idle coordinator and a successful Phala workspace inventory with no
matching workload. It archives only the uncommitted preparation, preserves keys
and the hotkey profile, and never retries VM creation. Afterwards, save any API
key changes and run `everycli reconnect` to refresh the signed deployment.
Do not delete profile files or bypass a refused recovery.

Worker telemetry uses the keys deployed to the worker. Updating everycli alone
does not update a running worker; check `everycli worker-update --check` for a
separately verified signed worker update. See [v0.5.6 release notes](RELEASE-NOTES-v0.5.6.md).

### v0.5.5 model dashboard and faster batches

- Unintegrated catalog entries are hidden. Integrated models that still need
  pricing, a worker update or a provider key keep their actionable readiness label.
- Overview shows all-time completed jobs by model and readable serving-fee tables.
  `everycli jobs` shows the same counts in the terminal; `--json` remains available.
  Completed means an accepted, verified output, not a payment or alpha reward.
- Models and **All models** show a badge when integrated models are added after
  your first catalog visit. Viewing All models acknowledges them on this browser
  and profile. Notification acknowledgments contain no keys.
- Batch discounts use one reviewed, atomic submission instead of a submission
  per model. Any changed pricing or revision rejects the whole batch. Unknown
  results are never automatically retried; refresh offers before trying again.
- Batch results can be dismissed, disappear after 12 seconds (30 seconds for
  errors), and clear when navigating away or changing model tabs.

Terminal batch example:

```bash
everycli offer-batch --models minimax/h3-max-turbo/text-to-video,minimax/h3-max/text-to-video --discount-pct 10
everycli jobs
```

Update with `everycli update`, then close and reopen `everycli ui`. These features
require the updated coordinator, already deployed on SN117. No worker redeployment
is needed. See [v0.5.5 release notes](RELEASE-NOTES-v0.5.5.md).

### v0.5.3 simpler setup and batch model editing

After updating with `everycli update`, close and reopen `everycli ui`.

- Setup shows **Select your hotkey**, then **Your hotkey** once attached, with
  its public address and verified UID. **Check registration** remains read-only.
- Deploy refreshes rejected authorization once with the attached, matching hotkey
  before any paid cloud request. Older profiles can locate the same hotkey in
  the standard wallet folder. Missing or mismatched keys still require attention;
  no other identity is substituted. Hosting still requires live-price approval.
- Attaching the same hotkey again with `init` refreshes an existing public profile
  while preserving saved API keys and deployment history.
- UI compute ceilings default to **$10/hour**, not a total spending cap. The
  signed instance type is unchanged, and storage is billed separately.
- Every Models tab supports selecting rows or all editable models in that tab,
  applying one discount, or withdrawing offers. One review shows the exact quotes.
  Changed quotes are not auto-approved. The original v0.5.3 batches were per-model;
  v0.5.5 replaces them with atomic batches. Uncertain requests are never retried
  automatically.
- Overview has four cards: **Worker**, **Status**, **Active offers**, and **Jobs**.
  Status distinguishes Ready, Working, Paused, Needs attention, and Unknown.
  Detailed readiness and attestation checks remain under Health checks.

See the [v0.5.3 release notes](RELEASE-NOTES-v0.5.3.md). This updates the local
CLI and browser UI, not your deployed worker or model offers.

### v0.5.2 dashboard and profile recovery

Run `everycli update`, then close and reopen `everycli ui`. The dashboard shows
your miner UID and a shortened hotkey with a copy button. UID is the last finalized
chain lookup; **Refresh UID** checks it again without signing a transaction.
Initialization and reconnect save this lookup automatically.

If the coordinator rejects a saved miner authorization (HTTP 401), this is
separate from a Phala API-token error. Check your computer's clock, then run:

```bash
everycli reconnect --wallet your-wallet --hotkey default
everycli doctor
```

Or use **Worker → Reconnect profile** with your saved hotkey. Reconnect requires
confirmation and a proof from the same local hotkey, verifies the current signed
binding, and preserves API keys, labels and deployment history. It never creates,
deletes, starts or updates a VM or resumes jobs. Interrupted local commits can be
finished by rerunning reconnect; unresolved cloud operations must be reconciled
first. A deleted worker remains deleted until you deliberately deploy again.

CLI updates do not silently reconnect or redeploy. Eligible native installations
also check for CLI updates when the UI starts (at most once per six hours); reopen
the UI to run the installed version. Source builds remain opt-in. See the
[v0.5.2 release notes](RELEASE-NOTES-v0.5.2.md).

### v0.5.1 setup and worker management

Update with `everycli update`, then open `everycli ui`. The UI checks keys with
free read-only requests before saving, preserves your setup step, and displays
compact worker names with local renaming. Rejected key replacements leave saved
keys unchanged; inconclusive checks display an explicit warning.

Both the CLI and UI can deliberately delete and replace a worker created on
another computer, with exact signed identity checks, a durable deletion journal
and fresh attestation before accepting work. See [replacement instructions](#move-management-to-another-computer)
and the [v0.5.1 release notes](RELEASE-NOTES-v0.5.1.md). Deletion is irreversible.
The worker runtime is unchanged from v0.5.0; do not replace a healthy VM just to
upgrade the CLI.

### v0.5.0 setup and reliability

The browser UI and miner reliability fixes ship together: safer provider failure
handling and balance checks, deadline-fenced submissions, longer media transfers,
non-expiring revocable authorization, crash-safe locks, update rollback copies,
local worker labels, logs, emergency stop and VM deletion. See the
[release notes](RELEASE-NOTES-v0.5.0.md) for upgrade and authorization-migration steps.
Update both the CLI and your existing worker; third-party VMs are not automatically
restarted. No redeployment is needed.

### v0.4.0 media coverage

The new worker supports H3 Max image/video/audio references, including measured
video frame counts for input-cost estimates; all seven Studio image models
(with ordered references and Ideogram precision masks); and ElevenLabs voice-over
and sound effects, MiniMax Music 3, and Stable Audio 2.5 instrumental scores.
Image models use Fal or OpenRouter; these audio models use Fal.

Run `everycli update`, then `everycli worker-update` to update the existing
workload. Updating the CLI alone does not change a running worker. Check
`everycli offers --json`, review the settings-specific price estimates, and
declare offers for the new `studio/media/` models you want to serve.
The new contracts require their own accepted offers; existing discounts are not
automatically copied. Routing also requires fresh attestation, credentials,
capacity, and budget. Local tools, unsupported models and unavailable miners may
still use the app's direct fallback; this is not a promise that all app traffic
will reach miners. Never deploy a second workload just to upgrade.

**Public rollout snapshot — 13 October 2026:** the shared coordinator release
contains 69 contracts, with 27 currently priced across Fal, OpenRouter and Vivix.
Compatible existing public miners can discover their own signed update; no
operator-provided invitation file is needed. Permissions change only after fresh
verified attestation. This is not an automatic fleet-wide worker upgrade.

From your original management profile:

```sh
everycli update
everycli worker-update --check
everycli worker-update
everycli offers
everycli doctor
```

If only three models remain available, check the **worker** release and current
attestation, not just `everycli --version`. Preserve your profile and existing VM.
An unbound or uncertain deployment must be reconciled before updating. Add the
needed provider keys and explicit offers for new contracts; old offers and the
worker's accepting/paused state are preserved.

Paid website tests passed for H3 Max Turbo portrait video and Grok Imagine image
through an updated owner miner. They do not certify all contracts or other miners.
See [validation and limits](VALIDATION.md#v040-public-worker-and-website-verification--13-october-2026).

## Worker failure diagnostics and safe failover

The safe-failover worker patch reports fixed submission error codes rather than
hiding all failures behind the job deadline. It requires the matching coordinator
deployment and a signed `everycli worker-update`; installing a CLI alone does not
update your VM. Use `everycli status --json` and inspect
`coordinator.recentFailures` after updating. Old failures cannot be reconstructed
when the previous worker discarded their cause.

Confirmed pre-generation authentication, billing or capacity rejections can move
the same job to another eligible miner, up to three distinct miners. A replacement
must have a current offer within the existing price/budget; customer inputs and
the original deadline are preserved. There must be an available alternative miner.
Moderation and invalid requests fail without cross-miner retries. Lost responses,
unknown submissions and already-submitted paid jobs are not blindly regenerated.

Fresh, current-policy attested zero-balance reports exclude unfunded provider
offers. Unsupported, stale and unavailable balances are not treated as zero.
Vivix W credits are checked separately from A credits. A known zero W balance
blocks W1 jobs; a positive balance is allowed, without assuming it can fund every
possible job. An unavailable query is not treated as zero. The workspace must be
the one associated with the worker's actual API key.

## Install everycli as a miner

Use a **prebuilt `everycli` binary**. You do not need a compiler, a language runtime,
Docker, a GPU, or a clone of this repository on your computer to run the CLI.
The miner worker runs separately in the deployed cloud workload.

Install with one command:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/everyframe-studio/everyframe-miner/releases/latest/download/everycli-installer.sh | sh
```

Prebuilt binaries and checksums are published on
[GitHub Releases](https://github.com/everyframe-studio/everyframe-miner/releases).
No GitHub account or token is needed to download them. A source-code archive is
not a prebuilt CLI download.

The installer detects your platform, verifies the binary's SHA-256 checksum, and
installs into `~/.cargo/bin` (or `$CARGO_HOME/bin` if configured). No development
toolchain is installed or required. It adds a PATH entry to `.profile`, `.bashrc`, and `.zshrc`
without duplicating the entry; symlinked profiles are left unchanged. If the bin
directory is already on your PATH, you can use `everycli` immediately. Otherwise,
run the source command printed by the installer or open a new terminal, then:

```sh
everycli --version
everycli --help
```

Linux x86-64/ARM64 builds require glibc 2.35 or newer (for example, Ubuntu 22.04+).
macOS Intel and Apple Silicon builds run on the corresponding release CI runners
(macOS 15 and 14 respectively). Older macOS versions are not validated. Native
Windows and Alpine/musl Linux are not supported; use a compatible Linux under WSL
on Windows. `curl` and either `sha256sum` or `shasum` are needed for installation.

To avoid modifying shell profiles, pipe into `EVERYCLI_NO_MODIFY_PATH=1 sh`
instead; add the bin directory to
PATH yourself. Fish users should use this option and configure their Fish PATH.

### Set up in your browser (everycli ui)

Prefer a guided page to terminal commands? After installing, run:

```sh
everycli ui
```

It opens a local page with:

- a step-by-step **setup guide** (hotkey check, Phala token, provider keys, profile, deploy, activate, models, resume)
- a live **status dashboard** (health checks, jobs, balances and earnings)
- **model offers**, checked **API keys** and **worker actions** (pause, resume, update, apply keys, shut down, delete and replace)

Every action runs the same everycli command and asks the same questions. Anything
that costs money shows the live price and needs you to type `yes`, as in the
terminal. The page and the terminal share one profile, so you can switch between them.

The page listens only on `127.0.0.1`, and the link printed in your terminal works
once (opening it prints a fresh link for another tab). Keys you enter go straight
into your owner-only profile and are never shown again. Closing the page or
stopping `everycli ui` (Ctrl-C) never stops your worker.

On a remote server, forward the port from your own computer, then open the printed link:

```sh
ssh -N -L 47117:127.0.0.1:47117 <user>@<server>
```

Options: `--port N` (default 47117, or the next free port), `--no-open` (print
the link without opening a browser), plus the usual `--network` and `--state-dir`.

### Upgrade everycli

```sh
everycli update          # Install the latest stable CLI; no login needed
everycli update --check  # Check for an update without changing anything
everycli --version
```

The built-in updater verifies the release checksum and atomically replaces
the executable in place. It does not access miner credentials, deploy anything,
change profiles, or update running workers. A failed download or checksum leaves
the old executable intact. It will not automatically downgrade. Run updates as
the account that owns the installation; no sudo is needed for a normal install.

Normal output is readable text, with download percentage/size and verification
and installation stages. If the server does not provide a size, it shows bytes
downloaded instead of guessing a percentage. Progress goes to stderr; redirected
logs use plain milestone lines. `everycli update --json` and
`everycli update --check --json` suppress progress and print one JSON result for scripts.
Human-readable update messages consistently prefix versions with `v`, such as
`v0.3.3 → v0.3.4`. JSON version fields retain their existing format.

An upgrade is performed by the **currently installed** CLI. Older versions may
still print their old output during that one upgrade; the new display applies
once the new executable is installed.

`everycli update` upgrades the **CLI**; `everycli worker-update`
updates a **deployed worker** using an approved signed release. If a documented command is missing, check
`everycli --version` and upgrade (rerun the installer for older CLIs without an
`update` command).

Checksums detect corrupted or mismatched downloads; they are not independent
publisher signatures. The installer and updater trust this GitHub repository
and HTTPS.

### Automatic CLI updates (v0.3.8)

Native user installations check for the latest stable release in the background
after successful interactive commands, at most once every six hours. Downloads
are checksum-verified and atomically installed; the new version runs on the next
invocation. Your current command completes normally, even when an update fails.

```sh
everycli auto-update            # Show settings and the last attempt
everycli auto-update --disable  # Opt out; manual everycli update still works
everycli auto-update --enable   # Restore the default
everycli auto-update --json     # Structured settings/result, no background work
```

`EVERYCLI_DISABLE_AUTO_UPDATE=1` also disables background updates. No background
check runs for JSON, CI, non-interactive, help, version, or explicit update commands.
Auto-updates are restricted to owner-managed native installs in
`${CARGO_HOME:-$HOME/.cargo}/bin/everycli` or `~/.local/bin/everycli`.
Build directories, package-manager paths, custom symlink launchers and unsafe
permissions remain manual. Settings and the last result live separately from
miner profiles in `~/.config/everycli-updater/`. No daemon or system service is installed.

Only the CLI executable is updated: cloud workers, saved keys, model offers,
discounts, running jobs and routing are never changed. Install this feature once
using `everycli update` after its release; older CLIs cannot gain auto-update
behavior without that first manual upgrade.

## Guided setup and command help (v0.3.4)

For a new miner, use the interactive wizard:

```sh
everycli setup --wallet my-wallet --hotkey default --max-hourly-usd 0.05
```

It checks registration, prompts privately for keys, initializes the signed
profile, then shows the live hosting quote for your explicit confirmation.
Optionally add `--provider fal` (or another generation provider) to configure
just that provider plus Phala. Use a Phala **workspace token from Settings →
API Tokens**, not a Private AI inference key. The hourly ceiling excludes
storage and is not a total budget.

The wizard stops on a failed step, an existing/uncertain deployment, or an
existing remote worker. It does not register on-chain, automatically activate
keys, resume paid work, or submit generation. Keep the original profile after
any interrupted deployment; use `status` and `reconcile` before retrying.
Deployment is step 4, not the end of setup. The final message gives steps 5–11
for readiness checks, activation, fresh attestation, resuming and model offers;
it does not mark the miner ready merely because the VM was created.
To show the checklist again from the original
management profile, without creating a VM or changing the worker:

```sh
everycli setup --continue
```

This reads the local deployment journal, not live readiness. Use the same
`--network` and `--state-dir` for the follow-up commands. It stops on an uncertain
operation until you reconcile it. Follow only the steps still needed; it never
activates keys or resumes paid work for you.

Every command has its own help and example, without needing a profile:

```sh
everycli help
everycli help deploy
everycli deploy --help
everycli setup --help
```

For automation, use the individual commands below instead of the interactive
wizard. Commands that need confirmation require `--yes` when there is no terminal
or when using `--json`. New `setup` is interactive and does not accept `--yes` or
`--json`; the read-only `setup --continue` checklist also supports `--json`.

Registration, key changes and update checks print readable summaries by default;
use `--json` for structured output. Key prompts show **Saved — Enter keeps existing**
or **Not saved — Enter skips**, without exposing key values. Removing an absent
key reports that nothing changed and does not ask for confirmation.

`providers` shows a table of saved-key presence, release permissions, and the
reason a provider is unavailable. Before initialization, release permission is
**Unknown** (`null` in JSON), not denied. A saved key is not proof of API access
or worker readiness. Use `--json` for structured output.

CLI success exits with `0`; unsuccessful checks and command errors exit with `2`.
Missing-profile responses use the same error code, message, and exit status
across commands. JSON diagnostics contain `ok`, `error`, `message`, and `next`,
with `file` when a local file is involved. No credential contents or raw provider
responses are printed. `doctor` may also include its structured `checks` list.

## Balances and offer tables (v0.1.5+)

### Check whether your miner is ready

```bash
everycli health          # Green PASS / amber WARN / red FAIL readiness checklist
everycli status          # Deployment, routing, offers and job activity as tables
everycli health --json   # Structured checks; exit 0 only when all checks pass
everycli status --json   # Full status fields, unchanged for scripts
```

All green in `health` means the current readiness checks passed: local profile,
signed configuration, coordinator, cloud workload, fresh attestation/heartbeat,
routing and support for your active model offers. Missing capability evidence is
an amber warning, never an assumed pass. `health` exits with code 2 if any check
fails or cannot be verified. These commands are read-only: no activation, restart,
new deployment or paid generation is performed.

Readiness does **not** guarantee traffic, provider funds, future successful jobs or
earnings. Historical accepted jobs are displayed separately as evidence of past
work, not a live test of every model. Zero active jobs is normal when idle. Use
`everycli balances` to check supported account balances separately. A saved key
not allowed by your worker release prevents an all-green result, but does not by
itself stop other supported providers from serving.

Colors appear in supported interactive terminals. Piped output stays plain text;
set `NO_COLOR=1` to disable colors. State labels remain visible without color.

`everycli offers` displays each model, provider, offer status, discount and
readiness. With an updated coordinator, the table includes Studio's API-backed
catalog, including integrated models not yet enabled for your worker.
`Pricing required` and `Worker release required` are **not** runnable offers.
Discovery does not enable models or place bids. Local-only rendering is excluded.
Older coordinators still return only their admitted model list.

Models prefixed with `studio/` use settings-bound video, image or audio contracts:
supported formats, resolutions, lengths and reference media are bound to the
signed job. They require a matching signed **worker**
update, the provider key, and a new explicit offer; an old text-only offer does
not authorize them. Updating the CLI alone does not update the deployed worker.
Run `everycli worker-update --check`, then review `everycli offers --json`.
For video, the base quote is a five-second reference point, not a fixed price for
every job. Image and audio contracts use their own units and settings.
The coordinator calculates each job's value and spending reservation from
its actual settings and reference inputs before applying your discount.
Unsupported combinations and unpriced reference inputs remain unavailable.

The discount applies to the base miner reward—not the provider's API charge.
Use `--json` for all readiness blockers and local key/release-permission fields.

**Known customer-billing limitation (13 October 2026):** a 10% miner bid reduced
the tested serving value from $0.20 to $0.18, but Studio still charged 30 credits,
not 27. Passing the miner discount through to customer credits is intended but
not implemented in this tested path. Do not advertise that saving as delivered.

If `doctor` says a saved provider key is not permitted by the signed deployment,
this is **not an API authentication failure**. Updating everycli alone does not
update the cloud worker or its permitted keys. Keep the existing profile and
worker; apply the signed app-specific worker update when available, reconcile,
activate, wait for fresh attestation, then resume. Do not deploy a second VM.

```sh
everycli balances          # Remaining account credits in USD
everycli offers            # Model / provider / offer / discount / readiness
everycli balances --json   # Includes timestamps and stale flags
```

Balance reads support Fal, OpenRouter, Vivix (W credits), BFL, Runway and Phala
prepaid credits. BFL and Runway credits are converted at their published one US
cent per credit. MiniMax, Replicate, Google, Luma and ElevenLabs publish no
balance endpoint this tool reads yet, so they display `unsupported`; failed checks display
`unavailable`, never a fabricated zero. Balances are account-level and may be
shared by multiple miners; do not add them across miners or treat them as profit.

Failed local checks include a safe reason and next step, such as HTTP 401
(authentication rejected), HTTP 403 (access forbidden), HTTP 429 (rate limit),
DNS failure, timeout, connection/TLS failure, or an invalid balance response.
`--json` includes each row's `diagnostic.code`, `message` and `next`.
Failed coordinator reads or publishes include `remoteError`; local balance
results remain visible even when publishing fails. The command exits with code 2
if a balance is unavailable, no rows are available, or a requested publish fails.
Unsupported providers and stale successful snapshots are labeled separately.
Remote snapshots from the existing protocol do not contain failure reasons:
refresh from the key-holding device for a live diagnosis. No raw provider
response, credential or authentication header is printed or published.

Phala includes paid and granted credits, not a post-paid spending limit; outstanding
invoices are not deducted here. OpenRouter's result is account credit, not a per-key budget.

Your normal Fal key reads the account balance. OpenRouter needs a management key
for account credits. Optional billing keys can be stored privately on your
management device (a Fal admin key reads Fal's documented billing API instead):

```sh
everycli set-api-keys --provider fal-billing
everycli set-api-keys --provider openrouter-billing
```

These billing-only keys are **never deployed to the worker**. Do not replace your
generation key with a billing/admin key. Phala uses the existing locally saved
hosting key. Keys are saved owner-only, unencrypted, like other CLI credentials.

To view balances on a second device using just your hotkey, explicitly sync from
the initialized profile on the device holding those API keys:

```sh
everycli balances --publish
```

Only fixed provider names, amounts, statuses and timestamps go to your miner's
authenticated coordinator view—no API keys or raw provider responses. Then run
`everycli balances` on the hotkey-only device. These are **owner-reported
snapshots**, not live provider queries from the second device. Repeat `--publish`
to refresh; snapshots are marked stale after 15 minutes. No background sync or
worker restart is performed automatically. Use the same `--state-dir` on each
command when managing multiple profiles. Publishing requires the coordinator
balance endpoint; reading still works against older coordinators with local keys.

## Set up a miner

Self-service onboarding requires **everycli v0.1.4 or later**. Run `everycli update`
first if you installed an older release. Updating the CLI does not migrate an
existing token-based worker automatically.

Before deployment, you need:

- A miner hotkey registered on SN117. `register-hotkey` checks and records subnet
  membership; on-chain registration is done separately using your wallet tool.
- A funded Phala Cloud account and its API key for the attested workload.
- At least one funded provider account supported by your deployment configuration.
- Your local miner hotkey file. No invitation file or `MINER_TOKEN` is needed.

The downloads and public worker images do not require registry credentials.
`init` proves hotkey ownership, verifies finalized subnet membership on the server,
and automatically downloads the signed deployment configuration. The CLI checks
its pinned coordinator signature. The deployed worker must then pass exact
image/TEE attestation before provider keys are enabled. Downloading the CLI alone
does not register or activate a miner, and no coldkey transaction is signed.

### 1. Check your hotkey

```sh
everycli register-hotkey --wallet my-miner --hotkey default
```

This reads the local hotkey's public address, checks the pinned chain genesis,
and verifies SN117 membership at a finalized block through the Bittensor RPC.
It records the UID and local keyfile path, never a seed. If you already registered
elsewhere, use `--hotkey-ss58 YOUR_SS58_ADDRESS` instead; no local wallet or
`btcli` is needed for this public lookup. Local signing still requires your
hotkey file during `init`.

If the hotkey is absent, the command exits unsuccessfully and, when wallet names
were supplied, prints the `btcli subnet register` command for you to run yourself.
It never creates a wallet, pays registration fees, or signs an on-chain transaction.
This membership check is not proof of ownership or coordinator admission.
For testnet SN566, pass `--network testnet` to each command; networks are not sticky.

### 2. Set your API keys

```sh
everycli set-api-keys
```

The CLI prompts for Phala Cloud and all ten generation providers with hidden
input. Enter skips a provider or keeps its existing key. You need a funded Phala
account and at least one release-approved generation provider, not all ten.
Keys are saved in an owner-only local profile (`credentials.json`, mode `0600`);
you do not need to create or maintain a `.env` file. Local storage is not encrypted
at rest. Never commit the profile or share its contents.

To change just one provider, or remove a locally saved key:

```sh
everycli set-api-keys --provider minimax
everycli set-api-keys --provider phala
everycli remove-api-key --provider fal
everycli providers
```

Provider names: `phala`, `fal`, `minimax`, `openrouter`, `bfl`, `replicate`,
`google`, `runway`, `luma`, `elevenlabs`, `vivix`. For automation, pipe a secret manager's
output into `everycli set-api-keys --provider NAME --stdin`; never put the
key itself in command arguments or shell history. `--json` never enables prompts.

Saving or removing keys is local-only. To apply saved generation keys to an
already activated worker, run `everycli apply-api-keys`. This asks for
confirmation, drains work, refuses to restart while jobs remain, checks the
reviewed workload, and encrypts credentials to its pinned key. Wait for fresh
post-restart admission, then run `everycli resume`. Only providers allowed
by the signed deployment can be applied. At least one must remain configured;
if retiring the last provider, stop the worker instead. Removing a local key
does not revoke it at the provider—revoke leaked credentials there immediately.

### 3. Initialize and deploy

```sh
everycli init
everycli doctor
everycli deploy --max-hourly-usd 0.06
everycli status
```

`init` reuses the hotkey path recorded in step 1 and the saved API keys. If you
used the public-address lookup, also provide `--wallet my-miner --hotkey default`
or `--hotkey-file FILE`. Existing automation can still use `--secrets-file FILE`
to import an owner-only dotenv file; omitted credentials are preserved. The import
does not execute shell expressions. Public-image deployments never send registry
credentials, even if stale registry keys exist locally.

`doctor` can report that no workload is deployed
before the initial `deploy`. Review its diagnosis rather than treating every
pre-deployment warning as an installation failure. The hourly ceiling is an
example; deployment is refused if the quoted compute rate exceeds it.

After a new miner's `init`, `status` shows `phase: not_deployed` and a null local
`appId`; the coordinator has no attestation, heartbeat or active model offer yet.
Continue with `deploy` above, review the hosting quote, and follow the staged
activation steps below. `doctor` stays `ok: false` until the miner is ready to
serve; initialization alone does not start mining. Older CLIs may show
`Phala workload: invalid_string` for this missing deployment ID—it does not mean
your Phala credential is invalid. If the coordinator already has a real workload
app ID, use the original management profile instead of deploying a duplicate.
Preserve interrupted deployment records and run `everycli reconcile` before retrying.

Cloud errors identify Phala and report safe HTTP or transport reasons, without
guessing that every failure means an invalid key or insufficient balance. A
connection error can include TLS failure; it is not proof of an authentication
problem. The CLI connects directly (proxy environment variables are not used).
After any uncertain deployment failure, inspect `status` and use `reconcile`
before retrying; an error response or timeout is not proof that no VM was created.

If `status` or `doctor` reports **No local miner profile found**, connect on that
device with `everycli init --wallet my-miner --hotkey default`. This is a local
setup state, not proof that the hotkey is unregistered on-chain. Profiles belong
to the current OS user, network and `--state-dir`; use the same selection as
before if already initialized. Missing credentials, invalid files and unsafe
permissions are reported separately. Do not delete an existing profile or deploy
another worker merely to restore status access. `--json` retains structured
diagnostics; an incomplete or unreadable profile exits with code 2.

`--wallet my-miner --hotkey default` reads
`~/.bittensor/wallets/my-miner/hotkeys/default` locally. Alternatively, use
`--hotkey-file /absolute/path/to/hotkey` instead of those two options. The current
reader supports an unencrypted sr25519 wallet JSON containing `secretSeed`;
the file must be owner-only. Encrypted keyfiles are rejected rather than silently
decrypted or uploaded. Never paste a seed into command arguments.

The hotkey signs separate, revocable console and worker authorizations. New v2
authorizations do not expire **when the signed worker deployment explicitly
enables `hotkeyDelegationVersion: 2`** after coordinator and worker upgrades.
Until then, the CLI preserves compatible v1 authorizations and enforces their expiry.
The UI omits expiry cards; a CLI upgrade alone cannot remove an existing signature's expiry. Only the
worker's restricted delegate is sent through Phala's encrypted environment;
the hotkey and console delegate stay on your computer. This does not sign a
blockchain transaction or access your coldkey. Short-lived request proofs,
onboarding challenges and TEE sessions still expire. Revoking the hotkey's
coordinator key version invalidates its delegates. Legacy v1 delegates retain
their signed expiry: after a compatible signed worker update, migrate with `everycli init` using your hotkey, then
`everycli apply-api-keys` to install the new worker authorization. Check fresh
attestation before resuming. Updating the CLI alone does not change a running VM.

### Worker labels, discovery and troubleshooting

```sh
everycli offers                     # Signed catalog; pre-setup discovery needs coordinator support
everycli label --name "studio-worker-1"
everycli status
everycli health
everycli logs                       # Bounded structured worker events
```

Labels belong to the selected local profile (`--state-dir`); they do not rename
the signed Phala VM or change attestation. Use one management profile per worker.
Catalog work values are not guaranteed earnings, provider prices, or alpha payouts.
Logs never enable public logging and omit raw boot logs and secret-bearing URLs;
use the authenticated Phala console for private/boot logs.

### Stop billing and uninstall

```sh
everycli stop                       # Drain active work, then stop compute
everycli stop --force               # Emergency stop; coordinator may be offline
everycli delete --app-id EXACT_APP_ID # Irreversibly remove this VM and its storage
```

Emergency stop can lose already-paid in-flight work. Stop does not delete storage.
Deletion requires the exact app ID and confirmation, verifies the signed
VM identity, and never deletes other resources. After deletion, confirm removal
and final charges in Phala Billing. Provider subscriptions and other VMs are separate.
An uncertain operation is journaled; do not deploy a duplicate or delete the profile.

### Move management to another computer

Initialize the same hotkey and configure a Phala **workspace API token** for the
account hosting your worker. `everycli status` lists matching cloud workloads.
To permanently replace one, use `everycli delete --app-id EXACT_APP_ID`, then
repeat that command until it reports `deleted`. Deletion progress is saved locally;
a timeout is not proof of removal. Then use `everycli deploy --max-hourly-usd LIMIT`.
The new VM must pass fresh attestation and activation before accepting work.
Existing model offers remain unchanged. This replaces the VM; it does not import
the old computer's management profile. Never delete a worker you intend to keep.

The same flow is available in `everycli ui` as **Delete and deploy again**.
Both permanent deletion and new hosting charges require confirmation.
The UI checks API keys with free read-only requests before saving: rejected keys
are not saved; inconclusive checks save with an explicit warning. Restricted
generation keys may not permit account queries, so this is not a generation test.
Worker names can be shortened in the display or renamed locally without changing
the signed VM identity.

To uninstall, first stop/delete every managed VM as appropriate, then run
`everycli auto-update --disable`. Locate the installed executable with
`command -v everycli` and remove that exact file (normally `~/.cargo/bin/everycli`).
Optionally remove its adjacent `everycli.rollback` backup and the installer-added
shell PATH line if nothing else uses it. Keep your wallet and management profiles
until all cloud billing is confirmed ended; uninstalling the CLI does not stop VMs.

CLI updates validate the downloaded executable before swapping it in and keep
the previous executable as `everycli.rollback` beside it. Source-built binaries
are not self-replaced; rebuild from source or explicitly reinstall the official
distribution. CLI updates and worker updates remain separate operations.

Older token-based deployments remain compatible until explicitly migrated.
Simply deleting a token from an old profile will not migrate it: the coordinator
binding, deployment configuration, and worker image must all support hotkey auth.

Activation happens in stages. Check `status` and `doctor` between them; do not
run through admission failures or repeatedly retry an uncertain deployment.
Workload attestation is automatic, not a manual approval request. If activation
cannot continue, the CLI identifies the failed check: pending or rejected
verification, stale attestation, an unavailable session, a missing heartbeat,
clock skew, a disabled miner, or a workload/profile mismatch. Follow that
diagnostic rather than deploying another VM. Provider keys stay protected by
the same attestation and workload-binding checks.

```sh
# Once status shows accepted attestation:
everycli activate
# After fresh post-restart admission:
everycli resume
everycli offers
# Example: offer one enabled model at a 10% discount:
everycli offer --model studio/fal/minimax/h3-max-turbo/text-to-video --discount-pct 10
everycli earnings
```

`resume` permits work but does not create a model offer. Choose an available
model from `offers`, then submit your bid. The model above is an example, not
a guarantee that it is enabled for every deployment.

`--max-hourly-usd` is a **compute-rate ceiling, not a total spending budget**.
Storage is extra and may remain billable after shutdown. There is no automatic
total-budget cutoff in this CLI. Provider bills do not decrease when you discount
your mining offer; traffic and earnings are not guaranteed.

### Pause or stop

```sh
everycli stop --drain-only  # Stop accepting new work; keep hosting running
everycli stop              # Request shutdown when no active jobs remain
everycli reconcile         # Check whether the requested operation completed
```

If jobs are still active, `stop` returns a draining status and leaves the VM
running. Wait for those jobs to finish, then run `stop` again; it does not
schedule a later shutdown automatically.

Shutdown does not delete the VM or its storage. Check Phala Cloud for retained
resources and ongoing charges. To restart the saved workload, use
`everycli start --max-hourly-usd 0.06`, check admission, and resume when ready.

### Command reference

| Command | Purpose |
| --- | --- |
| `ui` | Local browser page: setup guide, status dashboard, offers, keys and worker actions (127.0.0.1 only) |
| `setup` | Guided initial deployment with explicit confirmation; `--continue` reprints a read-only post-deployment checklist |
| `register-hotkey` | Check finalized subnet membership and record the public identity; no chain transaction |
| `set-api-keys` | Privately prompt for all keys or one provider; save locally |
| `remove-api-key` | Remove one local credential after confirmation; does not revoke it remotely |
| `apply-api-keys` | Explicitly drain and apply saved keys to an activated, reviewed worker; fresh admission required |
| `init` | Verify deployment configuration; sign local hotkey delegations and store credentials |
| `health` | Color-coded readiness checklist; `--json` for scripts; exit 0 only when all checks pass |
| `status` | Deployment, routing, offers and job activity tables; `--json` for full fields |
| `doctor` | Detailed diagnostics for readiness, admission, routing, and cloud state |
| `providers` | Show credential presence and signed-release permissions, never key values |
| `balances` | Show USD balances and native Vivix W credits; `--publish` syncs private snapshots |
| `offers`, `offer` | Inspect, set, or withdraw a revision-checked model bid |
| `offer-batch` | Review and apply multiple model bids atomically; no automatic retry |
| `jobs` | Individual attempts with filters/pagination; `--summary` for all-time model counts (development build) |
| `earnings` | Work/review accounting and estimated exposure, separate from unmeasured actual income/costs; no payouts |
| `deploy` | Create a pinned workload with provider credentials disabled |
| `activate` | Release encrypted provider keys only after fresh admission |
| `resume` | Permit new work after post-restart admission |
| `stop` | Drain; request shutdown only when idle. `--drain-only` keeps hosting running; `--force` permits emergency shutdown that may interrupt paid work |
| `delete` | Permanently remove the verified VM and storage; requires `--app-id ID` and confirmation; repeat to check completion |
| `logs` | Read bounded, sanitized structured worker events |
| `label` | View the local display name or change it with `--name NAME`; never changes the signed VM identity |
| `start` | Restart the saved stopped workload, with a reviewed hourly ceiling |
| `update` | Upgrade the CLI executable only; `--check` checks without installing |
| `worker-update` | Discover and install a signed update on the existing VM, then restore its accepting/paused state; `--check` is read-only |
| `reconcile` | Inspect uncertain operations without replaying paid cloud mutations |

Mainnet SN117 is the default. Use `--network testnet` for SN566. Default state
paths match the original CLI: `~/.config/everycli-mainnet117` and
`~/.config/everycli`. `--state-dir` selects an explicit profile. Testnet alone
honors `EVERYCLI_DIR`. State directories must be owner-only (`0700`); files `0600`.
Back up an existing profile before switching CLI implementations. Do not operate
the same profile with two CLI processes at once.

Commands accept `--json` for machine-readable output. Lifecycle operations and
offer changes require interactive confirmation or `--yes`; `--json` does not
imply consent. `init` with a local hotkey and saved credentials does not prompt
for confirmation and does not create a paid VM. The optional legacy deployment
file import remains available for existing managed deployments.
Exit codes: `0` success, `2` command errors or unsuccessful checks (including an
unregistered hotkey). Registration, key changes, updates, provider/offer/balance
tables and contextual errors use readable text by default. Other lifecycle
results retain formatted JSON; `--json` always requests compact structured output.

`earnings` shows coordinator accounting, not proof of an on-chain alpha payment.
Reward eligibility, validator weight submission, reveal, and chain emissions are
separate steps. The CLI does not initiate payouts.

## Providers

| Provider | `--provider` | Credential variable (optional file import) |
| --- | --- | --- |
| Fal | `fal` | `FAL_KEY` |
| MiniMax (direct API) | `minimax` | `MINIMAX_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
| Black Forest Labs | `bfl` | `BFL_API_KEY` |
| Replicate | `replicate` | `REPLICATE_API_TOKEN` |
| Google | `google` | `GEMINI_API_KEY` |
| Runway | `runway` | `RUNWAYML_API_SECRET` |
| Luma | `luma` | `LUMA_API_KEY` |
| ElevenLabs | `elevenlabs` | `ELEVENLABS_API_KEY` |
| Vivix (direct API) | `vivix` | `VIVIX_API_KEY` |

Use the key for the provider serving the contract, not just the model's brand.
For example, a MiniMax model served through Fal uses `FAL_KEY`; the direct
MiniMax adapter uses `MINIMAX_API_KEY`.

Exact contracts and immutable parameters are in `config/models.json`.
Credential names are centralized in `src/invitation.rs`. Possessing a provider key
does **not** enable a model: the operator must separately approve and price it.

After initialization, check which providers are configured and permitted, then
inspect the available model offers:

```sh
everycli providers  # Reports key presence and permissions, never key values
everycli offers
```

### Vivix W1

Available in **everycli v0.2.4+**. Run `everycli update` first if you have
v0.2.3 or earlier. Updating the CLI does not enable W1 on a deployed worker.

Save your **Vivix API key**, not a Fal or OpenRouter key:

```sh
everycli set-api-keys --provider vivix
everycli providers
everycli offers
```

The worker adapter uses the direct `api.vivix.ai` API. Its model contract is
`direct/vivix/vivix-w1`: text-to-video, 720p landscape (16:9), with audio,
and integer durations from 5 to 15 seconds (default 5). Existing W1 samples
encode at 1264x720; the output contract accepts that size as well as 1280x720.
The seed is sent in
the request envelope. Image/audio references and first/last frames are **not**
part of this miner contract; `vivix-w1-fast` and `vivix-w1-ultrafast` are not
aliases for it. See [Vivix's API documentation](https://docs.vivix.ai/video-generation/world-video-generation).

This is a **candidate contract**, tested with synthetic responses, not a claim
of live mining availability. The coordinator must add matching contract/pricing,
allow `VIVIX_API_KEY` in its signed deployment, and admit the new worker image
before it can route W1 jobs. Existing signed deployments are not modified by
saving the key. `providers` distinguishes a saved key from a release-permitted key;
`offers` is the source of truth for available mining models. Updating everycli
alone does not update a deployed worker.

For an existing miner, use the original management profile and run:

```sh
everycli worker-update --check
everycli worker-update
```

The one-command flow is available in **everycli v0.3.7 and later**. It
fetches a compatible signed package for your miner and app automatically: no
manual JSON download. After one confirmation it drains current jobs, updates the
same VM, waits for verified attestation, reactivates previously enabled keys,
waits for fresh post-restart attestation, and restores the previous accepting or
paused state. A paused miner stays paused. Existing offers and discounts are
unchanged; newly available models still need their own offers. No VM is created
or deleted, no stopped VM is started, and no paid test generation is submitted.

Progress is readable by default; `--json` emits only the structured result.
Non-interactive updates require `--yes`. Waiting is bounded to about ten minutes;
rerun the same command with the same profile to continue a saved update. A lost
cloud commit or key-activation response is not blindly replayed. v0.3.8
also recovers v0.3.7's `update_provision_intent`: it verifies the exact old VM is
still running the original signed configuration before retrying preparation.
Preparation only caches a candidate; committing/restarting is a separate operation.
Run `everycli reconcile` to perform that read-only cloud verification explicitly.
A genuinely different VM/configuration still fails closed. Keep the profile and journal.
The OS releases the operation lock after a process exits or crashes; a leftover
lock file alone does not block the CLI. Never delete it to bypass a live operation.

If no compatible package is published, the command says so and changes nothing;
that is not a claim that all providers are already supported. The advanced
`everycli worker-update --release FILE` path remains available and retains its
manual reconcile/activation steps. Do not redeploy a second VM or edit signed
configurations to enable a key. Provider submissions are never automatically
retried after an ambiguous response, preventing duplicate billable jobs.

From v0.4.1, `everycli balances` reads Vivix's read-only `/v1/balance` endpoint
and displays **W credits**, not A credits or an inferred USD amount. The endpoint
is undocumented: a missing/duplicate W entry, changed response shape or request
failure is `unavailable`, never an invented zero. Workspace IDs and raw responses
are not published.

Update both components with `everycli update` and `everycli worker-update` from
the original management profile. The updated worker checks deployed provider
balances once per minute and sends signed, attested telemetry. The coordinator
excludes a provider with a fresh confirmed non-positive balance (W credits for
Vivix, USD for Fal/OpenRouter/BFL/Runway), including a recheck before issuing a start grant.
This is **zero-only gating**: any positive balance remains eligible; it does not
compare the balance with the requested job's cost. A credits cannot fund W1.
Stale, unavailable and unsupported results are not assumed to be zero. This
is not a guarantee against concurrent spending on the same provider account.

## Tests

For contributors only; miners installing a release do not need a compiler or
to run these checks.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Tests use synthetic keys and in-memory transports, not live accounts or paid
generation. They cover original cross-language signature/encryption fixtures,
all 45 original contract hashes plus the Vivix contract and adapter mappings, credential isolation, nonce binding,
stable completion receipts, ambiguous submission recovery, durable cloud intents,
admission, lifecycle commands, bids, filesystem security, and fail-closed startup.

## Worker releases and security

Duration-capable workers can serve every whole-second duration offered by Studio
for H3 Max/Turbo (up to 15 seconds) and Seedance 2.5 (up to 30 seconds).
The model offer remains quoted per base five-second clip: job value and provider
budget scale with the requested duration, and the same discount applies.
Other supported-model, resolution, aspect-ratio and reference restrictions still
apply. Older workers continue receiving only their original fixed-duration jobs.
This requires updating the **deployed worker**, not just the CLI.

CLI downloads and deployed worker images are separate releases. Updating the CLI
does not change the image selected by your deployment configuration.

`config/release.json` pins the SN117 coordinator and chain. Self-service setup
selects the approved digest-pinned worker automatically; miners do not build an
image or supply registry credentials. A locally modified image is not automatically
admitted. Unknown OS measurements or KMS CA identities fail attestation rather
than being trusted on first use. The KMS runtime probe can vary between boots;
the coordinator still verifies the hardware configuration binding and complete
boot transcript. Phala remains the trusted KMS operator; see [SECURITY.md](SECURITY.md).

The worker never receives your hotkey private key. The CLI reads the hotkey only
to sign scoped authentication, never to move funds. Keep hotkeys, credentials,
and private state out of Git. See [SECURITY.md](SECURITY.md) for security guidance,
[COMPATIBILITY.md](COMPATIBILITY.md) for protocol details, and
[RELEASING.md](RELEASING.md) for maintainer release procedures.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
See [LICENSE-NOTICE.md](LICENSE-NOTICE.md) for third-party licensing guidance.
