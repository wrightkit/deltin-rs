# del-rs limitations and support boundary

Status: **evergreen** · Owner: Architecture. This document states the current
support boundary of the `del-rs` language implementation: what is deliberately not
implemented, and why. The authoritative declared surface is
[`support-matrix.toml`](support-matrix.toml); state meanings are defined in
[`compatibility.md`](compatibility.md). Capabilities are classified as
**lowering-dependent** (concrete Workshop encoding is del-rs #30 work; the
canonical WIR/catalog contract remains owned by `workshop-rs`) or
**intentionally unsupported** (editor-only or outside the language contract),
plus a short list of evidence-backed approximation areas.

## Lowering-dependent (del-rs #30 / workshop-rs canonical contract boundary)

- Concrete Workshop emission (del-rs #30): actions, values, events, variable slots,
  helper rules, dispatch tables, recursion stacks, reference layouts,
  optimizer choices. The typed HIR expresses intent only
  (`architecture.md` §15).
- Canonical Workshop catalog data (actions/values/events/constants):
  `del-rs` never vendors it; `CatalogProvider` reads the released
  `workshop-rs` catalog through the documented `WorkshopProvider` seam.
  `NoopProvider` remains available for Workshop-independent workflows.
- Vanilla Workshop superset bodies (`rule("...")`, `variables {}`,
  `subroutines {}`, `settings {}`, hooks): parsed as opaque token spans with
  no DEL/OSTW semantic interpretation.
- Lobby-settings / custom-game-settings imports (`.json`, `.lobby`):
  recorded with provenance, not interpreted.
- `ds.toml` keys other than `entry_point`: validated syntactically, never
  interpreted.
- Decompiler, optimizer, emulator, pathfinding tooling: inventory entries,
  not implemented (matrix `compiler-utility` / `decompiler`).
- `foreach` is a DEL-owned lowering/runtime strategy. The bounded #31 slice
  lowers global-rule iteration over array collections with scalar value binders
  by materializing the collection and index into generated global helper slots
  and using canonical `countOf`, `valueInArray`, `while`, and global-variable
  actions. Player-context iteration, non-scalar/reference binders, and
  re-entrant bodies still fail closed with `HI018`; this is not a claim that
  canonical `workshop-rs` WIR is missing a provider-local `Foreach` node.
- Global-rule local storage now also accepts lowerable array values: the local
  is materialized as a generated global slot and uses canonical WIR `Array`,
  global-variable, and array-index values. Player-context locals, arrays with
  object/reference elements, and re-entrant storage remain `HI018` gaps.
- Switch lowering materializes an unstable scrutinee into one generated global
  helper slot before emitting repeated case comparisons in global rules.
  Player-context dynamic switches and recursive contexts fail closed with
  `HI018`; the released WIR contract does not prove a safe player-scoped temp
  or runtime stack for this slice.
- Scalar value locals in one `Event.OngoingGlobal` rule body have a bounded
  lowering-only implementation using deterministic synthetic global slots.
  The adapter rejects player context, uninitialized or non-scalar locals,
  external Workshop actions, internal calls, recursive/re-entrant storage,
  parameters, member storage, and return-value ABI with structured
  `HI018`; a shared global slot is not a general local or invocation-frame ABI.
  The adapter does not change HIR or canonical WIR.

## Intentionally unsupported

- VS Code / language-server behaviors (completions, semantic tokens,
  codelens, incremental parse, snippets, debugger, element-count UI) —
  matrix `editor` category, `out-of-scope`.
- `abstract` keyword: not in the pinned upstream surface.
- `interface` semantics: no keyword exists upstream; `class B : A, X` extra
  types are parsed and inert.
- Union types (`T | U`): parsed and recorded; assignability/member semantics
  are not enforced (ratified decision Q11).
- `Players` type: reserved in the type list, unexercised by the corpus
  (ratified decision Q9).
- JSON import expressions (`import("file.json")`): parse-only; semantics
  planned (ratified decision Q5).
- Pattern-binding mutation through non-lvalue operands is rejected
  (SM017/SM048) per corpus; binding *alias* semantics are represented in HIR
  but not executed by the oracle beyond value semantics.

## Known approximation areas (evidence-backed)

- Unknown-type rejection: upstream rejects undeclared type names; del-rs
  treats them as external by the provider contract. Two corpus fixtures were
  reclassified `unknown` with rationale (`struct-ref-inline-*`).
- Struct literal `{0}` single-value form: modeled as a single-value struct
  literal per corpus evidence; upstream mechanics differ internally.
- `define` inference, array-member builtin set, and operator tables are
  corpus-driven; the matrix tracks each entry with evidence paths.
- Auto-for classification follows upstream `Loops.cs` (step is an expression
  statement).
- Lambda captures: by-value snapshot semantics implemented; by-reference is
  a documented model extension, not exercised (ratified decision Q1).

## Verification methodology

- The corpus harness (`tests/corpus.rs`, part of CI) walks the fixtures under
  `tests/corpus/`, asserts each fixture's `// expect:` outcome, and fails on
  missing `// source:`/`// license:` provenance headers. This is the standing
  accept/reject record.
- `tests/matrix.rs` and `del-rs support --check` mechanically validate
  `support-matrix.toml` (schema, ids, states, evidence paths, rationale
  notes) on every CI run.
- Differential comparison against a pinned upstream build is the defined
  gap-discovery methodology (see `compatibility.md`); it requires a pinned
  upstream build (`provenance.md`) and is not a CI merge gate.
