# Workshop conformance boundary

`deltin-rs` owns DEL/OSTW source fixtures and source-language expectations. The
canonical Workshop feature identities, Workshop census, client captures, and
Workshop-observable expectations belong to `workshop-rs` (issue #10).

## Evidence report

`deltin-rs maintainer compatibility --json` runs the source corpus and emits report
schema 1. The legacy top-level `compatibility` alias remains accepted for CI
scripts.
Each fixture has an independent evidence classification:

| Evidence | Meaning |
|---|---|
| `pinned-oracle` | Expected source behavior is pinned to the OSTW/DeltinScript reference. |
| `real-project` | The fixture is preserved from a complete project corpus. |
| `semantic-contract` | The expectation is defined by a documented DEL/OSTW semantic contract. |
| `internal-invariant` | The assertion is explicitly about a deltin-rs representation invariant, not upstream compatibility. |

For `pinned-oracle` cases, the report requires the `// source:` URL to point at
the repository and commit recorded in the support matrix. For `real-project`
cases, the report requires a separate repository, a full 40-hex-digit commit,
and a non-empty source path. A real-project case cannot use the pinned
upstream compiler repository as its provenance. Every fixture also requires a
non-empty `// license:` marker.

The current complete project fixtures come from the pinned OSTW compiler
repository and are therefore explicitly classified as `pinned-oracle`. They
provide project-level import/semantic/HIR coverage, but are not independent
real-project evidence. An independent real-project fixture remains a visible
follow-up for #26.

Existing fixtures with a pinned `// source:` URL are classified as
`pinned-oracle` automatically; files under `tests/corpus/projects/` default to
`real-project` but must opt into `pinned-oracle` when they are compiler-owned
fixtures. A fixture may override this with `// evidence: ...`.

The report separates `matched`, `known-gaps`, `unsupported`,
`unexpected-regressions`, and `inconclusive`. An `unknown` fixture must declare
`// status: known-gap`, `unsupported`, or `inconclusive`; it can never become a
matched case because the current implementation happens to agree with it.

Optional `// matrix: feature.id, ...` directives link a source case to the
DEL/OSTW support matrix. They are validated against
`docs/support-matrix.toml` without copying Workshop catalog definitions.

Project fixtures are checked at two complementary levels: the report runs each
source entry through project loading, semantic analysis, and HIR validation,
while the project test also checks the complete import graph as one project.

## Workshop integration

When `workshop-rs#10` publishes canonical feature identities, an integration
adapter may add those IDs to the source fixture metadata and carry them with
the lowering result. Until then, `deltin-rs` records only source constructs and
the `workshop-lowering` matrix state. It must not invent or vendor a second
Workshop catalog. End-to-end assertions will compare normalized Workshop
semantics and report failures by the canonical IDs supplied by `workshop-rs`,
not by generated text, temporary variables, optimizer choices, or formatting.

The complete project fixtures remain alongside focused and minimized source
cases. The report is additive: a project-level pass does not replace focused
parser, semantic, HIR, or property/invariant tests.
