# Model catalog refresh

This compatibility worker supports signed model definitions supplied by the
coordinator. Subsequent additions using a supported provider adapter do not
require a new CLI binary, worker binary, VM, or process restart. The Models page
already reads the coordinator's catalog.

The worker fetches definitions on its next claim after attestation and every
five minutes while idle. It never changes definitions during provider generation.
A failed refresh keeps the last usable definitions and retries metadata after a
minute; it never repeats a generation. The compiled registry is a bootstrap
fallback and a set of measured adapter templates, rather than a limit on model
identities.

## Supported data changes

The existing native/schema adapters (including WaveSpeed, Runway and BFL), Google
Interactions, and Studio video, media and tool settings engines can adopt new
model IDs. The server supplies a reviewed definition and a canonical example;
the worker checks both using its existing request validator and body builder.

The signing key is pinned by the signed worker release. Online replies bind the
chain, miner, session, nonce and timestamp. Definitions cannot introduce a new
provider, authentication method, origin, download permission, executable adapter
or task mode. A model-specific API path may change only within a compiled
generation namespace; fixed protocol paths stay fixed. A new provider protocol
still requires a reviewed worker release.

An accepted model ID is immutable. A changed execution contract needs a new ID.
Old definitions remain available for polling and recovery even after removal
from current discovery. The cache is signed, private to the worker UID, written
atomically, and checked again on process restart. It contains model data only.
A container or VM recreation may discard `/tmp`; the worker can fetch recovery
definitions even while the coordinator records an active attempt. Never remove
historical model definitions from coordinator storage during job recovery.

The process holds at most 512 total model IDs and 4 MiB of additional definition
JSON, with a 128 KiB per-definition limit. These bounds include the bootstrap
registry. Increase them only through a reviewed compatibility release.

## Authorization and rollout

Catalog installation does not create offers, choose discounts, activate keys,
change budgets, or resume a paused miner. Existing explicit operator model
allowlists remain fixed. Under offer-based routing, the coordinator operator can
opt a miner into managed catalog authorization once:

```http
POST /control/miners/<minerId>/catalog
Authorization: Bearer <operator control token>
Content-Type: application/json

{"managed":true}
```

Only compatible additions based on an already authorized adapter template gain
model authorization. Miners still choose their offers. An active attempt delays
authorization expansion, while read-only recovery definitions remain available.
`{"managed":false}` disables future expansion and leaves existing authorizations
and offers unchanged. Managed catalogs are refused under fair-turn routing,
where adding authorization could otherwise assign work without an offer.

Deploy the coordinator support first, then distribute this compatibility worker
through the normal signed update process. Older workers ignore the new protocol
advertisement. Workers advertising cached extension IDs require catalog protocol
support; if a coordinator rollback lacks it, the worker advertises its compiled
baseline only. Unsupported definitions stay blocked pending an adapter update.

This change is prepared for review. It does not release or deploy a worker,
change a production coordinator, enroll existing miners, or call a paid provider.
