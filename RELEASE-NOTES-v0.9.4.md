# Everycli v0.9.4 — BFL polling and result recovery

BFL workers now follow the regional polling location returned by the provider
and accept its current image-delivery hosts. Existing BFL task references remain
compatible across the update.

Accepted provider task IDs are preserved when a response needs operator review.
An unusable BFL response pauses that provider locally, while other providers
remain available. This prevents repeated paid submissions while the issue is
being investigated.

Update with `everycli update`, reopen `everycli ui`, and choose **Update worker**,
or run `everycli worker-update`. The update keeps the existing VM, provider keys,
offers and discounts. Review BFL offers after updating; withdrawn offers remain
withdrawn. Previously failed jobs are not automatically replayed.
