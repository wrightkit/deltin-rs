# Historical PM decision log — compatibility pointer

The previous `decisions.md` recorded PM ratifications for the original monolithic `architecture.md`, including Q1–Q16 decisions, milestone/support-matrix transitions, and issue-specific lowering slices.

Those records remain available in Git history as decision provenance. They are **not** a current architecture contract or a status database.

Current durable architecture is routed from [`docs/architecture/README.md`](architecture/README.md). Current support state comes from [`support-matrix.toml`](support-matrix.toml), [`compatibility.md`](compatibility.md), the corpus/real-project test surfaces, and the implementation itself.

When a historical decision remains semantically relevant, restate the stable invariant in the appropriate current architecture/domain contract rather than requiring Engineers to reconstruct current design from this old Q1–Q16 log. If a current Issue depends on a historical decision that is not represented in current contracts, surface that gap during implementation preflight instead of treating this file as implicitly binding.
