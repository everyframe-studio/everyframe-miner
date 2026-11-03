# Everycli v0.9.5 — BFL regional polling and generation recovery

BFL workers follow the regional polling location returned by the provider and
support its current image-delivery hosts. FLUX 3's **Reasoning** and **Generating**
states are treated as work in progress, and terminal failures remain distinct
from successful output.

Accepted task IDs are retained for recovery. Unusable response URLs pause BFL
locally, and uncertain submissions are not automatically replayed.

Run `everycli update`, then `everycli worker-update`, or choose **Update worker**
in the dashboard. The existing VM, keys, offers and discounts are preserved.
Withdrawn offers stay withdrawn; previously failed jobs are not replayed.
