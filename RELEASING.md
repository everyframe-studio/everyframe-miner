# Release checklist

1. Include the Apache-2.0 `LICENSE` and applicable third-party notices in release
   distributions, and publish a verified private security contact.
2. Run formatting, strict Clippy, tests, and a release build with `--locked`.
   Audit Rust dependencies and preserve applicable dependency notices.
3. For CLI distribution, build for each supported OS/architecture, publish binary
   checksums, and sign artifacts through the owner's existing release process.
4. For worker distribution, review `config/release.json`, coordinator/chain pins,
   the registry, and source changes. The public checkout stays `UNCONFIGURED`.
5. Build using reviewed **digest-pinned** Rust and runtime images with compatible
   libc versions. No credentials, invitation, wallet, or live profile may enter
   the build context. The `.dockerignore` uses an explicit allowlist.

```sh
docker build --target worker \
  --build-arg RUST_IMAGE='rust@sha256:REVIEWED_BUILDER_DIGEST' \
  --build-arg RUNTIME_IMAGE='debian@sha256:REVIEWED_RUNTIME_DIGEST' \
  -t everyframe-miner-rust:reviewed .
```

Use `--target cli` for a standalone CLI container; the default target is `worker`.
Final images contain only their
native executable and the chosen base runtime—no Node.js or Python is added.

6. Push only when explicitly approved; record the immutable image digest.
7. Request a new signed, digest-pinned compose invitation. The coordinator must
   review the new exact app/OS/KMS measurements. Never whitelist all images or
   bypass admission to make the port start.
8. Test in a separately approved canary with a stated total budget. Verify real
   dstack quotes, admission, activation, job completion, provider accounting,
   recovery, and shutdown before production promotion.

The Rust source does not automatically replace the current Node worker or Python
CLI. An existing signed invitation for a Node image still deploys that Node image;
using the Rust CLI alone does not change the worker selected by the invitation.
