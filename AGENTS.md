# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace. Apply
the workspace-level `AGENTS.md` first, then this repository's local ownership,
architecture, validation, and delivery rules.

`deltin-rs` is WrightKit's standalone Rust implementation of the DeltinScript /
OSTW language surface. It is not an internal Wright language repository. Wright
is a downstream tooling consumer that may integrate `deltin-rs` through native APIs
or LPP.

Parsing, project loading, semantic/type analysis, HIR, and compiler lowering
are capabilities owned by this repository. A **provider** is only an integration
role exposed through a protocol such as LPP; it does not replace the repository's
identity as an independent DEL/OSTW implementation.

## Ownership boundary

`deltin-rs` owns:

- DEL/OSTW syntax, parsing, project loading/imports, source model, and trivia;
- semantic/type resolution, diagnostics, provenance, and typed HIR;
- DEL/OSTW-specific runtime and compiler lowering semantics;
- standalone CLI/library tooling and compatibility evidence;
- Workshop → DEL/OSTW reconstruction when implemented.

`workshop-rs` owns:

- canonical raw Workshop semantics and identities;
- Workshop WIR, validation, settings/localization, parser, and emitter;
- Workshop-observable contracts shared across source-language implementations.

The durable dependency direction is:

```text
deltin-rs → workshop-rs
```

Do not copy canonical Workshop data, WIR, emitter, settings, or localization into
this repository. Missing canonical capabilities must be fixed in
`workshop-rs`, not approximated locally for convenience.

The standalone semantic path must remain useful independently of Workshop
emission. `check`, `inspect`, symbol/type queries, and project diagnostics must
not be forced through complete compiler lowering without an evidence-backed
reason.

Compatibility targets observable DEL/OSTW semantics, not upstream internal
architecture, generated helper identity, optimizer shape, formatting, or text
identity.

See [`docs/implementation-role.md`](docs/implementation-role.md) for the durable
repository/product relationship and [`docs/architecture.md`](docs/architecture.md)
for implementation details.

## Development priority

Prioritize real project usability over architecture polish. When a real
DEL/OSTW project exposes a blocker:

1. reproduce it with standalone `deltin-rs` tooling;
2. fix DEL/OSTW-owned behavior here;
3. route genuine canonical Workshop gaps to `workshop-rs`;
4. retain full-project evidence and add a minimized regression where practical;
5. prefer coherent implementation waves over unnecessary per-construct issue/PR
   fragmentation.

Internal module layout, helper abstractions, and concrete lowering organization
are revisable unless they affect a public/versioned contract, repository
ownership, source provenance, or observable compatibility.

## Upstream and provenance

Pinned OSTW/DeltinScript sources are compatibility references, not architecture
mandates. Unlicensed upstream compiler internals must not be copied or
mechanically translated. Fixtures and behavior evidence must follow the
repository's provenance and licensing documentation.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run --quiet -p deltin-rs-cli -- support --check
```

Run compatibility/corpus gates affected by the change. A passing unit-test count
is not sufficient evidence for a real-project support claim; rerun the affected
project workflow.

## Delivery

- Never push directly to `main`; use an independent branch and PR.
- Keep commits focused and avoid unrelated changes.
- Keep support-matrix and documentation claims synchronized with executable
  evidence.
- Never commit credentials, private runtime data, or unreviewed third-party
  material.
