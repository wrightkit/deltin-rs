# ADR-0005: Tests-first verification for DEL/OSTW compatibility

- Status: Accepted
- Date: 2026-10-04
- Related: [Issue #112](https://github.com/wrightkit/deltin-rs/issues/112),
  `wrightkit/.github#60`, [`compatibility.md`](../compatibility.md);
  supersedes the evidence-model framing in [ADR-0002](0002-language-core-semantics.md)

## Context

Verification grew a first-class evidence subsystem: a source-code taxonomy
(`EvidenceSource`, `FixtureStatus`, report schemas), per-fixture
`// evidence:`/`// status:`/`// matrix:` bookkeeping headers, mandatory
provenance headers on WrightKit-authored fixtures, matrix `evidence` path
lists, and a ~180-file `compatibility/ostw/` package of observation databases,
upstream-output probes, and a langserver oracle runner.

That subsystem wrapped the same parse → project → semantic → HIR pipeline the
ordinary corpus harness already executed, so it was a second database rather
than additional verification: two harnesses ran the same fixtures, the
probes/result files were consumed by no executable check, and the vocabulary
(`matched`/`known-gap`/`inconclusive`/`unexpected-regression`) described
record-keeping states, not test outcomes.

## Decision

1. Compatibility verification is expressed as ordinary tests and test data.
   There is no first-class evidence model: no evidence-source taxonomy, no
   fixture status lifecycle, no report schema in production code.
2. Fixture expectations are concrete outcomes. Walked corpus fixtures declare
   `// expect: ok | parse-error | semantic-error | hir-error`; an undecided
   expectation is not a test contract.
3. Attribution stays minimal and concrete. `// source:`/`// license:` headers
   and `LICENSE` files remain only where fixtures derive from or vendor
   third-party material under the pinned upstream revision.
   WrightKit-authored fixtures carry no provenance bookkeeping.
4. `support-matrix.toml` records declared support state only; it no longer
   requires per-entry evidence-path lists.
5. Committed reference data is retained only where an executable test consumes
   it as input: vendored real projects under `tests/real-projects/` and the
   reconstruction boundary under `tests/reconstruction-fixtures/` (whose
   `support-boundary.json` declares the fixture roles its test verifies).
   Point-in-time observation databases are not committed.
6. Structural comparison against the pinned upstream OSTW output remains the
   source→Workshop compatibility target (ADR-0003, `language-core.md`). It is
   delivered as ordinary tests over `workshop-rs`-parsed canonical programs,
   not as a committed database of recorded upstream observations.

## Consequences

- `src/compatibility.rs`, the `maintainer compatibility`/`compatibility` CLI
  commands, the corpus evidence/status/matrix headers, matrix evidence lists,
  and the dormant `compatibility/ostw/` package are removed. Their surviving
  verification content lives in `tests/corpus.rs`, `tests/real_projects.rs`,
  and `tests/reconstruction.rs`.
- A fixture that cannot be classified as a concrete expected outcome is a gap
  to resolve, not a status to record.
- Differential probing against a pinned upstream build remains a maintainer
  gap-discovery workflow outside the repository; its conclusions land as
  ordinary tests and fixtures, not as committed observation records.

## Scope boundary

This ADR governs how compatibility is verified, not what the language covers.
Core scope remains governed by [`language-core.md`](../architecture/language-core.md)
and ADR-0002's scope decisions.
