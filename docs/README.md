# deltin-rs Documentation

This directory is the durable documentation surface for `deltin-rs`. The root
[`README.md`](../README.md) is the user-facing overview. `deltin-rs` is a
standalone DEL/OSTW implementation. Documents describe parsing, project loading,
semantic analysis, HIR, tooling, and lowering directly; these capabilities are
not a separate product identity.

Implementation sequencing and acceptance criteria live in GitHub issues and
pull requests. Durable ownership, architecture, interfaces, compatibility, and
provenance live here.

## Documentation model

```text
implementation-role.md       repository identity and Wright/workshop-rs relationship
  └─ architecture.md         internal source/project/semantic/HIR architecture
      └─ compatibility.md    compatibility contract and support-state meanings
          └─ support-matrix.toml + evidence
```

## Index

### Ownership and architecture

- [`implementation-role.md`](implementation-role.md) — standalone DEL/OSTW
  implementation identity; provider meaning; dependency on
  `workshop-rs`; Wright as downstream tooling/integration consumer.
- [`architecture.md`](architecture.md) — detailed implemented architecture of
  the internal Workshop-independent parsing/semantic pipeline and the DEL-owned integration /
  lowering seams: module layout, source model, parser/project/semantic/HIR,
  oracle, public API, CLI contract, and test strategy.
- [`cli.md`](cli.md) — task-oriented command classification, migration aliases,
  exit codes, presentation policy, GitHub annotations, and static completion.

### Compatibility

- [`compatibility.md`](compatibility.md) — observable-semantic compatibility
  contract, accepted source forms, support-state meanings, and the distinction
  between Workshop-independent semantic support and end-to-end Workshop
  support.
- [`support-matrix.toml`](support-matrix.toml) — machine-readable declared
  support surface, validated by tests and `deltin-rs support --check`. This is the
  source of truth for current feature states.
- [`inventory.md`](inventory.md) — declared language/compiler surface with
  per-feature evidence.
- [`syntax-notes.md`](syntax-notes.md) — lexical/grammar observations from the
  pinned reference.
- [`limitations.md`](limitations.md) — evergreen supported/unsupported and
  lowering-dependent boundaries.
- [`provenance.md`](provenance.md) — pinned upstream identity, licensing
  guardrails, and re-pinning procedure.
- [`workshop-conformance.md`](workshop-conformance.md) — evidence/report
  integration with canonical `workshop-rs` feature identities.

### Interfaces and decisions

- Library and CLI surfaces are described by [`architecture.md`](architecture.md)
  and [`cli.md`](cli.md), and exercised by the integration tests.
- [`decisions.md`](decisions.md) records ratified product/semantic decisions.
  Historical decisions do not redefine the repository as a Wright-owned
  provider or language implementation.

## Development and testing

The repository test suites cover parsing, semantics, advanced language
features, HIR/oracle behavior, corpus/project evidence, support-matrix
validation, CLI contracts, and Workshop integration. Run the repository's
current validation gates from `AGENTS.md`.

Real-project support claims require full-project evidence in addition to
focused tests. Fixed test counts are not a substitute for behavioral coverage.

## Authority

| Contract | Document | Normative scope |
| --- | --- | --- |
| Repository/product role | [`implementation-role.md`](implementation-role.md) | Standalone implementation identity and cross-repo ownership. |
| Internal architecture | [`architecture.md`](architecture.md) | Source/parsing/HIR/runtime/lowering organization. |
| Compatibility | [`compatibility.md`](compatibility.md) | State meanings, methodology, oracle boundary. |
| Declared surface | [`support-matrix.toml`](support-matrix.toml) | Per-capability current support states with evidence. |
| Product decisions | [`decisions.md`](decisions.md) | Ratified semantic/product decisions. |
| Provenance | [`provenance.md`](provenance.md) | Oracle pin, licensing, re-pinning. |
