# Workshop conformance boundary

`deltin-rs` owns DEL/OSTW source fixtures and source-language expectations. The
canonical Workshop feature identities, Workshop census, client captures, and
Workshop-observable expectations belong to `workshop-rs` (issue #10).

## Verification

Source-side conformance is verified by ordinary tests (see
[`compatibility.md`](compatibility.md)):

- `tests/corpus.rs` walks the fixture corpus and asserts each fixture's
  declared `// expect:` outcome through parse → project → semantic → HIR.
- `tests/corpus/projects/` fixtures are exercised as complete projects through
  dedicated project tests covering the full import graph.
- `tests/real-projects/` vendors third-party projects (mobawatch, protect-ban)
  exercised end-to-end by `tests/real_projects.rs`; this satisfies the
  independent real-project coverage issue #26 tracked.
- `tests/reconstruction.rs` verifies the declared Workshop→OSTW reconstruction
  boundary against `tests/reconstruction-fixtures/`.

## Workshop integration

When `workshop-rs#10` publishes canonical feature identities, an integration
adapter may add those IDs to the source fixture metadata and carry them with
the lowering result. Until then, `deltin-rs` records only source constructs and
the `workshop-lowering` matrix state. It must not invent or vendor a second
Workshop catalog. End-to-end assertions compare the canonical Workshop
structure of `deltin-rs` and pinned upstream OSTW output, both parsed by
`workshop-rs`, and report failures by the canonical IDs it supplies, never by
generated text or formatting.

Project-level passes complement, and do not replace, focused parser, semantic,
HIR, or property/invariant tests.
