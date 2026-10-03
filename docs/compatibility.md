# deltin-rs Compatibility Verification

This document defines how `deltin-rs` verifies **current implementation support**. It does not define the DEL/OSTW core-language scope.

The current language contract is [`docs/architecture/language-core.md`](architecture/language-core.md): for the declared core-language surface, established upstream DeltinScript/OSTW behavior is the executable specification and is presumptively in scope unless explicitly excluded as editor/integration behavior or demonstrated non-contractual implementation detail.

`support-matrix.toml`, corpus fixtures, and real projects measure how much of that language is currently implemented and verified. A missing or `planned` matrix entry is an implementation gap, not evidence that an established core feature is outside the language.

## Compatibility target

Source→Workshop compilation converges structurally on the pinned upstream OSTW
output as defined in [`language-core.md`](architecture/language-core.md#compatibility-target):
both outputs are parsed by `workshop-rs` and compared as canonical programs, and
any unrecorded structural difference is a defect. The other surfaces target
observable semantic compatibility. Relevant checks include:

- accepted/rejected source and project behavior;
- structured diagnostics;
- source/project semantic queries;
- high-level runtime meaning such as dispatch, storage, references, closures, recursion, and lifetime behavior;
- structural comparison of source→Workshop lowering against the pinned upstream output where supported;
- declared Workshop→DEL/OSTW reconstruction behavior.

It does not require upstream compiler architecture, internal IR identity, formatting, or byte-identical Workshop text. Generated helper names, variable indices, and optimizer effects on emitted structure are part of the compared compilation structure.

## Verification surfaces

Compatibility verification is ordinary tests and test data (ADR-0005):

| Surface | What it asserts |
| --- | --- |
| [`tests/corpus.rs`](../tests/corpus.rs) | Each fixture under `tests/corpus/` satisfies its declared `// expect:` outcome through parse → project → semantic → HIR. |
| [`tests/real_projects.rs`](../tests/real_projects.rs) | Vendored third-party projects under `tests/real-projects/` run the full pipeline end-to-end: import-closure size is pinned and diagnostics stay within recorded bounds. |
| [`tests/reconstruction.rs`](../tests/reconstruction.rs) | The declared Workshop→OSTW boundary in `tests/reconstruction-fixtures/support-boundary.json`: positive fixtures reconstruct to OSTW the native parser accepts; reject fixtures fail with structured `reconstruct-*` codes. |
| Focused tests | Parser/semantic/HIR/project/Workshop-lowering unit and integration tests under `tests/` and `src/`. |
| [`support-matrix.toml`](support-matrix.toml) | Declared support state, validated mechanically by `tests/matrix.rs` and `deltin-rs support --check`. |

## Fixture conventions

- `// expect: ok | parse-error | semantic-error | hir-error` is required on
  every fixture under `tests/corpus/` (recursively, except `projects/`). A
  fixture that cannot be assigned a concrete expectation is a gap to resolve,
  not a status to record.
- `// source:` and `// license:` remain only on fixtures derived from
  third-party material under the pinned upstream revision (see
  [`provenance.md`](provenance.md)). WrightKit-authored fixtures need no
  provenance headers.
- `// note:` is free-form context and is ignored by the harness.
- `tests/corpus/projects/` fixtures are exercised by dedicated project tests
  rather than the generic walker.

## Support-matrix states

`support-matrix.toml` is the machine-readable record of **declared support
state** for tracked capabilities. It is validated by tests and `deltin-rs
support --check`.

| State | Meaning |
| --- | --- |
| `planned` | Known/tracked behavior is not yet claimed as implemented at this layer. |
| `source-supported` | Parsing/source representation is implemented; no stronger semantic claim is implied. |
| `semantic-supported` | Semantic/type/HIR behavior is implemented without requiring complete Workshop emission. |
| `lowering-dependent` | Source semantics exist but end-to-end support depends on DEL-owned lowering into canonical Workshop. |
| `end-to-end-supported` | The tracked capability is implemented through the declared end-to-end path. |
| `out-of-scope` | The tracked item is intentionally outside this repository's product/language implementation boundary, such as editor-only functionality. This state must not be used to exclude established core-language behavior merely because it is unimplemented. |

The matrix is not an architecture specification, feature authorization list, or substitute for upstream core behavior. Declared states are verified by the test surfaces above, not by per-entry citation lists.

## Methodology

- Do not promote an expectation because the implementation under test agrees
  with itself. An expectation grounded in the pinned upstream revision, a real
  project, or a documented semantic contract is independent; an unexpected
  divergence from an independent expectation is a compatibility failure until
  explained or the owning contract is deliberately changed.
- Minimized regression fixtures preserve a distinct failure mode without
  replacing the full-project coverage that exposed it.
- Differential comparison against a pinned upstream build remains the defined
  gap-discovery methodology; it requires the pinned build
  ([`provenance.md`](provenance.md)) and is a maintainer workflow, not a CI
  merge gate. Its conclusions land as ordinary tests and fixtures, not as
  committed observation records.

## Workshop-independent vs end-to-end support

Parsing, project loading, semantic/type analysis, HIR, diagnostics, and inspect/query may be supported independently of full Workshop lowering.

DEL/OSTW-specific runtime/compiler semantics remain owned by `deltin-rs`; canonical Workshop WIR/catalog/validation/emission remain owned by `workshop-rs`. See [`docs/architecture/workshop-boundary.md`](architecture/workshop-boundary.md).

A canonical Workshop gap is fixed in `workshop-rs` only when it is genuinely a Workshop concept. A DEL-specific runtime/lowering gap stays here.

## Provenance

Pinned upstream identity, licensing guardrails, and re-pinning procedure remain in [`provenance.md`](provenance.md). Syntax observations in [`syntax-notes.md`](syntax-notes.md) and inventory records may aid investigation, but neither overrides the current architecture contract or the executable test surfaces.
