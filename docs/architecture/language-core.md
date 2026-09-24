# DEL / OSTW Language Core Contract

`deltin-rs` is an independently usable Rust implementation of the DeltinScript / OSTW language. This contract defines semantic ownership and implementation direction; it does not claim every core feature is already implemented.

## Upstream core is the executable specification

For the declared DEL/OSTW core-language surface, the established DeltinScript / OSTW implementation is the executable specification. Core behavior is presumptively in scope unless explicitly excluded as editor/integration functionality or demonstrated to be a non-contractual implementation artifact.

The compatibility inventory, support matrix, corpus, reference probes, and real projects verify implementation completeness and observable compatibility. They do not decide feature-by-feature whether established core language behavior belongs in `deltin-rs`.

Upstream architecture is not a mandate. Understand the source-language behavior and implement it directly in clear Rust rather than mechanically translating upstream internals.

## Semantic ownership

`deltin-rs` owns:

- lexical/source syntax and trivia needed for diagnostics/tooling;
- project loading and import/module semantics;
- name/type/member/overload/access resolution;
- classes, structs, enums, inheritance, virtual dispatch, generics, lambdas, references, recursion, storage and other DEL/OSTW runtime semantics;
- diagnostics and source provenance;
- typed DEL HIR and source-aware tooling semantics;
- DEL/OSTW-specific runtime/compiler lowering;
- Workshop→DEL/OSTW reconstruction.

The Workshop-independent semantic path must remain useful without requiring complete backend lowering.

## Typed behavior, not an inventory language

Use typed Rust to express source-language behavior and invariants, including type/assignability rules, dispatch, capture/reference semantics, runtime lifetime/storage meaning, overload and argument binding, project resolution, and lowering decisions.

Machine-readable inventory/support data is appropriate for capability identity, evidence links, support state, provenance, and other declarative facts. It is not the semantic specification and must not become an interpreted language that defines source behavior.

A corpus entry or support-matrix row proves evidence/support status; it does not authorize a semantic design or narrow the upstream core scope.

## Feature locality

Source-language behavior should have a discoverable domain home. Parser, semantic checker, HIR lowerer, Workshop lowerer, and generic registries are phase infrastructure, not automatic homes for every future feature.

If implementing a feature would deepen an already mixed responsibility, the smallest bounded extraction needed to keep the changed behavior cohesive is within scope. Unrelated cleanup and speculative abstraction remain out of scope.

The semantic checker keeps one explicit `Checker` state object while locating its current behavior by domain: `semantic/check/resolution.rs` owns type, name, member, and overload resolution; `semantic/check/expressions.rs` owns expression typing and lvalue rules; `semantic/check/statements.rs` owns statement and local-declaration checking; and `semantic/check/rules.rs` owns rule/body traversal. `semantic/check.rs` remains the shared-state and phase-orchestration facade, while `semantic/resolve.rs` defines resolution result types.

## Compatibility target

For source→Workshop compilation, the pinned upstream OSTW compiler output is the correctness target, as defined by WrightKit goal principle 7. `deltin-rs` output must match its canonical Workshop structure: rule order, element identities, control flow, condition shape, value construction, variable names and indices (including generated helpers), and element cost. Compatibility is measured by parsing both outputs with `workshop-rs` and comparing the canonical programs structurally; text diffs, line counts, and text-pattern counts are not evidence, and formatting, whitespace, and comments are not criteria.

Structural rewrites are not accepted, even when they appear behaviorally equivalent or reduce element cost. Any structural difference is a defect unless it is a recorded exception approved by the owner, including a difference for an apparent upstream bug. Each exception records the upstream behavior, the `deltin-rs` behavior, the approving decision, and the test that pins it.

For the other surfaces (accepted/rejected programs, project behavior, diagnostics/provenance, source tooling behavior, and declared reconstruction contracts), target observable semantic compatibility. Upstream internal IR and compiler architecture remain non-contractual.

`deltin-rs` does not invent a WrightKit-only DEL/OSTW dialect.
