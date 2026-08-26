# deltin-rs Compatibility Contract

Status: **living reference** · Owner: Architecture. This document is the
human-readable compatibility contract for the `deltin-rs` parsing and semantic pipeline. The
machine-readable declared surface is
[`support-matrix.toml`](support-matrix.toml); every claim here is derived from
that matrix and from the corpus evidence it references.

## What compatibility means

Compatibility with OSTW/DeltinScript is **observable semantic compatibility
for the declared support surface**. It is not:

- upstream compiler architecture or internal representation identity;
- output-text identity (generated Workshop text, formatting, optimizer
  choices, or internal naming are not correctness criteria);
- a promise to reproduce upstream bugs or internals beyond observable
  behavior.

The pipeline contract is:

```text
DEL/OSTW source -> source model -> DEL semantic model -> typed DEL HIR
-> [integration boundary] -> workshop-rs
```

Compatibility is established and measured per capability, from the pinned
upstream oracle and the corpus, not as a single aggregate score.

## `.del` and `.ostw` as accepted source forms

OSTW and DeltinScript are the same language: DeltinScript is the language
implemented by the OSTW compiler (there is no separate reference
implementation — see [`provenance.md`](provenance.md)). The parser accepts
`.del`, `.ostw`, and `.workshop` source files interchangeably, and no
semantic distinction between the extensions is asserted unless corpus
evidence establishes one. The only dialect-like distinction in the corpus is
**OSTW syntax vs. the vanilla Overwatch Workshop superset syntax** (vanilla
rules and workshop-context blocks are parsed as opaque spans; see
[`syntax-notes.md`](syntax-notes.md) and [`limitations.md`](limitations.md)).

## Support-matrix states

Every tracked capability in [`support-matrix.toml`](support-matrix.toml) has
exactly one state, defined as follows:

| State | Meaning |
| --- | --- |
| `planned` | Inventoried and evidenced upstream, but not yet implemented; not claimed as supported. |
| `source-supported` | Lexed/parsed into documented AST structures with stable spans; no semantic claims. |
| `semantic-supported` | Resolved, type-checked, and diagnosed by the semantic model / HIR; no Workshop emission required. |
| `lowering-dependent` | Requires concrete Workshop encoding owned by deltin-rs #30; the canonical WIR, catalog, and emission contracts remain owned by `workshop-rs`, while typed HIR carries intent only. |
| `end-to-end-supported` | Fully supported through Workshop emission; currently unused (no end-to-end path exists in this crate). |
| `out-of-scope` | Deliberately outside the `deltin-rs` language contract (e.g. editor behavior). |

Categories: `syntax`, `semantic`, `runtime-semantics`, `workshop-lowering`,
`compiler-utility`, `decompiler`, `editor`, `project`.

Because states include `lowering-dependent` and `out-of-scope` capabilities,
**no single aggregate percentage is reported** anywhere in the documentation;
a percentage would misrepresent the declared support boundary. The matrix and
its per-entry evidence are the source of truth.

## Workshop-independent parsing and semantic analysis vs. end-to-end compilation

`deltin-rs` supports Workshop-independent parsing and semantic analysis:

- the full syntax, semantic, and runtime-semantics surface parses, resolves,
  type-checks, and lowers to typed HIR with provenance;
- a bounded semantic oracle executes high-level behavior (allocation/deletion,
  virtual dispatch, recursion, lambdas, arrays, switch fallthrough) so corpus
  cases can distinguish correct from incorrect behavior before any backend
  exists;
- Workshop-facing names bind through the `WorkshopProvider` trait; the
  `NoopProvider` treats them as unresolved-but-legal with structural checks
  only. No canonical Workshop catalog data lives in this crate.

End-to-end Workshop compilation (`DEL/OSTW -> Workshop text`) is
**`lowering-dependent`**: the concrete encoding (variable slots, helper
rules, dispatch tables, recursion stacks, reference layouts, emitter) is
deltin-rs #30 work. It consumes typed HIR across the documented boundary; the
canonical WIR, catalog, and emission contracts remain owned by `workshop-rs`.
Decompilation (`Workshop -> DEL/OSTW`) is `planned` (issue #9).

## Corpus and differential-testing methodology

Compatibility claims are grounded in the corpus under `tests/corpus/`, the
feature inventory ([`inventory.md`](inventory.md)), and — where reproducible —
the pinned upstream compiler.

- **Fixture headers.** Every `.del`/`.ostw` corpus fixture carries
  `// source: <url@commit>`, `// license: <license>`, and `// expect: <outcome>`
  directives. The corpus harness (`tests/corpus.rs`, run on every CI run)
  fails on missing or empty source/license directives and asserts each
  fixture's declared outcome.
- **Accept/reject agreement.** The primary compatibility record is
  accept/reject and diagnostic-presence agreement per fixture, expressed as
  `// expect:` outcomes — never output-text identity.
- **Differential comparison.** Comparing deltin-rs against a pinned upstream
  build is the defined gap-discovery methodology for matrix entries; it is
  gated on the availability of a pinned upstream build and is not a CI merge
  gate. Divergences are tracked against the matrix entries they affect.
- **Matrix validation.** `tests/matrix.rs` (CI gate) and the
  `deltin-rs support --check` command validate `support-matrix.toml`: schema,
  unique ids, fixed category/state sets, existing evidence paths, and a
  rationale note on every `lowering-dependent`/`out-of-scope` entry.
- **Evidence report.** `tests/corpus.rs` and
  `deltin-rs maintainer compatibility --json`
  classify each source case by independent evidence and separate matched
  behavior, known gaps, unsupported cases, unexpected regressions, and
  inconclusive evidence. Unknown expectations require an explicit non-passing
  status; they are never promoted to compatibility by native agreement.

`pinned-oracle` fixtures must use the pinned OSTW compiler repository and
commit. `real-project` fixtures must use their own immutable repository,
revision, path, and license provenance; they cannot reuse the pinned upstream
compiler identity. Compiler-shipped Examples and Modules remain project-level
`pinned-oracle` fixtures until an independent real project is added.

## Pinned upstream oracle and provenance boundary

Compatibility is defined against a single pinned upstream reference,
recorded in [`provenance.md`](provenance.md):

- `ItsDeltin/Overwatch-Script-To-Workshop` (repo + wiki), pinned at a
  specific commit, shallow-cloned under `.upstream-refs/` (git-ignored);
- upstream fixtures are imported under the MIT license with attribution
  headers; re-pinning is a deliberate compatibility-contract decision owned
  by the architect/PM;
- the `docs/` analysis is original deltin-rs work quoting small upstream
  examples for evidence.

## Related documents

- [`support-matrix.toml`](support-matrix.toml) — machine-readable declared surface (source of truth).
- [`inventory.md`](inventory.md) — feature inventory with per-entry upstream evidence.
- [`limitations.md`](limitations.md) — current support boundary and gap classification.
- [`provenance.md`](provenance.md) — pinned oracle identity and licensing rules.
- [`architecture.md`](architecture.md) — implemented architecture and the provider/HIR seams.
- [`workshop-conformance.md`](workshop-conformance.md) — machine-readable
  report and the canonical Workshop integration boundary.
- [`syntax-notes.md`](syntax-notes.md) — lexical/grammar reference from the pinned upstream.
