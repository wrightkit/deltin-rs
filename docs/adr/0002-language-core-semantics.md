# ADR-0002: DEL/OSTW core semantics and feature locality

- Status: Accepted — the verification/evidence-model framing is superseded
  by [ADR-0005](0005-tests-first-verification.md); the scope and locality
  decisions stand
- Date: 2026-09-12
- Related: [Issues #2](https://github.com/wrightkit/deltin-rs/issues/2), [#4](https://github.com/wrightkit/deltin-rs/issues/4), [#5](https://github.com/wrightkit/deltin-rs/issues/5), [#6](https://github.com/wrightkit/deltin-rs/issues/6), [#7](https://github.com/wrightkit/deltin-rs/issues/7), [#89](https://github.com/wrightkit/deltin-rs/issues/89), [`language-core.md`](../architecture/language-core.md)

## Context

The original architecture baseline and its Q1–Q16 decision log combined
language scope, implementation plans, support state, and historical milestone
details. The baseline was replaced by current contracts in #89. This record
normalizes the stable language decisions without turning the support matrix or
the old decision log into a semantic specification.

The rationale was recovered from the original architecture D1–D6 summary,
the ratified decision log, the #2–#7 issue contracts, and the current
`language-core.md` contract.

Historical evidence: the [pre-#89 architecture snapshot](https://github.com/wrightkit/deltin-rs/blob/0a9d695c241017cc6b070090b36bab30b09b99b0/docs/architecture.md),
the [Q1–Q16 decision log](https://github.com/wrightkit/deltin-rs/blob/0a9d695c241017cc6b070090b36bab30b09b99b0/docs/decisions.md),
the [#89 contract replacement](https://github.com/wrightkit/deltin-rs/commit/f462ec06966469f9e95340cdfadf6f08a68d3fa1),
and the ratified decisions preserved in that history.

## Decision

1. For the declared DEL/OSTW core-language surface, established upstream
   behavior is the executable specification. Compatibility inventories,
   support matrices, corpora, and real projects record implementation evidence;
   they do not decide which established core behavior belongs in scope.
2. Observable source-language behavior and invariants are implemented in typed
   Rust. Declarative data may record names, aliases, inventories, provenance,
   and support state, but it must not become an interpreted semantic language.
3. Source-language behavior has a discoverable domain home. Parser, checker,
   HIR-lowering, Workshop-lowering, and registry modules are phase infrastructure
   rather than default owners of unrelated semantic policy. A bounded extraction
   is appropriate when it keeps a changed domain cohesive; speculative generic
   frameworks and issue-specific module taxonomies are not.
4. Parsing, project loading, semantic checking, HIR, diagnostics, and source
   tooling remain useful without complete Workshop lowering when the requested
   operation does not depend on the target.

These decisions govern DEL/OSTW semantics. They do not require reproducing
upstream implementation architecture, helper identities, optimizer shape,
formatting, or generated output names.

## Consequences

Core implementation can follow observable DEL/OSTW behavior while keeping
support evidence honest and separately classified. Semantic rules remain in
code where their invariants are visible, while large factual inventories stay
data-driven. Engineers can add or repair a feature in its domain without
expanding an unrelated phase bucket or coupling semantic analysis to a backend.

## Compatibility impact

Compatibility claims target accepted/rejected programs, diagnostics and
provenance, project behavior, semantic tooling, high-level runtime intent, and
lowering results where evidenced. Passing a matrix check or matching emitted
text alone does not establish core semantic compatibility. Unsupported and
inconclusive evidence remains distinct from a match.

## Scope boundary

This ADR does not promote every historical Q1–Q16 default or support-matrix
state into a language rule. A material language decision belongs in the current
architecture contract and, when warranted, a separate ADR.
