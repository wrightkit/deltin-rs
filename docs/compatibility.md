# deltin-rs Compatibility Evidence Contract

This document defines how `deltin-rs` records and evaluates **current implementation support**. It does not define the DEL/OSTW core-language scope.

The current language contract is [`docs/architecture/language-core.md`](architecture/language-core.md): for the declared core-language surface, established upstream DeltinScript/OSTW behavior is the executable specification and is presumptively in scope unless explicitly excluded as editor/integration behavior or demonstrated non-contractual implementation detail.

`support-matrix.toml`, corpus fixtures, reference probes, and real projects measure how much of that language is currently implemented and evidenced. A missing or `planned` matrix entry is an implementation gap, not evidence that an established core feature is outside the language.

## Compatibility target

Source→Workshop compilation converges structurally on the pinned upstream OSTW
output as defined in [`language-core.md`](architecture/language-core.md#compatibility-target):
both outputs are parsed by `workshop-rs` and compared as canonical programs, and
any unrecorded structural difference is a defect. The other surfaces target
observable semantic compatibility. Relevant evidence includes:

- accepted/rejected source and project behavior;
- structured diagnostics and provenance;
- source/project semantic queries;
- high-level runtime meaning such as dispatch, storage, references, closures, recursion, and lifetime behavior;
- structural comparison of source→Workshop lowering against the pinned upstream output where supported;
- declared Workshop→DEL/OSTW reconstruction behavior.

It does not require upstream compiler architecture, internal IR identity, formatting, or byte-identical Workshop text. Generated helper names, variable indices, and optimizer effects on emitted structure are part of the compared compilation structure.

## Pinned OSTW evidence package

The owner-side pinned-reference package is [`compatibility/ostw/`](../compatibility/ostw/).
Its machine-readable records are the canonical evidence surfaces for the external
OSTW reference:

- `reference.json` identifies the immutable v3.4.0 release asset;
- `corpus.json` identifies the licensed corpus files, hashes, project roots, and provenance;
- `results.json` records the pinned accept/reject and diagnostic observations;
- `probes/` records focused semantic observations and their emitted-output hashes;
- `reconstruction/` records the declared Workshop-to-OSTW boundary.

`python3 compatibility/ostw/run_oracle.py --check` validates the committed package
without the upstream binary. Reference acquisition and observation refresh are
explicit maintainer operations; they are not native CI merge gates.

## Support-matrix states

`support-matrix.toml` is the machine-readable record of **current evidenced support state** for tracked capabilities. It is validated by tests and `deltin-rs support --check`.

| State | Meaning |
| --- | --- |
| `planned` | Known/tracked behavior is not yet claimed as implemented at this layer. |
| `source-supported` | Parsing/source representation is evidenced; no stronger semantic claim is implied. |
| `semantic-supported` | Semantic/type/HIR behavior is evidenced without requiring complete Workshop emission. |
| `lowering-dependent` | Source semantics exist but end-to-end support depends on DEL-owned lowering into canonical Workshop. |
| `end-to-end-supported` | The tracked capability is evidenced through the declared end-to-end path. |
| `out-of-scope` | The tracked item is intentionally outside this repository's product/language implementation boundary, such as editor-only functionality. This state must not be used to exclude established core-language behavior merely because it is unimplemented. |

The matrix is not an architecture specification, feature authorization list, or substitute for upstream core behavior.

## Evidence methodology

Compatibility evidence should remain attributable and independently useful:

- **Corpus fixtures** retain source/license/expectation provenance and structurally valid source context.
- **Pinned reference probes** compare against the recorded upstream implementation identity when reproducible.
- **Real projects** retain immutable repository/revision/path provenance and remain stronger product evidence than isolated synthetic examples.
- **Minimized regressions** preserve a distinct failure mode when useful without replacing the full-project evidence that exposed it.
- **Matrix validation** checks schema/state/evidence integrity; it does not prove the implementation is correct.

Do not promote an expectation because the implementation under test agrees with itself. Unexpected divergence from independent evidence is a compatibility failure until explained or the owning contract is deliberately changed.

## Workshop-independent vs end-to-end support

Parsing, project loading, semantic/type analysis, HIR, diagnostics, and inspect/query may be supported independently of full Workshop lowering.

DEL/OSTW-specific runtime/compiler semantics remain owned by `deltin-rs`; canonical Workshop WIR/catalog/validation/emission remain owned by `workshop-rs`. See [`docs/architecture/workshop-boundary.md`](architecture/workshop-boundary.md).

A canonical Workshop gap is fixed in `workshop-rs` only when it is genuinely a Workshop concept. A DEL-specific runtime/lowering gap stays here.

## Provenance

Pinned upstream identity, licensing guardrails, and re-pinning procedure remain in [`provenance.md`](provenance.md). Syntax observations in [`syntax-notes.md`](syntax-notes.md) and inventory records may aid investigation, but neither overrides the current architecture contract or executable evidence.
