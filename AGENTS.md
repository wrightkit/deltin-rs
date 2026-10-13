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
- standalone CLI/library tooling and compatibility verification;
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
not be forced through complete compiler lowering without a documented
reason.

Source→Workshop compilation converges structurally on the pinned upstream OSTW
compiler output, compared as `workshop-rs`-parsed canonical programs; formatting
is not a criterion. Structural differences, including behaviorally equivalent or
cheaper rewrites, are defects unless recorded as an approved exception (see
[`language-core.md`](docs/architecture/language-core.md#compatibility-target)).
Other surfaces (diagnostics, queries, reconstruction) target observable DEL/OSTW
semantics, not upstream internal architecture.

## Architecture routing

For substantive implementation work, resolve the relevant current contract from
[`docs/architecture/README.md`](docs/architecture/README.md) before editing.
The old `docs/architecture.md` and `docs/decisions.md` are historical/compatibility
entry points and do not override current contracts or code reality.

Use:

- [`language-core.md`](docs/architecture/language-core.md) for DEL/OSTW scope,
  project/semantic ownership, typed behavior, and feature locality;
- [`workshop-boundary.md`](docs/architecture/workshop-boundary.md) for runtime and
  compiler-lowering ownership at the canonical Workshop boundary.

If the Issue, current architecture contract, declared support state, and
source/tests disagree materially, stop and surface the mismatch rather than deciding the
architecture by implementation convenience.

## Upstream and provenance

For the declared DEL/OSTW core-language surface, the established upstream
implementation is the executable specification. Core behavior is
presumptively in scope unless explicitly excluded as editor/integration
functionality or demonstrated to be a non-contractual implementation artifact.

Inspect upstream source/docs/tests to understand behavior, then implement the
behavior directly in clear Rust. Do not mechanically translate or copy
unlicensed upstream compiler internals. Fixture attribution must follow
[`docs/provenance.md`](docs/provenance.md).

The support matrix, inventory, corpus, and real projects verify current
completeness and compatibility. They do not decide whether an established core
feature belongs in scope.

## Semantic implementation

Prefer typed Rust for observable DEL/OSTW behavior and invariants: project/import
semantics, type/member/overload rules, dispatch, capture/reference behavior,
storage/lifetime semantics, runtime intent, and lowering decisions.

Machine-readable matrices/inventories may record capability identity,
provenance, reference links, and support state. Do not turn them into an
interpreted semantic specification.

## Development priority

Prioritize real project usability over architecture polish. When a real
DEL/OSTW project exposes a blocker:

1. reproduce it with standalone `deltin-rs` tooling;
2. fix DEL/OSTW-owned behavior here;
3. route genuine canonical Workshop gaps to `workshop-rs`;
4. retain the real-project coverage and add a minimized regression where
   practical;
5. prefer coherent implementation waves over unnecessary per-construct issue/PR
   fragmentation.

Internal module layout and helper abstractions are revisable. If the smallest
diff would deepen an already mixed responsibility, the smallest bounded
extraction needed to keep the changed feature cohesive is in scope; unrelated
cleanup remains out of scope.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run --quiet -p deltin-rs-cli -- support --check
```

Run compatibility/corpus gates affected by the change. A passing unit-test
count does not establish a real-project support claim; rerun the affected
project workflow (`tests/real-projects/`).

## Delivery

- Keep commits focused and avoid unrelated changes.
- Keep support-matrix and documentation claims synchronized with executable
  tests.
- Never commit credentials, private runtime data, or unreviewed third-party
  material.

## Documentation

Durable documentation follows the shared documentation policy; `docs/README.md` is this repository's index.
