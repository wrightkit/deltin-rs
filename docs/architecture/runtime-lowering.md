# DEL Runtime Semantic Lowering Contract

This document defines the semantic contract for DEL/OSTW runtime behavior that cannot be lowered as isolated expressions. It deliberately does **not** define a shared physical runtime ABI.

The ownership path remains:

```text
typed DEL HIR
    ↓
DEL-owned runtime capability check + lowering
    ↓
canonical public workshop_rs::Program
```

Typed HIR carries valid DEL source semantics. `deltin-rs` owns runtime capability checks and source-language-specific lowering. `workshop-rs` owns canonical Workshop identities, validation, settings/localization, `Program`, and emission. Runtime object tables, handles, frame storage, dispatch data, capture payloads, scratch variables, and helper subroutines remain private `deltin-rs` implementation choices unless independent evidence makes part of their behavior observable.

## Compatibility target

Runtime compatibility means **observable semantic compatibility plus practical resource compatibility** with the pinned OSTW reference recorded in [`../provenance.md`](../provenance.md).

For the current pin, `817c1db4bace52123f054ffe10d3d8a06052e687`:

- DEL programs within the declared support surface must preserve upstream-observable behavior;
- physical Workshop layout, helper names, optimizer shape, and emitted text/action identity need not match upstream;
- different lowering strategies are allowed when they preserve semantics and practical Workshop usability;
- an upstream update does not silently change this contract; changing the pin requires an explicit compatibility review.

Compatibility does not require compiler-output identity.

## Evidence threshold

A behavior becomes a durable semantic requirement only when it has observable evidence from at least one of:

- a pinned upstream executable test;
- an executable differential fixture;
- a provenance-linked corpus or real-project regression;
- explicit upstream documentation that defines the behavior.

Pinned upstream source is useful implementation evidence, but source shape alone does not promote an allocator algorithm, helper layout, optimization, or apparent quirk into the semantic contract.

When only implementation evidence exists, the behavior remains an evidence gap or implementation observation. An upstream bug or quirk is preserved only when observable compatibility evidence shows that supported programs depend on it.

## Runtime readiness and unsupported modes

Backend readiness does not redefine the DEL language.

A DEL construct that is valid under the pinned compatibility target remains valid through parsing, type checking, and typed HIR even when its runtime lowering is not implemented yet. The runtime capability/lowering boundary must then either:

1. lower the construct with the required semantics, or
2. emit an explicit unsupported-capability diagnostic.

It must not convert a backend gap into a syntax/type error, silently choose a different runtime policy, or emit an approximate lowering with different observable behavior.

This permits staged engine readiness without inventing false support.

## Runtime settings and defaults

Runtime settings that change observable behavior remain part of the compatibility target even when individual modes are implemented in separate slices.

Defaults follow the pinned OSTW target. In particular, class-generation tracking is not made mandatory merely because it can reject more stale references; the pinned default remains the compatibility default.

For any runtime setting:

- a supported mode must match its evidenced upstream semantics;
- a mode that belongs to the compatibility target but is not implemented must fail at the runtime capability/lowering boundary;
- `deltin-rs` must not silently replace the requested mode with a safer, cheaper, or otherwise different mode.

## Objects, references, and lifetime

Class instances require source-visible identity and lifetime semantics. The runtime representation must preserve the observable consequences of aliasing, member access, inheritance/dispatch, deletion, validation, and supported reference policies.

Pinned high-level tests provide observable evidence for class allocation and field state, inheritance and overrides, invalid-reference handling, class arrays, and class-generation behavior. In generation-enabled tests, deletion followed by allocation can reuse the prior reference's observable pointer component while validation still distinguishes the stale lifetime.

The contract therefore requires the evidenced lifetime and validation relationships, not a particular allocator algorithm.

The following are **not** fixed by this contract without additional observable evidence:

- numeric slot assignment in general;
- a first-free search algorithm;
- the physical null/sentinel encoding;
- the shape of the object table;
- whether an internal handle is scalar, vector-like, packed, or split across storage.

If a representation component is observable through valid DEL/Workshop interop, only the evidenced observable relation is compatibility-sensitive. The rest remains private lowering state.

The pinned implementation suggests additional behavior for generation-disabled stale references and exact slot reuse, but source inspection alone is insufficient to freeze those details. They require an observable test, differential fixture, documented contract, or provenance-linked real-project case before an implementation issue may treat them as required semantics.

## Members, inheritance, and dispatch

Class member behavior, inherited state, and override dispatch are observable source semantics. Pinned high-level tests exercise inheritance and overrides across multiple runtime classes, so lowering must preserve those results.

The contract does not prescribe how those results are encoded. Instance fields may use parallel arrays, packed records, generated indexes, or another Workshop-valid representation. Runtime class discrimination may use tables, switches, generated subroutines, or another private strategy.

Static members likewise follow DEL source semantics; their physical storage is not object layout and is not fixed by this contract.

No class-layout or dispatch helper becomes a `workshop-rs` API merely because multiple DEL features use it.

## Calls and synchronous recursion

Synchronous nested and recursive calls must preserve all caller-observable invocation state needed to produce the pinned behavior. This includes whichever parameters, receiver state, local state, return state, and continuation state are semantically live across a nested invocation.

Pinned recursion tests exercise direct/subroutine recursion, recursive array state, and recursive invocation through a closure/function value. These results are compatibility requirements.

The contract does not require a universal activation-frame type or a particular LIFO array/continuation encoding. A non-recursive call may use a cheaper lowering when it preserves behavior. Recursive lowering may use any private representation that restores the same observable state.

## Suspension, async, and re-entry

Upstream-supported async, suspension, and re-entry semantics remain compatibility targets, but engine readiness may be staged independently from synchronous recursion.

Until a mode has sufficient observable evidence and an implementation that preserves it, `deltin-rs` must reject that mode with an explicit runtime capability diagnostic rather than approximate it or broaden its behavior by accident.

No unbounded recursion or suspension capacity is promised: every lowering remains subject to Workshop resource and validity constraints.

## Function values and captures

Function values must preserve the callable behavior, bound receiver behavior, and captured state that are observable under the pinned DEL semantics. Recursive function-value invocation is already covered by pinned executable tests.

The physical representation of a function value is private. A callable identifier, receiver, and captured payload may appear in the pinned implementation, but that serialized array shape is not an ABI requirement.

### Capture-semantics evidence gap

The pinned implementation currently snapshots captured values when encoding a portable lambda and flattens struct components into its payload. That is implementation evidence from `Parse/Lambda/Workshop/CaptureEncoder.cs`; it is not yet sufficient under this contract's evidence threshold to freeze ordinary capture timing or copy/reference semantics as a durable requirement.

Before an implementation issue relies on ordinary capture-by-value, capture-by-reference, mutable closure cells, or a specific class-capture rule, it must add or identify observable evidence for that behavior.

This evidence gap is not permission to invent a closure model. Until it is closed, no universal closure-cell ABI is introduced.

## Value-like types and runtime identity

Runtime identity machinery is introduced only where source semantics require independent identity, lifetime, or aliasing.

- classes require runtime identity/lifetime semantics;
- structs and enums remain value-like typed data unless independent evidence requires identity;
- ordinary arrays/collections do not acquire class-object identity merely to reuse class machinery;
- future collection/runtime features may use identity machinery only when their source semantics require it.

This classification prevents a universal object heap from becoming an accidental language model.

## Practical resource compatibility

Resource compatibility is judged by representative usability, not a fixed element/variable multiplier.

A runtime strategy is a compatibility regression when a provenance-linked, representative program that is valid and practically usable under the pinned upstream target becomes invalid under Workshop limits solely because `deltin-rs` introduces materially worse runtime resource cost.

Element, variable, subroutine, action, and related counts may be recorded as comparative metrics, but individual counts and ratios are not durable public contracts. Regression evidence should preserve the project/revision/path or minimized fixture that demonstrates the lost workflow.

Exact Workshop limits belong to canonical Workshop validation/catalog data where available; `deltin-rs` must not duplicate changing limits as runtime architecture constants.

## Workshop boundary

Runtime lowering may allocate DEL-private Workshop state and helpers, but the emitted result must use the canonical public `workshop_rs::Program` boundary and pass applicable `workshop-rs` validation.

A runtime strategy that cannot represent a supported source program as valid Workshop fails explicitly. It must not silently approximate DEL semantics.

DEL object layouts, reference policies, call frames, and function-value encodings remain source-language-specific and do not become `workshop-rs` semantics unless they are independently source-language-neutral Workshop concepts.

## Non-contractual implementation choices

Unless separate observable evidence says otherwise, the following may change without an architecture decision:

- allocator search strategy, including first-free versus another reuse strategy;
- object-table and member-storage layout;
- helper variable and subroutine names;
- physical handle encoding beyond evidenced observable behavior;
- frame, return-state, and continuation representation;
- function-value and capture-payload serialization;
- dispatch table/switch/helper shape;
- scratch-slot reuse and optimizer decisions;
- exact resource counts when practical usability remains intact;
- formatting and emitted action order when semantics and validity are equivalent.

## Implementation seams

Implementation may be decomposed without inventing a shared physical ABI:

1. object/reference lifetime and the pinned default runtime-policy baseline;
2. inheritance/member behavior and virtual dispatch;
3. synchronous nested/recursive call state;
4. function values and ordinary capture semantics after the capture evidence gap is closed;
5. additional runtime settings and async/suspension/re-entry modes as evidence and engine readiness permit.

Each seam consumes typed HIR and produces canonical Workshop `Program` data. Private helpers should be shared only when multiple implemented seams demonstrably need the same invariant; no generic runtime IR, VM, or public runtime framework is required.

## Architecture exclusions

This contract intentionally does not introduce:

- a unified physical DEL runtime ABI;
- a generic runtime IR or mini-VM between typed HIR and Workshop;
- mandatory generation-bearing references;
- a fixed allocator algorithm;
- universal mutable closure cells;
- a universal object heap for value-like types;
- a fixed resource-count ratio against upstream;
- a DEL-specific runtime API in `workshop-rs`;
- upstream helper/layout cloning as an architecture requirement.

These exclusions keep the contract at the minimum level required for semantic compatibility and real-project usability.

## Evidence map

The pinned identity is recorded in [`../provenance.md`](../provenance.md). Evidence for this contract currently includes:

| Behavior | Observable evidence | Implementation evidence | Contract treatment |
| --- | --- | --- | --- |
| Class allocation, member state, initial values | `Deltinteger.Tests/HighLevelTests/HighLevelTest.cs` | `Parse/Types/Classes/ClassType.cs`, `Parse/Workshop/ClassWorkshopInitializer.cs` | Observable behavior required; physical storage private. |
| Inheritance and overrides | `Deltinteger.Tests/HighLevelTests/HighLevelTest.cs` (`Inheritance & overrides`) | class relation/dispatch implementation | Observable dispatch results required; dispatch representation private. |
| Delete, invalid-reference validation, pointer reuse with generations | `Deltinteger.Tests/HighLevelTests/HighLevelTest.cs` class-generation/reference-validation tests | `Parse/Types/Classes/ClassData.cs`, `Parse/Statement.cs`, `Parse/Settings.cs` | Evidenced validation/reuse relationships required; general allocator algorithm is not. |
| Runtime-policy defaults/modes | pinned OSTW settings plus the compatibility decision in ADR-0005 | `Parse/Settings.cs` | Preserve pinned defaults and target modes; staged support may diagnose unsupported modes. |
| Synchronous recursion | `Deltinteger.Tests/HighLevelTests/RecursionTest.cs` | `Parse/Functions/Builder/RecursiveStack.cs` | Observable recursive results required; stack representation private. |
| Recursive function-value invocation | `Deltinteger.Tests/HighLevelTests/RecursionTest.cs` | lambda portable-builder implementation | Observable invocation result required; payload ABI private. |
| Ordinary lambda capture timing/copy semantics | **No independent observable evidence identified yet** | `Parse/Lambda/Workshop/CaptureEncoder.cs`, `Parse/Lambda/Action.cs` | Evidence gap; do not freeze or invent semantics from source shape alone. |
| Async/suspension/re-entry details | evidence required per mode before implementation claims support | async/call implementation | Compatibility target with staged readiness; unsupported modes diagnose explicitly. |
| First-free allocation, exact object/frame/capture layouts, helper names | none required for compatibility | pinned source implementation | Non-contractual unless future observable evidence proves otherwise. |

ADR-0005 records the architecture choices and rationale behind this contract. The architecture document remains the authority for the current invariant once that decision is accepted.
