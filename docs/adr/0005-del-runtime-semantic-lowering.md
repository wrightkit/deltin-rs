# ADR-0005: DEL runtime semantic-lowering compatibility

## Status

Proposed

## Context

ADR-0003 established the ownership path from typed DEL HIR through DEL-owned lowering into canonical `workshop_rs::Program`, but deliberately did not choose the runtime contract for classes, references, recursive calls, function values, captures, or related runtime policies.

Issue #104 requires that boundary to become explicit before the remaining runtime work in #31 is decomposed. Without a durable decision, individual implementation issues could independently choose incompatible object layouts, reference policies, call-state conventions, or closure representations.

The pinned OSTW implementation is an executable compatibility reference, not an architecture template. WrightKit targets observable source semantics and real-project usability rather than compiler-output identity or a clone of OSTW's internal runtime structures.

## Decision

### Compatibility target

`deltin-rs` targets **observable semantic compatibility plus practical resource compatibility** with the pinned OSTW revision recorded in `docs/provenance.md`.

A different physical lowering is acceptable when it preserves supported observable behavior and does not materially reduce practical Workshop usability for representative provenance-linked projects/corpus cases.

No fixed element, variable, action, or other resource-count ratio is part of the compatibility contract.

### Pinned compatibility authority

Runtime compatibility is evaluated against the pinned OSTW revision, currently `817c1db4bace52123f054ffe10d3d8a06052e687`.

New upstream commits or releases do not automatically change the contract. Updating the pin requires an explicit compatibility review of changed observable behavior.

### Semantic contract, not shared physical ABI

The runtime contract fixes cross-feature semantic invariants, not one physical runtime representation.

Object tables, handles, frame state, continuation encoding, dispatch structures, function-value payloads, capture storage, scratch state, and helper subroutines remain private lowering choices unless independent observable evidence makes a representation property compatibility-sensitive.

No generic runtime IR, mini-VM, or public runtime framework is introduced between typed DEL HIR and canonical Workshop solely to unify these features.

### Runtime settings and staged readiness

Observable runtime-policy modes provided by the pinned compatibility target remain compatibility targets. Their defaults follow the pinned OSTW defaults; `deltin-rs` does not silently select a safer or otherwise different default.

Engine readiness may be staged. A valid DEL construct or runtime mode that has not yet been implemented remains valid through parsing, type checking, and typed HIR. The runtime capability/lowering boundary must emit an explicit unsupported-capability diagnostic rather than reinterpret the source, approximate its semantics, or turn a backend gap into a source-language error.

This applies to non-default reference/generation policies and to async, suspension, and re-entry modes as their implementations are added.

### Evidence threshold

Pinned source implementation alone is not enough to make an apparent runtime behavior a durable semantic requirement.

A semantic requirement needs observable evidence from an upstream executable test, executable differential fixture, provenance-linked corpus/real-project case, or explicit upstream documentation.

Implementation-only behavior remains implementation evidence or an evidence gap. Upstream bugs/quirks are preserved only when compatibility evidence shows supported programs depend on them.

### Object/reference compatibility

The contract preserves evidenced object identity, lifetime, member behavior, inheritance/override behavior, deletion/validation behavior, and supported reference-policy semantics.

It does not freeze a general slot numbering scheme, first-free allocator, object-table shape, null sentinel representation, or handle encoding merely because the pinned implementation uses one.

Where a reference representation is observably exposed, only the evidenced relationship is compatibility-sensitive. For example, pinned high-level tests observe reuse of a pointer component under generation-enabled delete/reallocate behavior; that does not by itself promote the complete first-free allocator algorithm into the contract.

### Calls and function values

Synchronous nested/recursive calls must preserve the caller-observable state required by pinned executable recursion tests, including recursive function-value invocation.

This does not require a universal activation-frame ABI. Non-recursive and recursive paths may use different private representations when behavior remains equivalent.

Function-value/capture representation is likewise private. Ordinary lambda capture timing and copy/reference semantics are not frozen until the evidence threshold is met; current upstream capture-encoder source is implementation evidence, not sufficient by itself.

### Practical resource compatibility

Resource compatibility is evaluated against representative, provenance-linked workflows rather than fixed counts.

If a supported program that is practically usable under the pinned upstream target becomes invalid under Workshop resource limits solely because `deltin-rs` introduces materially worse runtime overhead, that is a compatibility regression.

Comparative resource counts remain evidence/metrics rather than durable API constants.

### Ownership boundary

Runtime semantics and source-specific lowering remain owned by `deltin-rs`. The result must use canonical public `workshop_rs::Program` and applicable Workshop validation.

DEL object layouts, reference policies, call frames, closure payloads, and helpers do not become `workshop-rs` APIs unless they independently represent source-language-neutral Workshop semantics.

## Alternatives rejected

### Clone the pinned OSTW runtime layout

Rejected because observable compatibility does not require helper names, arrays, allocator implementation, optimizer shape, or emitted Workshop identity. Cloning those details would turn implementation history into architecture without independent requirements.

### Define one unified runtime ABI or runtime IR

Rejected because the current requirements only need shared semantic invariants. A universal ABI/IR would add abstraction and state before repeated implementation requirements prove it necessary.

### Make generation-bearing references mandatory

Rejected because generation/reference policies change observable behavior and resource cost, and the pinned compatibility target exposes multiple modes with established defaults.

### Use safer WrightKit-specific defaults

Rejected because the same unconfigured DEL project should not silently receive different runtime semantics from the pinned compatibility target.

### Treat backend gaps as language errors

Rejected because parser/type/HIR ownership describes valid DEL semantics, while runtime support is an engine-readiness property. Conflating them would falsely narrow the language.

### Require fixed resource-count parity

Rejected because exact compiler shape is not a compatibility goal and dynamic counts are not stable architecture contracts. Representative practical usability is the relevant constraint.

### Treat pinned source code as sufficient semantic specification

Rejected because source alone cannot distinguish intended observable behavior from implementation detail, optimizer choice, or accidental bug.

## Consequences

- Implementation issues can be decomposed around object/reference lifetime, inheritance/dispatch, synchronous recursion, function values/captures, and later runtime modes without first agreeing on one shared physical ABI.
- Engine support may advance incrementally while unsupported valid modes fail explicitly at the lowering capability boundary.
- Differential/upstream/corpus evidence is required before implementation details are promoted into durable semantics.
- Resource regressions are judged by provenance-linked workflow loss rather than arbitrary ratios.
- The ordinary lambda-capture model remains an explicit evidence gap and must not be invented by an Engineer task.
- A future repeated need for shared private runtime structures may justify a local abstraction under the normal Rule-of-Three/AHA bar; this ADR does not pre-authorize one.
- Independent design ablation remains required before this ADR can move from `Proposed` to `Accepted`.

## Related

- [Issue #104](https://github.com/wrightkit/deltin-rs/issues/104)
- [Issue #31](https://github.com/wrightkit/deltin-rs/issues/31)
- [ADR-0003](0003-hir-workshop-boundary.md)
- [`../architecture/runtime-lowering.md`](../architecture/runtime-lowering.md)
- [`../provenance.md`](../provenance.md)
