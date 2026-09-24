# deltin-rs

`deltin-rs` is a standalone Rust library and CLI for the DeltinScript / OSTW
language surface. It parses, type-checks, and lowers DeltinScript projects to
Overwatch Workshop code without requiring external .NET runtimes.

While Wright and external editors consume `deltin-rs` through native Rust APIs or
LPP, the compiler and semantic analyzer operate independently.

`deltin-rs` owns DEL/OSTW syntax, project loading, semantic analysis, type
checking, runtime lowering, and source reconstruction. Shared Workshop
semantics, catalog identities, and emission remain delegated to `workshop-rs`.

```text
DEL / OSTW source
    ↓
deltin-rs parsing / project loading / semantic analysis
    ↓
DEL semantic model / typed HIR
    ↓
deltin-rs runtime + compiler lowering
    ↓
workshop-rs canonical Program / validation / emission
    ↓
Workshop text
```

The reverse direction starts with Workshop parsed by `workshop-rs` and uses
`deltin-rs`-owned reconstruction logic to produce useful DEL/OSTW source.

## Key features

- Recoverable parsing: retains authored text, comments, trivia, and exact
  locations for accurate diagnostics and refactoring tools.
- Project resolution: deterministic multi-file import handling and project
  discovery.
- Semantic analysis: scoping, type inference, method overloads, classes,
  structs, enums, interfaces, and virtual dispatch.
- Typed representation: high-level semantic model that stays backend-neutral
  until lowering.
- Semantic queries: symbol, reference, and type lookups for editors and Wright.
- Workshop code generation: lowers DEL HIR into the canonical `workshop-rs`
  `Program` model with
  explicit error reporting for unsupported runtime behavior.
- Verified compatibility: validated against corpus fixtures, oracle snapshots,
  and differential tests.

## Compatibility

For the declared support surface, compiled Workshop output converges
structurally on the pinned upstream OSTW compiler output (rule order, element
identities, control flow, conditions, values, variable names and indices, and
element cost) when both outputs are parsed as canonical Workshop programs;
formatting is not compared. Source analysis and tooling surfaces target
observable DeltinScript / OSTW semantics, not upstream compiler architecture.

| Capability | Status | Notes |
| --- | --- | --- |
| Syntax & parsing | ✅ Supported | Recoverable parser with source/trivia evidence |
| Projects & imports | ✅ Supported | Multi-file import resolution; project/compiler surfaces continue to expand |
| Type checking | ✅ Supported | Scoping, overload resolution, access control |
| Classes, structs & enums | ✅ Semantic support | High-level semantics exist; some concrete Workshop runtime lowering remains incomplete |
| Inheritance / virtual dispatch | ✅ Semantic support | Concrete runtime lowering is still being closed |
| Generics / lambdas / pattern matching / recursion | ✅ Semantic support | End-to-end Workshop behavior remains evidence-gated where applicable |
| Embedded Workshop / lobby data | 🟡 Partial | Canonical Workshop contracts are still being integrated |
| Workshop builtins | 🟡 Partial | Canonical catalog binding exists; breadth and lowering continue to expand |
| DEL/OSTW → Workshop compilation | 🟡 Partial | Core HIR→Program lowering exists; advanced runtime/project surfaces are incomplete |
| Workshop → DEL/OSTW reconstruction | ⏳ Not yet | Will consume canonical `workshop-rs` semantics and remain owned by `deltin-rs` |

Exact implementation evidence lives in the
[machine-readable support matrix](docs/support-matrix.toml); see
[`docs/compatibility.md`](docs/compatibility.md) for methodology and state
meanings. The matrix records current support; it does not define which
established upstream core-language features belong in scope.

## CLI and library

Rust embedding uses the `deltin-rs` package and does not pull CLI-only
dependencies such as `clap` or `clap_complete`. Install the executable package
from source with `cargo install --path cli`.

```text
deltin-rs check <file-or-dir> [--json]
deltin-rs inspect <file> <line>:<col> [--json]
deltin-rs support [--check] [--json]
deltin-rs dev parse <file> [--json]
deltin-rs dev hir <file-or-dir> [--json]
deltin-rs completion <bash|zsh|fish|powershell>
deltin-rs maintainer compatibility [--json]
```

The standalone semantic/tooling path does not require Wright. Workshop-dependent
compilation uses the released `workshop-rs` library. See
[`docs/architecture/README.md`](docs/architecture/README.md) for the current
architecture and dependency boundary.

## Relationship with Wright

`deltin-rs` owns the syntax, type system, and AST for DeltinScript. Higher-level
tools such as Wright consume these semantic results to provide cross-language
refactoring, linting, and language server capabilities without modifying
compiler internals.

## Building

Requirements: Rust 1.85+.

```sh
cargo build --workspace --release
cargo test --workspace --all-targets
```

## Validation

Before claiming implementation work complete, run the repository quality gates
and the affected compatibility/corpus checks. Real-project support claims must
also be revalidated against the relevant full project rather than inferred from
unit-test counts alone.

## Documentation

Current architecture, compatibility/evidence, interfaces, provenance,
limitations, and maintainer references are indexed in
[`docs/README.md`](docs/README.md).

## Contributing

This repository is part of the WrightKit multi-repository workspace. Apply the
workspace-level `AGENTS.md` first, then this repository's local `AGENTS.md`.

## License

`deltin-rs` is distributed under the [MIT license](https://opensource.org/licenses/MIT).
Compatibility fixtures retain their recorded upstream provenance and licensing.
