# Release automation

`release-plz` maintains the release PR, publishes `deltin-rs` to crates.io, and
creates the canonical `vX.Y.Z` tag.

The release PR updates the package version and generated changelog. Merging
that PR into `main` runs `release-plz release`; pull-request heads do not
publish packages.

The protected GitHub Actions `release` environment must provide
`CARGO_REGISTRY_TOKEN`, able to publish `deltin-rs`. The repository Actions secrets
must provide `GH_TOKEN`, a fine-grained token with repository Contents and
pull-request read/write access for release PR and tag operations. Credentials
must never be committed.

The publication job uses a stable concurrency group with
`cancel-in-progress: false`. Normal CI remains the source-change quality gate;
release-specific checks should include `actionlint` and this package dry-run:

```sh
cargo package --locked
```

This check does not prove publication. Completion requires observing the package
version on crates.io and the matching tag after a real release. If publication
fails, correct the failed step and rerun the release path; do not republish an
already-published version under a different version.
