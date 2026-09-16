# deltin-rs Current Architecture

This directory routes the **current** architecture contracts for `deltin-rs`.

Use it for substantive implementation preflight. Keep these evidence classes separate:

- documents here state current durable language/ownership contracts;
- source, Cargo metadata, tests, corpus, project fixtures, and integrations establish current implementation reality;
- `support-matrix.toml` and compatibility evidence record current evidenced support, not the definition of the DEL/OSTW core language;
- `docs/architecture.md` and `docs/decisions.md` are retained only as compatibility/history entry points for the earlier implementation baseline.

## Routing

| Concern | Current contract / authority |
| --- | --- |
| DEL/OSTW core scope, project/semantic ownership, typed implementation model | [`language-core.md`](language-core.md) |
| Runtime/lowering boundary with canonical Workshop | [`workshop-boundary.md`](workshop-boundary.md) |
| Objects, references, recursive/re-entrant calls, function values, and captures | [`runtime-lowering.md`](runtime-lowering.md) |
| Current support evidence/state | [`../support-matrix.toml`](../support-matrix.toml), [`../compatibility.md`](../compatibility.md), corpus/real-project evidence |
| Pinned upstream identity/provenance | [`../provenance.md`](../provenance.md) |
| Syntax observations | [`../syntax-notes.md`](../syntax-notes.md), when consistent with upstream/current evidence |
| CLI contract | [`../cli.md`](../cli.md) |

## Decision history

[`docs/adr/`](../adr/README.md) records the rationale for durable architecture
choices. It is separate from this current contract and from implementation
reality. The registry classifies historical material, externally owned
decisions, and unresolved future work so that proposed migrations are not read
as accepted architecture.

Do not put current dependency versions, crate layout snapshots, feature counts, Issue progress, milestone state, or transient lowering gaps in this directory. If an Issue, historical design note, current contract, and code reality disagree, surface the mismatch before implementation.
