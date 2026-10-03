# deltin-rs Documentation

This directory is the durable documentation surface for `deltin-rs`. The root
[`README.md`](../README.md) is the user-facing overview.

## Documentation model

```text
architecture/README.md       current architecture routing
  ├─ language-core.md        current DEL/OSTW language/semantic contract
  └─ workshop-boundary.md    current runtime/lowering ownership boundary
adr/                         point-in-time architecture decision history
support-matrix.toml          current declared support state
compatibility.md             compatibility verification
provenance.md                pinned reference/licensing provenance
source/tests/corpus          current implementation reality
architecture.md/decisions.md historical compatibility pointers
Issues / PRs / releases      mutable execution state
```

Current architecture is not reconstructed from old implementation plans. For
substantive work, start from [`architecture/README.md`](architecture/README.md),
then inspect current source/tests/support state and the Issue contract.

## Current architecture

- [Architecture routing](architecture/README.md)
- [DEL/OSTW language core](architecture/language-core.md): upstream core as the
  executable specification, project/semantic ownership, typed implementation,
  and feature locality.
- [DEL/OSTW / Workshop boundary](architecture/workshop-boundary.md): runtime and
  lowering ownership, typed HIR intent, and canonical Workshop boundary.
- [`adr/`](adr/README.md) — normalized rationale for durable architecture
  decisions, with explicit historical, external, and unresolved classifications.
- [Repository agent guidance](../AGENTS.md): implementation preflight,
  provenance, validation, and delivery.

[`architecture.md`](architecture.md), [`decisions.md`](decisions.md), and
[`implementation-role.md`](implementation-role.md) are retained for old links;
they are not parallel current architecture authorities.

## Compatibility and support reality

- [`support-matrix.toml`](support-matrix.toml) — machine-readable declared
  support state. It measures implementation completeness; it does not define
  the upstream core-language scope.
- [`compatibility.md`](compatibility.md) — compatibility verification surfaces
  and support-state meanings.
- [`inventory.md`](inventory.md) — feature inventory with upstream source
  citations; not a feature authorization list.
- [`syntax-notes.md`](syntax-notes.md) — lexical/grammar observations from the
  pinned reference.
- [`limitations.md`](limitations.md) — current supported/unsupported boundaries,
  subject to executable verification and current architecture.
- [`provenance.md`](provenance.md) — pinned upstream identity, licensing
  guardrails, and re-pinning procedure.
- [`workshop-conformance.md`](workshop-conformance.md) — integration with
  canonical `workshop-rs` feature identities.

## Interfaces

- [`cli.md`](cli.md) — CLI task surfaces, exits, presentation, and completion.
- Public library behavior is established by source/API tests and the current
  architecture contracts rather than a frozen module-layout document.

## Historical design records

The former ~95 KB `architecture.md` and Q1–Q16 `decisions.md` encoded a
point-in-time implementation baseline, including crate layout, dependency
versions, milestone states, API sketches, and issue-specific decisions. Those
details remain in Git history. Stable invariants that remain binding must be
represented in the current architecture contracts instead of requiring an
Engineer to infer them from historical prose.

## Development and testing

Run the repository validation gates from `AGENTS.md`. Real-project support
claims require the `tests/real-projects/` runs in addition to focused tests.
Matrix/test counts are not proof of semantic completeness.
