# DEL Runtime Lowering Contract

This document defines the current runtime-lowering contract for DEL/OSTW semantics that require state beyond direct expression lowering. It is a semantic contract for `deltin-rs`, not a required physical layout.

The ownership path remains:

```text
typed DEL HIR
    ↓
DEL-owned runtime lowering
    ↓
canonical public workshop_rs::Program
```

Typed HIR carries source-language intent. Runtime handles, frame stacks, dispatch tables, capture payloads, scratch variables, and helper subroutines are backend concerns owned by `deltin-rs`. `workshop-rs` continues to own only canonical Workshop identities, validation, settings/localization, `Program`, and emission.

## Object identity and allocation

A DEL class instance has one stable logical identity for its live lifetime. That identity is represented by an object handle. A handle contains enough information to locate the object's runtime slot and, when generation tracking is enabled, to distinguish different lifetimes that reuse the same slot.

The compatibility allocator follows these invariants:

- handle zero is the null / no-object value;
- live objects occupy non-zero runtime slots;
- allocation reuses the first free slot before extending object storage;
- an object's slot does not change while that object is live;
- construction allocates identity before constructor code runs so `this` and self-references observe the final identity;
- deletion marks the slot free and clears DEL-owned instance storage for that object;
- a later allocation may reuse the freed slot.

The exact Workshop variables or arrays used to realize the object table are not part of the contract.

### Generation policy and stale references

Generation tracking is a compatibility policy, not an unconditional safety feature.

When class-generation tracking is disabled, a reference is identified by its slot. After deletion and slot reuse, an old stale reference may therefore alias the new lifetime in that slot. Runtime lowering must not silently add generation checks in this mode because that changes observable upstream behavior.

When class-generation tracking is enabled, the logical handle is `(slot, generation)`. Deletion increments the slot generation before it becomes reusable. Dereference validity requires both a live slot and a matching generation, so a stale handle does not become valid when the slot is reused.

Reference validation and invalid-delete behavior follow the selected DEL/OSTW runtime settings. A failed validity check must not mutate the referenced slot. Logging/abort behavior is policy layered on the validity result; it is not encoded into typed HIR.

## Instance members, static members, and dispatch

Every live object has a runtime class identity in addition to its object handle. The class identity drives runtime type discrimination and virtual dispatch.

Instance storage obeys these invariants:

- a field declaration has one logical storage position within the compatible inheritance layout;
- field lookup is keyed by object slot plus that declaration's storage position;
- base-class fields remain addressable for derived objects;
- derived fields extend rather than reinterpret inherited storage;
- deleting an object clears the instance-storage positions reachable by that object's class family before the slot is reused.

A parallel-array layout, packed object row, or another Workshop-valid representation may implement these invariants. No one physical layout is part of the public architecture contract.

Static members are not object-indexed. They have storage associated with the declaring static member and any source-semantic specialization required by the type system. Creating or deleting an instance does not create or destroy static storage.

Virtual calls and overridable member access dispatch from the receiver's runtime class identity. The lowering may use switches, lookup data, generated subroutines, or another valid form, but it must choose the most-derived applicable implementation while preserving the statically checked DEL signature and source-level access semantics.

## References and locations

A class-typed value is a handle value and assignments copy that handle; they do not clone the object. Multiple variables holding the same valid handle therefore alias the same instance state.

A by-reference argument is different from a class reference. It denotes an assignable source location and must continue to target that location for the duration of the call. Runtime lowering must preserve the distinction already present in typed HIR between value arguments and destination/reference arguments.

Backend location descriptors are private runtime-lowering data. They must not be added to typed HIR merely because a particular Workshop encoding needs an index path, temporary slot, or helper variable.

## Calls, recursion, and activation state

Every invocation has a logical activation frame. For calls that can be nested recursively, re-entered, or invoked through a portable function value, the frame must preserve all invocation-specific state that can be observed after a nested call returns:

- value parameters and by-reference parameter locations;
- receiver / current object when applicable;
- local values whose lifetime crosses a nested invocation;
- return value state;
- the continuation needed to resume the caller.

Recursive and mutually recursive calls use LIFO frame semantics per Workshop execution domain. Player-scoped execution must not share frame state across players; global execution may use global state where source semantics permit it.

The upstream implementation demonstrates recursion with pushed parameters, receiver state, and explicit continuation positions. Those particular arrays and skip markers are implementation evidence, not required output shape. `deltin-rs` may use a different Workshop-valid encoding if nested calls restore the same logical state.

Non-recursive calls do not need to pay for the recursive frame mechanism when existing fixed-slot lowering is semantically sufficient. Runtime analysis may therefore select the cheaper path for call graphs that do not require re-entry.

### Suspension and re-entry

A value that must survive a Workshop suspension/yield cannot live only in recyclable scratch state that another invocation may overwrite. Any call form that can suspend must either:

1. place its live activation state in storage whose lifetime spans the suspension, or
2. be rejected by an explicit source/runtime compatibility diagnostic when the DEL/OSTW semantics do not permit that execution mode.

Lowering must not broaden async/re-entry behavior by accident. Existing semantic restrictions remain authoritative until independent upstream/corpus evidence establishes additional supported behavior.

Runtime stack depth is ultimately bounded by Workshop value/array/resource limits. The compiler should report statically provable validity/capacity failures; it cannot promise unbounded recursion at runtime.

## Function values and lambda captures

A portable function value has three logical parts:

```text
callable target identity
optional bound receiver
captured value payload
```

The exact serialized Workshop shape is private to runtime lowering.

For the pinned compatibility target, ordinary lambda capture is capture-by-value at function-value creation time:

- scalar/value variables contribute their current value;
- structs and other value aggregates contribute a snapshot of their value components;
- a captured class value contributes its object handle, so both the closure and outer code still refer to the same object lifetime;
- a bound instance method/function value retains its receiver handle.

Rebinding an outer variable after a value capture does not require a mutable closure cell. No general capture-by-reference closure environment is introduced without independent source-language evidence that requires one. If future evidence identifies an explicit reference-capture construct, that construct must carry location semantics deliberately rather than changing ordinary captures.

Invocation decodes the callable target, restores the bound receiver and capture payload, binds call arguments, and then enters the same activation-frame contract used by direct calls. Recursive invocation through a function value must therefore preserve frame isolation just like direct recursion.

## Value-like types

The runtime object heap is only for semantics that require class identity/lifetime.

- structs are value-like aggregates and are copied/flattened according to their typed value semantics; they do not acquire object identity merely to reuse class machinery;
- enums lower as typed scalar/domain values and do not require runtime allocation;
- ordinary arrays/collections remain value-like Workshop data when DEL semantics do not assign them independent object identity;
- class references stored inside structs, arrays, or captures remain handle values and preserve the referenced object identity;
- a future collection/runtime feature uses the object allocator only if source semantics independently require reference identity, lifetime, or aliasing.

This separation prevents a universal heap abstraction from becoming an accidental source-language model.

## Workshop boundary and validity

Runtime lowering may allocate DEL-private Workshop variables and subroutines, but emitted operations and identities must be represented through the canonical public `workshop_rs::Program` contract and validated by `workshop-rs`.

No DEL runtime helper, object-layout concept, call-frame type, or closure descriptor becomes a `workshop-rs` semantic API unless it is independently a source-language-neutral Workshop concept.

Physical lowering must account for Workshop variable, subroutine, value, array, and action constraints. Exact limits belong to canonical Workshop validation/catalog data where available rather than duplicated constants in `deltin-rs`. A runtime strategy that cannot produce valid Workshop for a source program fails explicitly; it does not silently approximate DEL behavior.

## Non-contractual implementation choices

The following may change without an architecture change when observable semantics remain stable:

- helper variable and subroutine names;
- parallel arrays versus packed storage;
- exact physical encoding of `(slot, generation)`;
- continuation token or return-stack representation;
- switch/table/helper shape used for virtual or function-value dispatch;
- scratch-slot reuse and optimizer decisions;
- formatting or emitted action order when behavior and validity are equivalent.

## Implementation seams

Implementation work may be split along these seams without inventing a new shared runtime ABI:

1. object allocator, deletion, validity policy, and member storage;
2. inheritance class identities and virtual dispatch;
3. recursive/re-entrant activation frames and return state;
4. function-value encoding, capture materialization, and invocation dispatch;
5. runtime-sensitive value aggregates/collections only where existing value lowering is insufficient.

Each seam consumes typed HIR and produces canonical Workshop `Program` data. Shared private helpers may be introduced only when at least two seams require the same invariant; a public generic runtime/VM layer is not required by this design.

## Design ablation

The design was reduced against the required semantics before fixing this contract:

- **No generic VM/runtime IR.** Typed HIR plus private lowering state is sufficient; a new public intermediate layer adds architecture without satisfying an independent requirement.
- **No mandatory generation handles.** Pinned OSTW makes generation tracking configurable, and forcing it changes stale-reference behavior.
- **No universal closure cells.** Pinned lambda lowering snapshots captured values; ordinary capture-by-reference is not evidenced.
- **No universal object heap for structs/enums/arrays.** Their value semantics do not require class identity.
- **No new `workshop-rs` runtime API.** Existing canonical `Program` ownership is the correct boundary; DEL runtime layout remains source-language-specific.
- **No upstream layout cloning.** Upstream object arrays, recursion skip markers, lambda arrays, and helper names are useful executable evidence but are not semantic contracts by themselves.

What remains is the minimum shared contract needed to keep object lifetime, dispatch, nested calls, and function values mutually compatible.

## Compatibility evidence

The pinned upstream identity is recorded in [`../provenance.md`](../provenance.md). At commit `817c1db4bace52123f054ffe10d3d8a06052e687`, the relevant executable evidence includes:

- `Deltinteger/Deltinteger/Parse/Types/Classes/ClassData.cs`: null slot, first-free allocation, optional generation-bearing references, validity checks;
- `Deltinteger/Deltinteger/Parse/Statement.cs`: deletion invalidates the slot and increments generation when enabled;
- `Deltinteger/Deltinteger/Parse/Types/Classes/ClassType.cs` and `Parse/Workshop/ClassWorkshopInitializer.cs`: object identity, instance storage, inheritance layout, deletion clearing, and runtime class relations;
- `Deltinteger/Deltinteger/Parse/Functions/Builder/RecursiveStack.cs` plus `Deltinteger.Tests/HighLevelTests/RecursionTest.cs`: recursive parameter/receiver/continuation preservation and observable recursive results;
- `Deltinteger/Deltinteger/Parse/Lambda/Workshop/CaptureEncoder.cs` and `PortableBuilder.cs`: callable identity, bound receiver, captured-value payload, and portable invocation;
- `Deltinteger/Deltinteger/Parse/Lambda/Action.cs`: captured outer-variable discovery and lambda invocation categories;
- `Deltinteger/Deltinteger/Parse/Settings.cs`: generation tracking and reference-validation behavior are explicit compiler/runtime policies.

These files are evidence for observable behavior and constraints. Their C# class structure, helper objects, names, and emitted Workshop shape are not architecture requirements for `deltin-rs`.
