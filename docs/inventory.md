# OSTW / DeltinScript feature inventory

This inventory defines the declared compatibility surface of `deltin-rs` as of the pinned
upstream references (see [provenance.md](provenance.md)):

- **ostw** = `.upstream-refs/ostw` @ `817c1db4bace52123f054ffe10d3d8a06052e687`
  (https://github.com/ItsDeltin/Overwatch-Script-To-Workshop)
- **wiki** = `.upstream-refs/ostw-wiki` @ `e8894b972fae3fa9fd81dab0bb3672cc740a771e`

Every entry names its upstream evidence location (`path@commit`, `wiki/<page>`). Entries are
the contract: anything not listed here is not a deltin-rs compatibility requirement. Upstream
internals are only relevant where observable behavior requires them (per issue #2 non-goals).

File extensions accepted as OSTW source upstream: `.del`, `.ostw`, `.workshop`
(`wiki/Getting-Started`).

---

## 1. Syntax (lexical + grammar surface)

Lexical evidence: `ostw/Deltinteger/Deltinteger/Compiler/Parse/Lexer/LexController.cs`,
`LexScanner.cs`, `CharData.cs`; token set in `Compiler/Utility.cs` (enum `TokenType`).

| Feature | Description | Evidence |
|---|---|---|
| `syntax.comments.line` | `//` line comments | `ostw/.../Parse/Lexer/LexController.cs` (`MatchLineComment`); `wiki/Comments-and-documentation` |
| `syntax.comments.block` | `/* ... */` block comments | same (`MatchBlockComment`) |
| `syntax.comments.action` | `#` doc/action comments (workshop comments on statements, doc comments on definitions) | same (`MatchActionComment`); `wiki/Comments-and-documentation` |
| `syntax.identifiers` | `[a-zA-Z0-9_]+` identifiers | `ostw/.../Parse/Lexer/CharData.cs` |
| `syntax.numbers` | integer + decimal literals (`123`, `1.5`, `.5`, `5.`), optional leading `-` | `Parser.cs` (`ParseNumber`), `LexController.cs` (`MatchNumber`) |
| `syntax.strings` | `"..."` and `'...'` string literals, `\` escapes | `LexController.cs` (`MatchString`); `wiki/Strings` |
| `syntax.strings.localized` | `@"..."` / `@'...'` localized strings | `Parser.cs` (`TokenType.At` handling); `wiki/Strings` |
| `syntax.strings.interpolated` | `$"..."` / `$'...'` with `{expr}` holes, `{{` escape, multi-token lexing | `LexController.cs` (`MatchString`); `Parser.cs` (`ParseInterpolatedString`); `wiki/Strings` |
| `syntax.strings.classic-format` | `<"<0> ... <1>", expr0, expr1>` formatted strings | `Parser.cs` (`ParseFormattedString`); `wiki/Strings` |
| `syntax.rules` | `rule: "name" { ... }`; optional `disabled`; optional numeric sort order after name; optional `setting.Setting` rule settings; optional `if` conditions before block | `Parser.cs` (`ParseRule`, `TryGetRuleCondition`); `wiki/Rules`; corpus `tests/corpus/parser/basic-rule.del` |
| `syntax.rules.events` | `Event.<EventType>` token between rule name and conditions (11 event types; see workshop-lowering) | `Parser.cs` (`TryGetRuleCondition` handles via vanilla tokens); `wiki/Rules` |
| `syntax.vanilla-rule` | `rule("name") { event { } conditions { } actions { } }` vanilla workshop rule syntax | `Parser.cs` (`IsVanillaRule`, `ParseVanillaRule`); corpus `tests/corpus/parser/vanilla-rule.del`; `wiki/Overwatch-Workshop-Superset` |
| `syntax.variables` | `[attrs] Type name [id] [= init] [;]`; `define` for inferred type; macro variables with `:` instead of `=` | `Parser.cs` (`ParseDeclaration`, `ParseVariableElements`); `wiki/Variables`; `wiki/Methods,-Macros-and-Subroutines` |
| `syntax.variables.id` | explicit workshop variable ID: `define myVar 5 = ...` | `Parser.cs` (`ParseVariableElements`); `wiki/Variables` |
| `syntax.variables.extended` | `!` extended-collection marker: `define myVar! = ...` | `Parser.cs` (`ParseVariableElements`); `wiki/Variables` |
| `syntax.variables.reservation` | `globalvar { "Name", 0 }` / `playervar { ... }` reservation blocks | `Parser.cs` (`ParseVariableReservation`); `wiki/Variables` |
| `syntax.functions` | `[attrs] Type name(params) [block | : macro;]`; `void`; `recursive`; default args `= expr` | `Parser.cs` (`ParseDeclaration`/`IsDeclaration`, `ParseAttributes`); `wiki/Methods,-Macros-and-Subroutines` |
| `syntax.functions.attributes` | `public` `private` `protected` `static` `override` `virtual` `recursive` `globalvar` `playervar` `ref` `in` `persist` (note: **no** `abstract` keyword exists) | `Parser.cs` (`ParseAttributes`); `LexController.cs` (keyword list) |
| `syntax.functions.subroutine` | subroutine name string after parameter list: `void F() "name" { }`; optional `playervar`; optional `: 'vanillaSubroutine'` link | `Parser.cs` (`ParseSubroutineName`); `wiki/Methods,-Macros-and-Subroutines`; `wiki/Overwatch-Workshop-Superset` |
| `syntax.macros` | `Type name(...): expr;` and macro variables `Type name: expr;` | `wiki/Methods,-Macros-and-Subroutines`; corpus `tests/corpus/projects/modules/Container.del` |
| `syntax.parameters` | typed params with optional `in`/`ref`/`const`, default values, `!` extended marker | `Parser.cs` (`ParseParameters`); `wiki/Methods,-Macros-and-Subroutines`; `wiki/Variables` |
| `syntax.arguments` | positional and named arguments (`Name: expr`), including in vanilla workshop calls | `Parser.cs` (`ParseParameterValues`); `ostw/overwatch-script-to-workshop/syntaxes/ostw.tmLanguage.json` (`argument-list`) |
| `syntax.types` | type names, `Type[]` array types, `Type<A, B>` generics, `const` types, `(P) => R` function types, `T | U` pipe types (anonymous struct unions) | `Parser.cs` (`ParseType`); `wiki/Lambdas-and-function-types` |
| `syntax.generics` | type parameter lists `<T, S>` on classes/structs/enums/functions; type arguments on calls and `new` | `Parser.cs` (`ParseOptionalTypeArguments`, `ParseGenerics`); corpus `tests/corpus/semantic/generic-parent-linking-ok.del` |
| `syntax.typealias` | `type Name = Type;` | `Parser.cs` (`ParseTypeAlias`) |
| `syntax.classes` | `class Name<T> [: Base] { members }` | `Parser.cs` (`ParseClassOrStruct`); `wiki/Classes` |
| `syntax.structs` | `struct Name<T> { fields }`; anonymous inline structs `{ Field: value, ... }`; `single struct` variant | `Parser.cs` (`ParseClassOrStruct`, `ParseStructDeclaration`); `wiki/Structs`; corpus `tests/corpus/highlevel/nested-single-struct.del` |
| `syntax.structs.update` | `..expr` spread/update syntax inside struct literals | `Parser.cs` (`ParseStructDeclaration` spread); `wiki/Structs` |
| `syntax.enums` | `enum Name<T> { A, B = key, C(Type, Type) }`; `single enum` variant; explicit keys | `Parser.cs` (`ParseEnum`, `TryParseEnumValueType`); `wiki/Enums`; `wiki/Expanded-Enum-Syntax-and-Pattern-Matching` |
| `syntax.constructors` | `constructor(params) [ "subroutine name" ] { }` with accessors | `Parser.cs` (`ParseConstructor`); `wiki/Classes` |
| `syntax.inheritance` | `class B : A, OtherInterface` | `Parser.cs` (`ParseClassOrStruct`); `wiki/Classes` |
| `syntax.virtual-override` | `virtual` / `override` modifiers on members | `Parser.cs` (`ParseAttributes`); `wiki/Classes`; corpus `tests/corpus/highlevel/inheritance-overrides.del` |
| `syntax.static` | `static` members; access via `Type.member` | `wiki/Classes`; corpus `tests/corpus/projects/modules/Container.del` (constructor) |
| `syntax.this-root` | `this` (inside classes), `root` (rule-level access from classes) | `LexController.cs` keyword list; `wiki/Variables` |
| `syntax.new` | `new Type(args)` class allocation | `Parser.cs` (`ParseNew`); `wiki/Classes`; corpus `tests/corpus/highlevel/class-allocation.del` |
| `syntax.delete` | `delete expr;` class deallocation | `Parser.cs` (`ParseDelete`); `wiki/Classes` |
| `syntax.imports` | `import "file.del";` statement; optional `as name`; `!` prefix = bundled Modules dir; `.json`/`.lobby` lobby settings; `import("file.json")` expression (JSON import with `as` binding) | `Parser.cs` (`ParseImport`, `ParseJsonImport`); `Parse/Import/Importer.cs`; `Extras.cs` (`CombinePathWithDotNotation`); `wiki/Miscellaneous`; `wiki/Lobby-Settings`; tests `Deltinteger.Tests/ImportJsonTest.cs` |
| `syntax.lambdas` | `(params) => expr`, `x => expr`, `() => { stmts }`, typed params `(String s) => ...`, `const` lambdas | `Parser.cs` (`ParseLambda`, `ParseLambdaParameter`); `wiki/Lambdas-and-function-types`; corpus `tests/corpus/highlevel/recursion-closure.del` |
| `syntax.functiontypes` | `(P1, P2) => R` types; `P => R` single-param shorthand; arrays of function types `(() => void)[]` | `Parser.cs` (`ParseType` lambda branch); `wiki/Lambdas-and-function-types` |
| `syntax.operators` | see full precedence table in `syntax-notes.md`; assignment ops `= += -= *= /= %= ^= ++ --`; comparison; `&& || !`; `is` pattern matching; ternary `?:` | `ostw/.../Compiler/Parse/Operators/CStyleOperator.cs`; `LexController.cs` (`MatchCSymbol`) |
| `syntax.casts` | `<Type>expr` type casts | `Parser.cs` (`ParseTypeCast`); `wiki/Classes` |
| `syntax.arrays` | `[a, b, c]` literals, empty `[]`, indexer `expr[i]`, nested | `Parser.cs` (`ParseCreateArray`, `GetArrayAndInvokes`); corpus `tests/corpus/highlevel/array-modification.del` |
| `syntax.statements` | `if/else if/else`, `for`, auto-`for` (single-expr variable form), `while`, `foreach (T x in arr)` with `!` extension, `switch/case/default` (fallthrough), `break`, `continue`, `return`, `delete`, expression statements | `Parser.cs` (`ParseStatement` and friends); `wiki/Loops`; `wiki/Miscellaneous`; corpus `tests/corpus/highlevel/switch-fallthrough.del` |
| `syntax.async` | `async expr` / `async! expr` asynchronous subroutine calls | `Parser.cs` (`ParseAsyncExpression`); `wiki/Methods,-Macros-and-Subroutines` |
| `syntax.hooks` | `expr = expr;` hook statements (vanilla target assignment) | `Parser.cs` (`ParseHook`) |
| `syntax.vanilla-context` | workshop-context lexing inside `variables { }` / `subroutines { }` / `settings { }` blocks and vanilla rules | `LexController.cs` (`MatchWorkshopContext`, `MatchLobbySettingsContext`); `Compiler/Syntax Tree/Superscript.cs`; `wiki/Overwatch-Workshop-Superset` |
| `syntax.recovery` | incomplete constructs produce structured errors without crashing (`identifier expected`, `{ expected`, missing expression recovery) | `Parser.cs` (`ParseExpected` etc.); `Deltinteger.Tests/HighLevelTests/EnumTest.cs` (`IncompleteEnumErrors`); corpus `tests/corpus/parser/enum-incomplete.del`, `incomplete-is-pattern.del` |

## 2. Semantic (binding, scopes, types, overloads, conversions, access control)

Evidence: `Deltinteger.Tests/Semantics/*`, `Deltinteger.Tests/HighLevelTests/EnumTest.cs`
(error cases), `Deltinteger.Tests/LanguageTests/*`, and the corresponding `Parse/*` sources.

| Feature | Description | Evidence |
|---|---|---|
| `semantic.scoping` | block-scoped variables; rule-level `globalvar`/`playervar` visible project-wide; `root` access from classes | `wiki/Variables`; `Parse/Scope.cs` |
| `semantic.access-control` | `public` / `private` / `protected` members (classes, structs, functions, macros, constructors) | `wiki/Classes`; `Parse/Functions/User/*` |
| `semantic.immutability` | `:` (macro/const) variables and non-variable sources cannot be assigned; "variable 'x' cannot be set" diagnostics | `Deltinteger.Tests/HighLevelTests/ArrayTest.cs`; corpus `tests/corpus/semantic/immutable-array-modification-error.del` |
| `semantic.struct-ref-methods` | struct methods that write fields require `ref`; calling a `ref` method requires a mutable variable source; `ref` methods can only be called from `ref` context | `Deltinteger.Tests/Semantics/SemanticsTest.cs`; corpus `semantic/struct-variable-guard-*.del`, `ref-call-from-*.del` |
| `semantic.struct-indexing` | parallel structs are not indexable; struct arrays and `single` structs are; diagnostic "This struct cannot be indexed" | `SemanticsTest.cs` (`ParallelStructIndexers`); corpus `semantic/struct-index-error.del` etc. |
| `semantic.condition-restrictions` | rule `if` conditions cannot be constant/parallel values | `Deltinteger.Tests/LanguageTests/RuleTest.cs`; corpus `semantic/invalid-condition-value-*.del` |
| `semantic.type-checking` | static typing, `Any` fallback, `define` inference, parallel/single type tracking | `Parse/Types/*`; `wiki/Data-types`; `wiki/Structs` |
| `semantic.type-args` | `single T` type-argument constraint (forbids parallel arguments) | `wiki/Structs` (`single` type argument constraint) |
| `semantic.enum-keys` | enum member keys cannot be constant or parallel data types | `EnumTest.cs` (`InvalidEnumKeyType`); corpus `semantic/enum-invalid-key-*.del` |
| `semantic.enum-recursion` | recursive enum/struct value types are rejected ("Type 'A' calls itself recursively") | `EnumTest.cs` (`RecursiveEnumError`); corpus `semantic/enum-recursive-*.del` |
| `semantic.pattern-matching` | `is` operand/pattern type compatibility (parallel enums reject number operands, constant/parallel operands rejected); extraneous variable bindings rejected; bound-variable mutability follows operand mutability | `EnumTest.cs`; `wiki/Expanded-Enum-Syntax-and-Pattern-Matching`; corpus `semantic/enum-*-pattern-*.del` |
| `semantic.generic-binding` | generic parent type resolution for inline and subroutine methods | `Deltinteger.Tests/Semantics/GenericsTest.cs` (issue #476); corpus `semantic/generic-parent-linking-*.del` |
| `semantic.overloads` | method groups; overload resolution by parameter/return type match for function-typed assignments | `wiki/Lambdas-and-function-types`; `Parse/Functions/MethodGroup.cs` |
| `semantic.ref-in-params` | `in` (direct use, reevaluable), `ref` (mutable variable pass-through), constant types implicitly `in`; not allowed in macros/subroutines | `wiki/Methods,-Macros-and-Subroutines`; corpus `semantic/ref-param-target.del` |
| `semantic.target-player-resolution` | player-variable targets resolved through expressions (player var, `HostPlayer().x`, struct chains, `ref` params) for workshop actions | `Deltinteger.Tests/LanguageTests/TargetPlayerVariableTest.cs`; corpus `semantic/chase-*.del`, `player-struct-target.del` |
| `semantic.disabled` | `disabled rule:` and `disabled if (...)` compile without executing | `Deltinteger.Tests/LanguageTests/DisabledRuleTest.cs`; corpus `semantic/disabled-*.del` |
| `semantic.variable-prefix` | `VariablePrefix` compiler option renames rule-level variables | `Deltinteger.Tests/LanguageTests/VariablePrefix.cs`; corpus `semantic/variable-prefix.del` |
| `semantic.lambda-capture` | captured variables are saved by value at lambda creation; captured values are read-only | `wiki/Lambdas-and-function-types` |
| `semantic.single-vs-parallel` | `single` values assignable to `Any`; parallel values have restrictions (indexer, `Any`, some array ops) | `wiki/Structs`; `EnumTest.cs` (`ParallelEnumUsedAsAny`); corpus `semantic/enum-parallel-any-errors.del` |

## 3. Runtime semantics (new/delete, references, dispatch, recursion, closures)

Evidence: `Deltinteger.Tests/HighLevelTests/*`, `Deltinteger.Tests/HighLevelTests/EnumTest.cs`
(ok cases), `Parse/Variables/Workshop/ValidateReference.cs`, `Parse/Workshop/ClassWorkshopInitializer.cs`.

| Feature | Description | Evidence |
|---|---|---|
| `runtime-semantics.class-allocation` | `new` allocates an instance (max 999); per-instance fields share `_objectVariable_x` registers | `wiki/Classes`; corpus `highlevel/class-allocation.del`; `wiki/Home-and-FAQ` |
| `runtime-semantics.delete` | `delete var;` frees the instance index; later access is an invalid reference | `wiki/Classes`; corpus `highlevel/reference-validation.del` |
| `runtime-semantics.reference-validation` | invalid reference access aborts the rule and logs `[Error] Accessed invalid reference`; `global_reference_validation` / `track_class_generations` options; `ReferenceValidationType` inline vs subroutine | `ds.toml` wiki page; corpus `highlevel/reference-validation*.del`, `generation-validation.del`, `class-array-validation.del`; `Deltinteger.Tests/TestUtils.cs` |
| `runtime-semantics.generations` | freed indexes are reused; old references to freed generations fail (pointer equality via `XOf(<Any>a1) == XOf(<Any>a2)`) | corpus `highlevel/generation-validation.del` |
| `runtime-semantics.virtual-dispatch` | virtual members dispatch by runtime type across inheritance chains (fields and methods) | `wiki/Classes`; corpus `highlevel/inheritance-overrides.del` |
| `runtime-semantics.initial-values` | class field initializers run on allocation, including inherited initializers and struct fields | issue #355; corpus `highlevel/initial-class-values*.del` |
| `runtime-semantics.recursion` | `recursive` functions: inline (continuation in action-set) and subroutine forms with variable stacks; recursive closures via function-typed globals | `wiki/Methods,-Macros-and-Subroutines`; corpus `highlevel/recursion-*.del` |
| `runtime-semantics.lambda-closures` | recursive self-referencing lambdas stored in `globalvar`; arrays through recursive closures | corpus `highlevel/recursion-closure.del`; `RecursionTest.cs` |
| `runtime-semantics.struct-copy` | value semantics: struct assignment copies; class assignment copies references | `wiki/Data-types`; `wiki/Structs` |
| `runtime-semantics.enum-storage` | no-inner / parallel / `single` enum storage shapes; `.Key` extraction; slots `_slot0..` | `wiki/Expanded-Enum-Syntax-and-Pattern-Matching`; corpus `highlevel/enum-*.del` |
| `runtime-semantics.pattern-binding` | bound pattern variables alias the operand storage (mutable iff operand mutable) | `wiki/Expanded-Enum-Syntax-and-Pattern-Matching`; corpus `highlevel/enum-binding-mutability-ok.del` |
| `runtime-semantics.switch-fallthrough` | `switch` cases fall through; `break` required; `default` | `wiki/Miscellaneous`; corpus `highlevel/switch-fallthrough.del` |
| `runtime-semantics.auto-for` | auto-`for` lowers to workshop `For()`; works with whole variables only | `wiki/Loops`; corpus `semantic/auto-for-*.del` |
| `runtime-semantics.extended-collection` | `!` variables stored in array-backed extended collections | `wiki/Variables` |
| `runtime-semantics.subroutine-context` | subroutines run in global context by default; `playervar` keyword switches to player context | `wiki/Methods,-Macros-and-Subroutines` |
| `runtime-semantics.async-calls` | `async`/`async!` subroutine invocation semantics | `wiki/Methods,-Macros-and-Subroutines` |
| `runtime-semantics.emulator` | upstream has a workshop emulator used by high-level tests (`EmulateTick`, `AssertVariable`) — evidence for runtime semantics, not a language feature | `Deltinteger.Tests/TestUtils.cs`; `Deltinteger/Emulator/*` |

## 4. Workshop lowering (actions, values, events, Workshop-only constructs)

Evidence: `ostw/Deltinteger/Deltinteger/Elements/*` (workshop element catalog `Elements.json`),
`Parse/Vanilla/*`, `Parse/Workshop/*`, `wiki/Rules`, `wiki/Chasing-and-modifying-variables`,
`wiki/Lobby-Settings`, `wiki/Overwatch-Workshop-Superset`.

> Per issue #2, these are **inventory-only until the integration stage**; they are the
> `lowering-dependent` rows of the support matrix and do not block source implementation work.

| Feature | Description | Evidence |
|---|---|---|
| `workshop-lowering.workshop-catalog` | the canonical workshop elements/actions/values catalog (vendor data, e.g. `Elements.json`); deltin-rs does **not** copy it into the repository (issue #3 non-goal) | `ostw/Deltinteger/Deltinteger/Elements/Elements.json`; `LoadData.cs` |
| `workshop-lowering.events` | 11 event types (`OngoingGlobal`, `OngoingPlayer`, `OnElimination`, `OnFinalBlow`, `OnDamageDealt`, `OnDamageTaken`, `OnDeath`, `OnHealingDealt`, `OnHealingTaken`, `OnPlayerJoin`, `OnPlayerLeave`) plus subroutines; player/assault/heal contexts | `wiki/Rules` |
| `workshop-lowering.actions` | workshop actions called from OSTW (e.g. `SmallMessage`, `CreateHudText`, `CreateEffect`, `Kill`, `Wait`) | `wiki/Rules`; `wiki/Getting-Started`; corpus `projects/modules/*.del` |
| `workshop-lowering.values` | workshop values (`TotalTimeElapsed()`, `EventPlayer()`, `EyePosition()`, ...) usable as OSTW expressions | `wiki/Workshop-Functions-in-OSTW`; corpus `projects/*/*.del` |
| `workshop-lowering.constants` | workshop enumerators (`Effect.Sphere`, `Color.Red`, `Button.Interact`, `Operation.Max`, `Rounding.Down`, `RateChaseReevaluation.*`) | `wiki/Getting-Started`; `wiki/Chasing-and-modifying-variables`; corpus `tests/corpus/semantic/player-struct-target.del` |
| `workshop-lowering.variables` | global/player/scope variable assignment; IDs; `Set Variable (At Index)` vs `Modify Variable (At Index)`; `Initial Global` / `Initial Player` generated rules | `wiki/Variables`; `wiki/Overwatch-Workshop-Superset`; `wiki/Rules` |
| `workshop-lowering.extended-collection` | `_extendedGlobalCollection` / `_extendedPlayerCollection` array storage | `wiki/Home-and-FAQ`; `wiki/Variables` |
| `workshop-lowering.classes` | `_objectVariable_x` instance storage; class arrays; memory functions `ClassMemory*()` | `wiki/Classes`; `Global Functions/ClassMemory.cs` |
| `workshop-lowering.structs` | parallel structs (one workshop var per field) vs `single` structs (one array per value); mapping/filtering costs | `wiki/Structs` |
| `workshop-lowering.enums` | enum storage: plain number, parallel slots (`_slot0`), `single` arrays | `wiki/Expanded-Enum-Syntax-and-Pattern-Matching` |
| `workshop-lowering.vanilla-superset` | mixing vanilla `variables {}` / `rule(...)` / actions into OSTW projects; variable and subroutine linking (`{'vanilla name'}`) | `wiki/Overwatch-Workshop-Superset`; corpus `tests/corpus/parser/vanilla-rule.del` |
| `workshop-lowering.auto-for` | `for` with a plain variable/condition form lowers to workshop `For Player/Global Variable` | `wiki/Loops`; `Deltinteger.Tests/LanguageTests/TargetPlayerVariableTest.cs` |
| `workshop-lowering.chase-modify` | `ChaseVariableAtRate/OverTime`, `StopChasingVariable`, `ModifyVariable` with `Operation.*`; player-target extraction | `wiki/Chasing-and-modifying-variables`; corpus `semantic/chase-*.del`, `modify-*.del` |
| `workshop-lowering.subroutines` | subroutine rules with parameters and return values; `async` calls | `wiki/Methods,-Macros-and-Subroutines` |
| `workshop-lowering.strings` | custom strings, localized strings, interpolated strings, classic `<...>` formats compile to workshop string actions | `wiki/Strings`; `Parse/Strings/*` |
| `workshop-lowering.lobby-settings` | `import "customGameSettings.json"` merges lobby settings into output | `wiki/Lobby-Settings`; corpus `projects/pathfinding/customGameSettings.json` |
| `workshop-lowering.workshop-comments` | `#` action comments become workshop comments | `wiki/Comments-and-documentation` |
| `workshop-lowering.output-formats` | `c_style_workshop_output` (new `Global.x = ...` syntax vs old `Set Global Variable(...)`), `use_tabs_in_workshop_output` | `ds.toml` wiki page |

## 5. Compiler utility (NOT part of the language contract)

These are upstream compiler-side capabilities deltin-rs may provide as utilities, but they are
**not** OSTW language features and do not define compatibility:

| Feature | Description | Evidence |
|---|---|---|
| `compiler-utility.optimizer` | constant folding, vector shortcutting, `Value In Array` → `First Of`; controlled by `optimize_output` | `wiki/Optimizing`; `ostw/Deltinteger/Deltinteger/Elements/Optimize.cs` |
| `compiler-utility.element-count` | element-count estimation model | `wiki/Optimizing` |
| `compiler-utility.emulator` | workshop emulator for testing | `ostw/Deltinteger/Deltinteger/Emulator/*` |
| `compiler-utility.pathfinding` | `Pathmap` class, `.pathmap` files, `Pathfind` rules, pathmap editor | `wiki/Pathfinding`; `ostw/Deltinteger/Deltinteger/Pathfinder/`; corpus `projects/pathfinding/` |
| `compiler-utility.model-import` | `import "file.obj"` model import + custom text | `wiki/Importing-Models-and-Custom-Text`; `ostw/Deltinteger/Deltinteger/Model/` |
| `compiler-utility.workshop-string-utility` | standalone workshop string utility tool | `ostw/Deltinteger/Deltinteger/Workshop String Utility/` |
| `compiler-utility.asset-exporter` | alphabet/model export tooling | `ostw/Deltinteger/Deltinteger/Asset Exporter/` |
| `compiler-utility.ds-toml` | `ds.toml` project configuration (`entry_point`, `out_file`, `optimize_output`, `global_reference_validation`, `track_class_generations`, `reference_validation_type`, `abort_on_error`, `log_delete_reference_zero`, `new_class_register_optimization`, `reset_nonpersistent`, `paste_check_is_extended`, `subroutine_stacks_are_extended`, `c_style_workshop_output`, `compile_miscellaneous_comments`, `use_tabs_in_workshop_output`) | `wiki/ds.toml` |
| `compiler-utility.json-type` | `import("file.json") as name` runtime JSON values | `wiki/Importing-data-from-.json-files`; `Deltinteger.Tests/ImportJsonTest.cs` |

## 6. Decompiler

| Feature | Description | Evidence |
|---|---|---|
| `decompiler.workshop-to-ostw` | decompile workshop text to OSTW: text → elements → code | `ostw/Deltinteger/Deltinteger/Decompiler/` (`TextToElement/`, `ElementToCode/`, `Decompiler.cs`); `wiki/Decompiling` |
| `decompiler.settings` | lobby settings extracted during decompile | `Decompiler/Json/DecompilerMeta.cs`; `wiki/Decompiling` |
| `decompiler.editor-commands` | clipboard/insert commands are editor integration (see editor section) | `wiki/Decompiling`; `overwatch-script-to-workshop/src/decompile.ts` |

## 7. Editor (VS Code extension / language server — **out of scope for the deltin-rs language contract**)

Everything in this category is explicitly **not** a deltin-rs compatibility requirement. It is
listed so the matrix can mark it `out-of-scope` rather than silently dropping it.

| Feature | Description | Evidence |
|---|---|---|
| `editor.language-server` | LSP server (`Server.cs`, `StdServer.cs`, handlers) | `ostw/Deltinteger/Deltinteger/Language Server/*` |
| `editor.incremental-parse` | incremental lexer/parser for live editing (token ranges, relexing) | `ostw/.../Compiler/Parse/Lexer/Increment.cs`, `LexState.cs`; `Deltinteger.Tests/Parser/ParserTest.cs` |
| `editor.completions` | completion, hover, signature help, definition links | `Parse/Import/Importer.cs` (completion ranges); `Language Server/Handlers/*` |
| `editor.semantic-tokens` | semantic token highlighting | `Language Server/SemanticTokenHandler.cs` |
| `editor.codelens` | reference/implements code lenses | `Parse/CodeLens.cs`; `wiki/Getting-Started` |
| `editor.snippets` | code snippets | `Language Server/Snippets.cs` |
| `editor.extension` | VS Code extension host (commands, output panel, version selector, workshop panel) | `ostw/overwatch-script-to-workshop/src/*` |
| `editor.tmlanguage` | TextMate grammar (used above as *syntax-surface evidence*, but shipping a VS Code grammar is editor scope) | `ostw/overwatch-script-to-workshop/syntaxes/ostw.tmLanguage.json` |
| `editor.debugger` | workshop debugger integration | `wiki/Debugger`; `Deltinteger/Debugger/*` |
| `editor.element-count-ui` | element count display | `wiki/Optimizing` |

## 8. Project (multi-file loading)

Evidence: `tests/corpus/projects/`, `Parse/Import/Importer.cs`, `wiki/ds.toml`.

| Feature | Description | Evidence |
|---|---|---|
| `project.import-resolution` | relative-path `import "file.del";` resolution from the importing file; extension dispatch (`.del`/`.ostw`/`.workshop` source vs `.json`/`.lobby` settings); cycle/self-import/double-import handling | `Parse/Import/Importer.cs`; corpus `tests/corpus/projects/pathfinding/Pathfinding.del` |
| `project.modules-resolution` | `!`-prefixed imports resolve to a configured modules directory (upstream: the compiler's bundled `Modules/`) | `Extras.cs` (`CombinePathWithDotNotation`); corpus `tests/corpus/projects/modules/PathfindEditor.del` |
| `project.lobby-settings-import` | importing `customGameSettings.json` merges lobby settings | `wiki/Lobby-Settings`; corpus `tests/corpus/projects/pathfinding/customGameSettings.json` |
| `project.pathmap-loading` | `new Pathmap("Map.pathmap")` loads pathmap data at compile time | corpus `tests/corpus/projects/pathfinding/Map.pathmap`; `wiki/Pathfinding` |
| `project.ds-toml` | `ds.toml` project configuration discovery; this slice loads `entry_point` and syntax-validates other keys without interpreting compiler options | `wiki/ds.toml`; corpus `tests/corpus/projects/ds-toml/`; negative `tests/corpus/projects/invalid-ds-toml/` |

---

## Evidence gaps / notes

- The upstream test-suite raw strings (`.del` snippets inside C# `[TestMethod]` bodies) are the
  primary corpus source; there is no separate upstream `tests/*.del` directory of source files.
- Large real-world vanilla workshop scripts exist at
  `ostw/Deltinteger/Deltinteger.Tests/Assets/TestWorkshopScripts/*.ostw` (used by
  `SuperscriptTest.cs` for atomize/reconstruct). They were **not copied** into the corpus
  (size 1,243–14,108 lines, no per-file headers upstream); they remain available as
  differential-test input for the vanilla-workshop path and are referenced here as evidence.
- The `Deltinteger.Tests/Assets/TestJsonImport/*.json` files back the `import("...json")`
  fixtures; the import expression is documented above.
- Upstream wiki pages can drift from the pinned wiki commit; the pinned wiki SHA
  `e8894b972fae3fa9fd81dab0bb3672cc740a771e` is the authoritative doc reference.
