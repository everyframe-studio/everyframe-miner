## Worker readiness

Fixes an incorrect stale-attestation error when a worker still has a valid admitted session and a fresh heartbeat. The CLI and coordinator now use the same session validity rules. Expired sessions, offline workers and post-restart attestation checks remain enforced.

Run `everycli update`. No worker update is required for this fix.
