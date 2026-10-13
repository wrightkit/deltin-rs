# OSTW / DeltinScript syntax notes (parser reference)

Precise lexical/grammar observations extracted from the pinned upstream implementation
`ItsDeltin/Overwatch-Script-To-Workshop` @ `817c1db4bace52123f054ffe10d3d8a06052e687`
(clone: `.upstream-refs/ostw`) and its wiki @ `e8894b972fae3fa9fd81dab0bb3672cc740a771e`
(clone: `.upstream-refs/ostw-wiki`).

Path abbreviations:

- `LexController.cs` = `Deltinteger/Deltinteger/Compiler/Parse/Lexer/LexController.cs`
- `Parser.cs` = `Deltinteger/Deltinteger/Compiler/Parse/Parser.cs`
- `CStyleOperator.cs` = `Deltinteger/Deltinteger/Compiler/Parse/Operators/CStyleOperator.cs`
- `Utility.cs` = `Deltinteger/Deltinteger/Compiler/Utility.cs` (enum `TokenType`)

OSTW and DeltinScript are the same language: the DeltinScript language is implemented by the
OSTW compiler (there is no separate DeltinScript repository — see `docs/provenance.md`).
Differences between "OSTW" and "DeltinScript" as *language dialects* are not observable
upstream; the only dialect-like distinction is **OSTW syntax vs. the vanilla Overwatch
Workshop superset syntax** (section 15 below).

---

## 1. Comments

Three comment kinds (`LexController.cs`: `MatchLineComment`, `MatchBlockComment`,
`MatchActionComment`; `wiki/Comments-and-documentation`):

- Line: `// text`
- Block: `/* text */`
- Action/doc: `# text` — to end of line. Before a statement it becomes a workshop comment;
  before a definition it is documentation. Example (`wiki/Comments-and-documentation`):

```
# Notify players that the game has begun.
SmallMessage(AllPlayers(), $'The game has begun!');
```

## 2. Tokens / symbols

Complete symbol table from `LexController.cs` (`MatchCSymbol`) and `Utility.cs` (`TokenType`):

```
{ } ( ) [ ] : ; .. . ~ ! , => @
= ^= *= /= %= += -=      (assignment)
^ * / % + -               (math)
++ --                     (inc/dec)
&& || |                   (boolean; | is the type-union pipe)
== != < > <= >=           (comparison)
?                         (ternary)
' " $ @" $' $"...          (strings — see section 4)
```

Note: **assignment is `=`, equality is `==`** (the upstream lexer distinguishes `TokenType.Equal`
from `TokenType.EqualEqual`). `=>` is `TokenType.Arrow` (lambda/function types).

## 3. Identifiers and numbers

- Identifiers: `[a-zA-Z0-9_]+` (`CharData.cs`). Keywords are matched first and rejected as
  identifiers (`MatchKeyword` checks a non-identifier char follows).
- Keywords (`LexController.cs` `MatchDefault`): `is import for while foreach in rule disabled
  true false null if else break continue return switch case default class struct enum new
  delete define void public private protected static override virtual recursive globalvar
  playervar persist ref this root async constructor as type single const json`.
  Plus workshop keywords: `variables subroutines settings` (English forms and localized
  forms via `VanillaSymbols`).
- Numbers: digits with optional decimal part, e.g. `5`, `0.5`, `.5`, `5.`; a leading `-` is
  lexed separately and folded into the number by the parser (`Parser.cs` `ParseNumber`,
  `IsNumber`). No hex/binary/scientific literals observed.

## 4. Strings

From `LexController.cs` (`MatchString`) and `wiki/Strings`:

| Form | Example | Notes |
|---|---|---|
| plain | `"Hello!"` / `'Hello!'` | both quote kinds, `\` escapes |
| localized | `@"Hello!"` | `@` prefix |
| interpolated | `$"Hello {name}!"` / `$'...'` | `{expr}` holes, `{{` literal brace, `$` prefix; lexed as Head/Middle/Tail tokens |
| classic format | `<"<0> killed <1>", Victim(), Attacker()>` | `Parser.cs` `ParseFormattedString`; optional `@` after `<` for localized |

The `StringExpression` parse accepts: `String`, `@ String`, `$`-interpolated, and
`< [@] String [, expr...] >` (`Parser.cs` `GetSubExpression` cases
`TokenType.String`, `TokenType.At`, interpolated tokens, and the `<`-form check
`IsFormattedString`).

## 5. Operator precedence

From `CStyleOperator.cs` (higher binds tighter), plus constants:
`TypeCastPrecedence = 11`, `ArrayIndexPrecedence = 13`, `InvokePrecedence = 14`:

| Prec | Operators | Assoc |
|---|---|---|
| 1 | `~` (squiggle; workshop value indirection) | — |
| 2–3 | `?` `:` (ternary) | right |
| 4 | `\|\|` | left |
| 5 | `&&` | left |
| 6 | `==` `!=` | left |
| 7 | `>` `<` `>=` `<=` `is` | left |
| 8 | `-` `+` | left |
| 9 | `%` `/` `*` | left |
| 10 | `^` (power) | left |
| 11 | unary `!` `-`; also `<Type>` casts | — |
| 13 | `.` and `[i]` indexing | left |
| 14 | `()` invocation | left |

Evidence examples:
- `a = b + c * d` style precedence via the table above.
- `is` binds at comparison level: `if (npc is ShopKeeper(shop_keeper_info))`
  (`wiki/Expanded-Enum-Syntax-and-Pattern-Matching`).
- Ternary: `define numberOfBosses = NumberOfPlayers() < 5 ? 1 : 2;` (`wiki/Miscellaneous`).
- Cast: `<Type>expression` (`Parser.cs` `ParseTypeCast`; `wiki/Classes`).

## 6. Rules

Grammar (`Parser.cs` `ParseRule`, `TryGetRuleCondition`; `wiki/Rules`):

```
[disabled] rule : String [Number] [ Identifier . Identifier ... ] [conditions...] Statement
condition  := [disabled] if ( Expression )
```

- `rule: "My rule" { ... }` — no event ⇒ global.
- `rule: "A player rule!" Event.OngoingPlayer { ... }` — event token before conditions.
- `rule: "Kill when touching the ground." if (!inSafeZone) if (IsOnGround(EventPlayer())) { Kill(EventPlayer()); }`
  — multiple conditions, no braces needed on the last one.
- Sort order: `rule: "Disable inspector" -1 { ... }` — number after the name
  (`wiki/Rules`; real use: `projects/modules/PathfindEditor.del` `rule: "Commands" -1`).
- Rule settings: `Identifier.Identifier` tokens after the name
  (`Parser.cs` loop building `RuleSetting`).
- `disabled rule: "" { ... }` compiles but never executes
  (`Deltinteger.Tests/LanguageTests/DisabledRuleTest.cs`).
- Vanilla rule form: `rule("my vanilla rule") { event { Ongoing - Global; } actions { Small Message(...); } }`
  (`Deltinteger.Tests/Parser/ParserTest.cs`; corpus `tests/corpus/parser/vanilla-rule.del`).

## 7. Variables

Grammar (`Parser.cs` `ParseDeclaration`, `ParseVariableElements`):

```
[attrs] Type Identifier [Number | ! | { targets }] [= Expr | : Expr] ;
```

- `globalvar define a;` / `playervar define lives = 5;` (`wiki/Variables`).
- Inferred type keyword is `define`: `define b;`
  (`wiki/Variables`; `Parser.cs` `ParseType` accepts `TokenType.Define`).
- Explicit type: `globalvar MyClass classVar = new MyClass();` (`wiki/Variables`).
- Macro variable (no assignment, `:` initializer): `public define ScopeData: RoundToInteger(EventDamage(), Rounding.Down);`
  (`projects/modules/Container.del`). `:` here is an initial-value marker (constant), distinct
  from struct-literal colons.
- Explicit ID: `define myVar 5 = EventPlayer();` (`wiki/Variables`).
- Extended collection: `globalvar define myVar! = EventPlayer();`, also on parameters
  (`void MyFunction(define parameter!)`), `wiki/Variables`.
- Reservations: `globalvar { "Variable Name", 0, 1 }` (`wiki/Variables`;
  `Parser.cs` `ParseVariableReservation`).
- Target-variable link for vanilla vars: `playervar Number checkpointIndex {'checkpoint_reached'};`
  (`wiki/Overwatch-Workshop-Superset`; `Parser.cs` `ParseOptionalTargetWorkshopVariable`).

## 8. Functions, macros, subroutines

Grammar (`Parser.cs` `ParseDeclaration`/`IsDeclaration`, `ParseAttributes`;
`wiki/Methods,-Macros-and-Subroutines`):

```
[attrs] Type Name ( params ) [String [ : String ]] { statements }   // function / subroutine
[attrs] Type Name ( params ) : Expr ;                                // macro (expression body)
[attrs] Type Name : Expr ;                                           // macro variable
```

- Examples:
  - `void method_name() { }` — void method.
  - `Vector Normal(in Vector start, in Vector end) { return RayCastHitNormal(...); }` — `in` params.
  - `Vector Normal(Vector start, Vector end): RayCastHitNormal(start, end, null, null, true);` — macro form.
  - `void Subroutine() "My subroutine!" { }` — subroutine (name string after params).
  - `void Subroutine() playervar "My Subroutine!" { }` — player-context subroutine.
  - `async MySubroutine();` / `async! MySubroutine();` — async call (`Parser.cs` `ParseAsyncExpression`).
- `ref` param: `void modify_value(ref Number variable, in Number destination = 100, in Number rate = 1) {...}` —
  default arguments allowed (`wiki/Methods,-Macros-and-Subroutines`).
- `recursive` attribute: `recursive Number factorial(Number n) { ... }`
  (`Deltinteger.Tests/HighLevelTests/RecursionTest.cs`).
- Attributes parsed in any order: `public private protected static override virtual recursive
  globalvar playervar ref in persist` (`Parser.cs` `ParseAttributes`). `abstract` is **not** a
  keyword (the wiki jokingly says "Tell Deltin to implement abstracts!" — `wiki/Classes`).
- Named arguments: `CreateHudText(VisibleTo: AllPlayers(), Header: IconString(icon), Text: text, TextColor: Color.SkyBlue);`
  (`wiki/Methods,-Macros-and-Subroutines`); positional and named can be mixed
  (`projects/modules/PathfindEditor.del`).

## 9. Types

Grammar (`Parser.cs` `ParseType`; `wiki/Lambdas-and-function-types`):

```
Type        := void
             | [const] Identifier [< Type {, Type} >] []* [| Type]*
             | [const] ( Type {, Type} ) [| Type]* => Type     // lambda/function type
             | ( Type ) []*                                     // grouped type
```

- Arrays: `Number[]`, `Number[][]`, `( () => void )[]` (function arrays,
  `wiki/Lambdas-and-function-types`).
- Function types: `String => void`, `(Number, Number) => Number`, `const () => void`.
- Pipe types (anonymous struct unions): `Number | String` (type pipe token `|`).
- Type alias: `type Name = Type;` (`Parser.cs` `ParseTypeAlias`).
- `single T` type-parameter constraint: `void function<single T>(in T[] array)` (`wiki/Structs`).
- `void` is a type usable as return type; `define` (inference) appears in type position.

## 10. Classes, structs, enums, constructors

Grammar (`Parser.cs` `ParseClassOrStruct`, `ParseEnum`, `ParseConstructor`):

```
[single] class  Name [<T,...>] [: Type {, Type}] { members }
[single] struct Name [<T,...>] [: Type {, Type}] { members }        // structs may inherit too
[single] enum   Name [<T,...>] { Identifier [( Type {, Type} )] [= Expr] {, ...} }
constructor := [attrs] constructor ( params ) [String] { statements }
```

- Members are variable/function declarations or constructors.
- Examples:
  - `class DeathZone { public Vector Location; public Any Radius; public constructor(in Vector location, in Any radius) { Location = location; Radius = radius; } }`
    (`wiki/Classes`).
  - `class Powerup { public virtual void TouchedBy(Any player) {} }`,
    `class SpeedBoost : Powerup { public override void TouchedBy(Any player) {...} }` — inheritance
    and virtual/override (`wiki/Classes`).
  - `struct Dictionary<K, V> { public K[] Keys; public V[] Values; public V Get(in K key) { return Values[Keys.IndexOf(key)]; } }`
    (`wiki/Structs`).
  - `single struct Entity { public String Name; public Number Identifier; }` (`wiki/Structs`).
  - `enum PowerupType { Wall, JumpBoost, SpeedBoost, DamageBoost }` (`wiki/Enums`).
  - Expanded enums: `enum NpcType { Basic, ShopKeeper(String), Enemy(Number) }`,
    `enum Option<T> { None = false, Some(T) = true }` (`wiki/Expanded-Enum-Syntax-and-Pattern-Matching`).
- Anonymous inline struct literal: `{ X: 0 }`, `{ Vector XYZ: Vector.Up, Number W: 0 }` (typed or
  untyped fields; `Parser.cs` `ParseStructDeclaration`), or `single { ... }` for a single-valued
  literal.
- Struct update/spread: `{ Charges: equippedItem.Charges - 1, ..equippedItem }` — `..` token is
  `TokenType.Spread` (`wiki/Structs`).
- Class allocation: `A a1 = new A();` (`Parser.cs` `ParseNew`); deallocation: `delete a;`
  (`Parser.cs` `ParseDelete`; `wiki/Classes`).
- `this` and `root` keywords are expression atoms (`Parser.cs` `GetSubExpression`).
- Static access: `Type.member`.

## 11. Statements

Grammar (`Parser.cs` `ParseStatement`, `ParseBlock`, `ParseIf`, `ParseFor`, `ParseWhile`,
`ParseForeach`, `ParseSwitch`, `ParseDelete`):

```
Block      := { Statement* }
Statement  := Block | if (Expr) Statement [else Statement] | while (Expr) Statement
            | for ( [Init] ; [Cond] ; [Iter] ) Statement
            | foreach ( Type Identifier [!] in Expr ) Statement
            | switch ( Expr ) { { case Expr : | default : | Statement }* }
            | return [Expr] ; | break ; | continue ; | delete Expr ;
            | Expression ;
```

- `if/else if/else`: `if (value == 1) { RESULT = 1; } else if (value == 3) { RESULT = 2; } else { RESULT = 3; }`
  (`Deltinteger.Tests/HighLevelTests/HighLevelTest.cs`; corpus `highlevel/if-chain.del`).
- `for (Number i = 0; i < 10; i++) a[i] = new A();` (corpus `highlevel/class-array-validation.del`).
- Auto-for (single-variable form): `for (define = start; end; step) { ... }` and
  `for (HostPlayer().a = 0; 1; 1) {}` (`wiki/Loops`; `Deltinteger.Tests/LanguageTests/TargetPlayerVariableTest.cs`).
- `foreach (Vector position in Positions) ActivateScoper(HostPlayer(), 1, new EffectMaker(position));`
  (`projects/modules/Container.del`).
- `while (historyItems.Length > historyPage) historyItems.ModRemoveByIndex(historyPage);`
  (`wiki/Lambdas-and-function-types`).
- Switch with fallthrough: `case 1: out = 2; // Fallthrough to case 2` then `case 2:` — cases do
  **not** break implicitly (`Deltinteger.Tests/HighLevelTests/HighLevelTest.cs` `Switch` test;
  corpus `highlevel/switch-fallthrough.del`). `default:` is `TokenType.Default`.

## 12. Expressions, lambdas, pattern matching

- Expression atoms (`Parser.cs` `GetSubExpression`): number, `true`/`false`, string forms, `null`,
  `this`, `root`, `new Type(args)`, `[a, b]` array literal, `async [ ! ] expr`,
  `{ ... }` struct literal, `<...>` formatted string, `import("file.json")`, lambda, identifier,
  `(expr)` group, `{...}` inline struct, `single {...}`.
- Casts: `<Type>expr` — disambiguated from `<` comparison by `IsTypeCast()` lookahead.
- Lambdas (`Parser.cs` `ParseLambda`; `wiki/Lambdas-and-function-types`):
  - `values => { ... }` — single param, block body.
  - `(x, y) => x % y` — multi-param, expression body.
  - `() => { SmallMessage(AllPlayers(), "hi"); }` — no params.
  - `(String text) => BigMessage(AllPlayers(), text);` — typed params (parentheses required).
  - `constLocation => { ... }` — `const` lambdas usable with constant workshop types.
- Pattern matching (`Parser.cs` `IsExpression`/`PatternMatching/PatternMatching.cs`;
  `wiki/Expanded-Enum-Syntax-and-Pattern-Matching`):
  - `if (EnumTest.B is EnumTest.A) { }` — full member form.
  - `if (shorthand is B(x, y)) { x = 5; y = 6; }` — shorthand + variable binding.
  - Bindings alias the operand's storage; mutability follows the operand.

## 13. Imports and projects

Grammar (`Parser.cs` `ParseImport`):

```
import String [as Identifier] ;
```

- `import "Player Controller.del";` — relative to the importing file
  (`wiki/Miscellaneous`).
- `import "!Container.del";` — `!` prefix resolves to the compiler's bundled `Modules/`
  directory (`Extras.cs` `CombinePathWithDotNotation`; real use:
  `projects/modules/PathfindEditor.del`, `projects/pathfinding/Pathfinding.del`).
- `import "customGameSettings.json";` — lobby settings file
  (`wiki/Lobby-Settings`; `projects/pathfinding/Pathfinding.del`).
- `import "file.json" as jsonVar;` — JSON as a typed variable (import expression form:
  `define x = import("Struct.json");` — `Parser.cs` `ParseJsonImport`;
  `Deltinteger.Tests/ImportJsonTest.cs`).
- Imported file extensions: `.del`, `.ostw`, `.workshop` (source), `.json`/`.lobby` (settings)
  (`Parse/Import/Importer.cs`).
- Self-import = warning; double import of the same file = silent skip; already-imported =
  warning (`Importer.cs` `ImportResult`).
- Project config: `ds.toml` at project root (`wiki/ds.toml`).

## 14. Vanilla workshop superset

OSTW accepts vanilla Workshop code inline (English only) (`wiki/Overwatch-Workshop-Superset`):

- `variables { global: 0: bot1 }` blocks (`Compiler/Syntax Tree/Superscript.cs`).
- `rule("Initialize bots") { event { Ongoing - Global; } actions { Create Dummy Bot(...); } }`.
- Localized workshop keywords come from `VanillaSymbols` (trie-based; `Parse/Vanilla/WorkshopSymbolTrie.cs`).
- Variable linking: `globalvar Player bot1 {'bot1'};`
- Subroutine linking: `void mySubroutine() 'Description': 'vanillaSubroutine' { ... }`
- Rules must be entirely vanilla **or** entirely OSTW; vanilla variable declarations must
  precede OSTW variable links.
- Vanilla target indexers: `{'var'}[..]` / `{'var'}[expr]` (`Parser.cs` `ParseVanillaTargetIndexer`).

## 15. OSTW vs "DeltinScript" observable differences

Within the pinned upstream there is no separate DeltinScript dialect to diff against: the
compiler, its language, and the wiki all treat "DeltinScript" as the name of the language OSTW
implements (the repo README is literally titled "Deltin's Script To Workshop"). The only
syntax-level split observable in the codebase is:

- **OSTW syntax** (sections 1–13): the C-style language with rules, classes, etc.
- **Vanilla workshop superset** (section 14): Workshop's own `variables`/`rule(...)`/`actions`
  syntax, parsed by the same lexer in `LexerContextKind.Workshop` mode.

No other dialect differences are observable; any claim of an "OSTW vs DeltinScript" syntax
split beyond this is unsupported by the pinned reference and should be treated as a question for
the architect before encoding it in the parser.
