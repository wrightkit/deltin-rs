# PM Ratifications for architecture.md §21 (Q1–Q16)

Status: **ratified** · Owner: PM · Date: 2026-08-16. These are binding product decisions
for the Engineer. Evidence authority: `docs/inventory.md`, `docs/syntax-notes.md`,
`tests/corpus/**`, `docs/support-matrix.toml`, and the pinned upstream clones
(`.upstream-refs/ostw` @ `817c1db4bace52123f054ffe10d3d8a06052e687`,
`.upstream-refs/ostw-wiki` @ `e8894b972fae3fa9fd81dab0bb3672cc740a771e`).
Where evidence is absent, the permissive/`planned` default applies and is marked
**default-applied**.

## Issue #31 storage slice boundary

The first DEL-owned storage slice materializes only scalar, value-semantics locals
declared in the same `Event.OngoingGlobal` rule body. The adapter maps each
`HirVarId` to the deterministic synthetic global name
`__del_rule_local_<HirVarId>` and preserves the source declaration span on the
generated WIR variable and write targets. This is a backend lowering strategy,
not a change to parser, semantic checking, typed HIR, or the canonical Workshop
catalog/WIR.

The shared global table is not a local-variable ABI. To avoid suspension or
re-entry aliasing, the slice rejects player context, functions, methods,
parameters, closures, recursive calls, and any external Workshop action in the
rule body (including actions that may suspend or restart rules). Uninitialized
locals, arrays, objects, structs, references, `foreach`, and unsupported value
shapes remain structured `HI018` failures and produce empty WIR. The local map
is scoped to one rule lowering and uses the common runtime-global allocator, but
that does not make the slot safe for overlapping activations. The slice therefore
provides no evidence for the remaining #31 runtime strategies or for advancing
the support matrix.

---

1. **Lambda capture dialect — ByValue only.**
   Evidence: `docs/inventory.md` `semantic.lambda-capture` ("captured variables are saved
   by value at lambda creation; captured values are read-only");
   `wiki/Lambdas-and-function-types` §Variable capturing ("the value of the variable is
   saved. The values of these saved variables cannot be changed after the lambda
   expression declaration"); corpus `tests/corpus/highlevel/recursion-closure.del`
   (self-referencing lambda stored in `globalvar`; captures never mutated).
   Decision: `CaptureMode::ByValue` is the only implemented mode (snapshot at creation,
   read-only). `ByReference` remains a documented model/oracle extension, never exercised
   by fixtures or the checker dialect. No dialect switch exists.
   Matrix: `semantic.lambda-capture` stays `planned` until the semantic milestone flips it
   with ByValue-only semantics.

2. **`in` parameter assignability — error (SM017).**
   Evidence: upstream `Parse/Variables/Builders/IVariableComponent.cs`
   (`case AttributeType.In: varInfo.VariableTypeHandler.SetWorkshopReference();`) →
   `Builders/VariableTypeHandler.cs` (`GetVariableType()` returns
   `VariableType.ElementReference`) → `Variables/Workshop/Gettables.cs`
   (`WorkshopElementReference.CanBeSet() => false`; `Set`/`Modify` throw "Cannot modify
   WorkshopElementReference") → `Variables/Semantics/Resolve/VariableResolve.cs` ("The
   variable 'X' cannot be set"). Wiki `Methods,-Macros-and-Subroutines.md`: constant
   types "cannot be set to" and "Macro parameters are always `in`". Corpus:
   `tests/corpus/semantic/ref-param-target.del` assigns through a `ref` param (the legal
   contrast); `projects/modules/PathfindEditor.del` constructors read `in` params but only
   assign fields.
   Decision: assignment to an `in` parameter is `SM017`; `in` values are read-only
   reevaluable elements. Architecture §13.7 default confirmed.
   Matrix: `semantic.ref-in-params` stays `planned` (flips `semantic-supported` at M2 with
   this behavior).

3. **Import extension resolution — extension required; no `.del`/`.ostw` fallback.**
   Evidence: upstream `Parse/Import/Importer.cs` `ImportResult`: path combined verbatim
   (`Extras.CombinePathWithDotNotation` → `Path.Combine(directory, file)`), `FileType =
   Path.GetExtension(...)`; missing file ⇒ error "The file '...' does not exist." No
   extension-appending exists. Corpus: every import carries an extension
   (`projects/pathfinding/Pathfinding.del` `import "!Debug Camera.del";` /
   `import "customGameSettings.json";`, `projects/modules/PathfindEditor.del`
   `import "!Container.del";`).
   Decision: resolve the literal path as written relative to the importing file's
   directory; `import "x"` (no extension) is `PJ002` unless a file literally named `x`
   exists. **Correction to architecture.md §11**: drop the proposed "try `.del` then
   `.ostw`" fallback — it is not upstream behavior.
   Matrix: `project.import-resolution` stays `planned` (flips `source-supported` at M1
   with exact-path semantics).

4. **`import "x" as name` and `!` imports — `as` inert for source imports; `!` = modules
   dir, resolution `planned`.**
   Evidence: upstream `Compiler/Parse/Parser.cs` `ParseImport` (parses `as` identifier);
   `Parse/Import/Importer.cs`: the `as` identifier is used **only** for `.json` (typed
   variable binding); for `.del`/`.ostw`/`.workshop` imports it is parsed and ignored.
   `Extras.cs` `CombinePathWithDotNotation`: `!` prefix ⇒ `Path.Combine(Program.ExeFolder,
   "Modules")` (compiler-bundled directory). Corpus manifest
   (`tests/corpus/projects/modules/.manifest.md`) maps `!` imports to the corpus modules
   dir; no `as` usage in any corpus file.
   Decision: parse `import "x" as name` (syntax); no namespace semantics — the `as`
   binding is inert for source files (upstream-observable). `as` on `.json` statement
   imports (typed variable) is semantic behavior → `planned`. `!` imports parse as
   `BundledModule`; the modules directory is corpus/resolution-config dependent → the
   resolution target is `planned` (architecture §11 Q-4 reference confirmed).
   Matrix: `project.modules-resolution` stays `planned`; `syntax.imports` flips at M1
   (parse).

5. **`import("file.json")` expression — parse-only in M1; semantics `planned`.**
   Evidence: `wiki/Importing-data-from-.json-files.md` (`define NUM_BOTS =
   import("data.json").NUM_BOTS;` with member access and JSON→type conversion);
   upstream `Parser.cs` `case TokenType.Import: return ParseJsonImport();`. Corpus: no
   usage anywhere. Decision: **default-applied** — the expression parses
   (`ExprKind::JsonImport`); member access/typing/conversions are not checked until a
   semantic milestone; documented intentional-`planned`.
   Matrix: `compiler-utility.json-type` stays `planned`.

6. **Named + positional args — positional bind by index; named bind by name; named args
   must trail.**
   Evidence: upstream `Parse/Parameters/Overload Chooser/OverloadChooser.cs`
   (`ParametersFromContext`: duplicate name ⇒ "The parameter X was already set.";
   `MatchOverload`: non-picky → `OrderedParameters[i] = inputParameters[i]`; picky →
   matched by `option.Parameters[p].Name`, unknown ⇒ "Named argument 'X' does not exist
   in the function ..."; positional after named ⇒ "Named argument 'X' is used
   out-of-position but is followed by an unnamed argument"). Corpus
   `projects/modules/PathfindEditor.del:19`: `CreateHudText(AllPlayers(), Text:"...",
   TextColor:Color.Blue, Location:Location.Right, SortOrder:0)` — mixed call, positional
   first.
   Decision: positional args fill parameters by source index in order; named args bind by
   parameter name (trailing named arguments only); named-before-positional ordering is an
   error (the architecture §13.6 "positional ... before named args" rule is ratified); codes
   `SM010` (unknown name), `SM011` (duplicate), and `SM053` (ordering violation).
   Matrix: `semantic.overloads` stays `planned` (M2).

7. **`const` scope — function types only; no const scalar variables/params.**
   Evidence: `wiki/Lambdas-and-function-types.md` (const function types
   `const () => void myConstantFunction;`, const-typed params
   `const Location => void cast`, const lambdas; "cannot be reassigned once they have
   been changed"); corpus `projects/modules/PathfindEditor.del:628`
   `void pressLoop(const () => void action)`; immutable *variables* exist only in the
   `:`-init form (`projects/modules/Container.del:10` `public define ScopeData: ...`).
   Decision: `const` is valid on function types (declarations, params, lambdas) only;
   `const` on scalar variables/params is not part of the surface. Assignment to a const
   function value = `SM015`.
   Matrix: `syntax.functiontypes`, `syntax.lambdas` stay `planned` (M1).

8. **Type-name shadowing — legal; user types shadow builtins.**
   Evidence: upstream `Parse/Translate.cs` (builtins added to `GlobalScope` via
   `ScriptTypes.AddTypesToScope`; user types appended to the child `RulesetScope`);
   `Parse/Types/Semantics/TypeFromContext.cs` (`scope.TypesFromName(name)` → `providers[0]`,
   "TODO: Check ambiguities" — no conflict diagnostic);
   `TypeHelpers.cs` `TypesFromName` walks innermost-first ⇒ user type wins over builtin.
   `Number`/`Boolean`/`Any`/`Void` are not keywords (syntax-notes §3 keyword list).
   Corpus: no fixture either way (**evidence thin — upstream-code decision**).
   Decision: user classes/structs/enums may be named `Number`, `Boolean`, `Any`, `Void`,
   etc.; resolution order is innermost scope → … → project scope → builtins → provider
   (architecture §13.4 ratified); no shadowing diagnostic.
   Matrix: no state change (`semantic.type-checking` stays `planned`).

9. **`Players` type — reserved, unexercised, `planned`.**
   Evidence: corpus — zero occurrences (only vanilla "All Players" action text in
   `tests/corpus/parser/vanilla-rule.del`). Upstream `Parse/ScriptTypes.cs`: `Players()`
   is an internal factory (`new PipeType(Player(), PlayerArray())`) **not** registered in
   `GetDefaults()`/`AllTypes` ⇒ not name-resolvable in user code.
   Decision: **default-applied** — keep `Players` in the type list (task-fixed,
   reserved); no resolution path binds the identifier `Players`; document as
   intentional-`planned`.
   Matrix: no dedicated entry exists; no state change.

10. **`interface` — no keyword, no semantics; `class B : A, X` parses with extra types
    inert.**
    Evidence: corpus — zero `interface` occurrences. Upstream: no `interface` keyword
    (syntax-notes §3 keyword list); `Parser.cs` `ParseClassOrStruct` parses the full
    `: Type {, Type}` list; `Parse/Types/Classes/User/DefinedClassProvider.cs` uses only
    `_typeContext.Inheriting[0]` — additional types are parsed and ignored (no error, no
    interface checks).
    Decision: **default-applied** — parse the implements list and record it (architecture
    `TypeDecl.implements`); no interface semantics (satisfaction, multi-interface
    resolution) — matrix `planned`; no `interface` keyword anywhere.
    Matrix: `syntax.inheritance` stays `planned` (flips `source-supported` at M1 with
    parse+ignore semantics).

11. **Union types `T | U` — parsed; assignability/member semantics `planned`.**
    Evidence: corpus — zero union-type fixtures. Upstream `Parse/Types/Global/PipeType.cs`
    implements unions (merged member scopes via `scope.CopyAll` per included type;
    `PipeTypeOperatorInfo` forwards operators) and uses them internally
    (`ScriptTypes.Players()` = `Player | Player[]`). Inventory `syntax.types` lists
    `T | U`.
    Decision: **default-applied** — parse `T | U` (`TypeRefKind::Union`); no
    assignability/member checks until corpus evidence or a dedicated milestone; record
    upstream merged-scope behavior in `syntax-notes.md` as reference.
    Matrix: `syntax.types` stays `planned`.

12. **Default member visibility — Public.**
    Evidence: upstream `Parse/Attributes.cs` (`AccessLevel { Public, Private, Protected }`
    with `Public = 0`; `GenericAttributeAppender.Accessor` is never assigned without an
    access attribute ⇒ defaults to `Public`). Corpus: all fixture members are explicitly
    `public` (e.g. `tests/corpus/highlevel/initial-class-values.del`,
    `inheritance-overrides.del`, `player-struct-target.del`) — no counter-evidence; wiki
    `Classes.md` documents the three levels but no default.
    Decision: member/constructor without an access modifier is `Public`. Architecture
    §13.5's "per corpus (Q-12)" is resolved: Public.
    Matrix: `semantic.access-control` stays `planned` (M2 flips with Public default).

13. **Rule name — required.**
    Evidence: upstream `Parser.cs` `ParseRule`: `Token name =
    ParseExpected(TokenType.String);` — missing name is a parse error. Corpus: every rule
    in every fixture carries a name, including the empty string (`parser/basic-rule.del`
    `rule: "hello" {}`; `semantic/auto-for-player-var.del` `rule: ""`).
    Decision: `rule:` name string is required; empty string `""` is legal; `rule { ... }`
    is a parse error. Architecture §9 `RuleDecl.name` "required per corpus (Q-13)"
    confirmed.
    Matrix: `syntax.rules` stays `planned` (flips at M1).

14. **Auto-for grammar — one classic-`for` grammar; auto-for = iterator-is-expression
    classification.**
    Evidence: upstream `Parser.cs` `ParseFor` (single grammar:
    `for (init; cond; iter) stmt`); `Parse/Loops.cs:118` `IsAutoFor =
    forContext.Iterator is ExpressionStatement`; auto-for initializer may be a declaration,
    an assignment, or a bare expression ("start at 0"); restrictions
    `CanBeIndexed = false, FullVariable = true`; errors "Auto-for loops require an
    initializer." and "Expression in for loop must be a variable declaration, assignment,
    or variable identifier". Wiki `Loops.md` documents `for (define = start; end; step)`
    and `for (var = start; end; step)`. Corpus `semantic/auto-for-player-var.del`
    (`for (a = 0; 1; 1) {}`) and `semantic/auto-for-host-var.del`
    (`for (HostPlayer().a = 0; 1; 1) {}`) — both `expect: ok`.
    Decision: both forms must parse; the parse-level distinction is the upstream
    classification (iterator slot is an expression statement ⇒ auto-for). Implementation
    note: the auto-for target may be a member expression (`HostPlayer().a`), so
    `AutoForVar::Existing` must carry an expression lvalue target, not only an `Ident`
    (architecture §9 shape note). Semantic restrictions land with the semantic milestone.
    Also note architecture.md line 501's "(corpus: Q-15)" label is a typo for Q-14.
    Matrix: `runtime-semantics.auto-for` stays `planned`.

15. **Generic inference — explicit type args required; argument-based inference only when
    unambiguous.**
    Evidence: upstream `OverloadChooser.cs` `ExtractInferredGenerics` (infers type args
    from argument types and from expected return type; second pass after lambda typing;
    incomplete inference ⇒ `InferSuccessful = false`, linkers cleared) — inference
    exists but only succeeds when every type arg is accounted for. Corpus: only explicit
    class-generic instantiation (`tests/corpus/semantic/generic-parent-linking-ok.del`
    `Test<Number> t = new Test<Number>();`); no generic-*function* call fixture; wiki
    `Structs` Dictionary example is struct generics only.
    Decision: implement explicit `None<Number>()` type args; argument-based inference
    only when unambiguous (all type args derived, no conflicts); ambiguous/incomplete
    inference is a diagnostic, never silent `Any`. Architecture §13.6 ratified.
    Matrix: `semantic.generic-binding` stays `planned`.

16. **Number literals — decimal int/real only; no hex, no binary, no scientific.**
    Evidence: upstream `LexController.cs` `MatchNumber` (digits, optional `.` with
    optional trailing digits; `.5` and `5.` legal; bare `.` rejected; no `0x` path);
    `docs/syntax-notes.md` §3 ("No hex/binary/scientific literals observed");
    `docs/inventory.md` `syntax.numbers` ("`123`, `1.5`, `.5`, `5.`"). Corpus: zero hex
    occurrences. **Correction to architecture.md §7**: "hex int (corpus)" is unsupported
    — no corpus hex evidence exists.
    Decision: the lexer accepts `\d+`, `\d+\.\d*`, `\.\d+` (with parser folding of a
    leading `-`); no hex/binary/scientific forms or suffixes; `syntax-notes.md` §3 stands.
    Matrix: `syntax.numbers` stays `planned` (flips at M1).

---

## Matrix state impact summary

| Q | Entry | State now → |
|---|-------|-------------|
| 1 | `semantic.lambda-capture` | `planned` (ByValue-only at M2) |
| 2 | `semantic.ref-in-params` | `planned` (SM017 on `in` assignment at M2) |
| 3 | `project.import-resolution` | `planned` (exact-path semantics at M1; §11 fallback removed) |
| 4 | `project.modules-resolution` | `planned` (resolution target undecided) |
| 5 | `compiler-utility.json-type` | `planned` (parse-only at M1) |
| 6 | `semantic.overloads` | `planned` |
| 7 | `syntax.functiontypes`, `syntax.lambdas` | `planned` |
| 8 | — | no change |
| 9 | — | no change (reserved, unexercised) |
| 10 | `syntax.inheritance` | `planned` (parse+ignore at M1) |
| 11 | `syntax.types` | `planned` |
| 12 | `semantic.access-control` | `planned` (Public default at M2) |
| 13 | `syntax.rules` | `planned` (name required at M1) |
| 14 | `runtime-semantics.auto-for` | `planned` |
| 15 | `semantic.generic-binding` | `planned` |
| 16 | `syntax.numbers` | `planned` (decimal-only at M1) |

No matrix entry changes state in this document; all entries remain `planned` as filed.
State flips happen at M1/M2 milestones with fixture evidence, per the milestone gates recorded
in GitHub issue/PR history. Two architecture.md corrections are recorded (Q3 extension
fallback; Q16 hex claim).

## #31 bounded runtime slice (2026-08-18)

The released `workshop-rs 0.1.1` WIR has global/player variable tables and
structured control-flow actions, but no local-variable table, parameterized
subroutine ABI, or runtime stack. The first DEL-owned runtime slice therefore
uses only evidence-backed encodings that remain inside those public WIR forms:

- An unstable `switch` scrutinee is assigned once to a generated global helper
  slot in a global rule; every case comparison reads that slot. Player-context
  dynamic switches fail closed with `HI018` rather than sharing that global
  temp across players. Recursive materialization and any construct that cannot
  lower to a WIR value also fail closed with `HI018`.
- Parameter storage, foreach, member storage, object lifetime, recursion
  stacks, and return values remain outside this slice. The bounded scalar local
  subset above is lowering evidence only; no HIR layout,
  parser contract, or canonical WIR node is changed to make them appear
  supported.

The support matrix remains `lowering-dependent`; these tests are implementation
evidence for the bounded adapter slice, not end-to-end Workshop execution proof.
