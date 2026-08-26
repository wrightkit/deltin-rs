# deltin-rs

`deltin-rs` is WrightKit's standalone Rust implementation of the DeltinScript /
OSTW language surface. It is intended to be useful on its own as a library and
CLI for parsing, loading, type-checking, inspecting, compiling, and eventually
reconstructing supported `.del` / `.ostw` projects.

Wright is a downstream consumer that integrates `deltin-rs` with broader tooling
such as linting, analysis, validated source editing, agent workflows, CI, and
language services. An LPP **provider** is an integration
role that `deltin-rs` may expose to Wright or other tooling clients.

Canonical raw Workshop behavior is shared rather than duplicated. `deltin-rs`
owns DEL/OSTW syntax, project loading, semantic/type behavior, runtime lowering,
compiler behavior, diagnostics/provenance, standalone tooling, compatibility
evidence, and Workshop-to-DEL reconstruction. `workshop-rs` owns canonical
Workshop catalog identities, WIR, validation, settings/localization, raw
Workshop parsing, and emission.

```text
DEL / OSTW source
    ↓
deltin-rs parsing / project loading / semantic analysis
    ↓
DEL semantic model / typed HIR
    ↓
deltin-rs runtime + compiler lowering
    ↓
workshop-rs canonical WIR / validation / emission
    ↓
Workshop text
```

The reverse direction starts with Workshop parsed by `workshop-rs` and uses
`deltin-rs`-owned reconstruction logic to produce useful DEL/OSTW source.

## Features

- **Recoverable parsing:** authored text, comments, trivia, identifiers, and
  source locations are retained for diagnostics and source tooling.
- **Project loading:** deterministic multi-file import resolution and project
  discovery.
- **Semantic analysis:** name and type resolution, overloads, access control,
  classes, structs, enums, inheritance, virtual dispatch, generics, lambdas,
  pattern matching, and recursion checks.
- **Typed semantic representation:** allocation/deletion, references, dispatch,
  recursion, closures, and storage intent remain backend-neutral until lowering.
- **Tooling APIs:** symbol, reference, type, and resolution queries for
  standalone consumers and Wright.
- **Compiler integration:** DEL HIR lowers through canonical `workshop-rs` WIR;
  unsupported runtime/project behavior remains explicit.
- **Compatibility evidence:** machine-checked support matrix, corpus fixtures,
  provenance records, bounded semantic oracle, and evidence reports.

## Compatibility

Compatibility targets observable DeltinScript / OSTW semantics for the declared
support surface, not upstream compiler architecture, formatting, helper names,
temporary variables, or output-text identity.

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
| DEL/OSTW → Workshop compilation | 🟡 Partial | Core HIR→WIR lowering exists; advanced runtime/project surfaces are incomplete |
| Workshop → DEL/OSTW reconstruction | ⏳ Not yet | Will consume canonical `workshop-rs` semantics and remain owned by `deltin-rs` |

Exact feature evidence lives in the
[machine-readable support matrix](docs/support-matrix.toml); see
[`docs/compatibility.md`](docs/compatibility.md) for methodology and state
meanings.

## CLI and library

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
[`docs/implementation-role.md`](docs/implementation-role.md) for the durable
relationship and [`docs/architecture.md`](docs/architecture.md) for internal
implementation details.

## Relationship with Wright

Wright is not the owner of DEL/OSTW language semantics. It consumes `deltin-rs`
and adds a unified product layer across DEL/OSTW, OverPy, and raw Workshop,
including cross-language lint, analysis, source-edit transactions, agent
interfaces, CI/embedding, and language services.

LPP/provider support is therefore an adapter surface for integration, not the
identity of this repository.

## Building

Requirements: Rust 1.85+.

```sh
cargo build --release
cargo test --all-targets
```

Install the current CLI from source with:

```sh
cargo install --path .
```

## Validation

Before claiming implementation work complete, run the repository quality gates
and the affected compatibility/corpus checks. Real-project support claims must
also be revalidated against the relevant full project rather than inferred from
unit-test counts alone.

## Documentation

Architecture, compatibility, interfaces, provenance, limitations, and
maintainer references are indexed in [`docs/README.md`](docs/README.md).

## Contributing

This repository is part of the WrightKit multi-repository workspace. Apply the
workspace-level `AGENTS.md` first, then this repository's local `AGENTS.md`.

## License

`deltin-rs` is distributed under the [MIT license](https://opensource.org/licenses/MIT).
Compatibility fixtures retain their recorded upstream provenance and licensing.
