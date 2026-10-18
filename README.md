# deltin-rs

`deltin-rs` is a standalone Rust library and CLI for the DeltinScript / OSTW
language for Overwatch Workshop. It parses and type-checks DeltinScript
projects with no .NET runtime required, with partial compilation to Workshop
code.

## Features

- Recoverable parser that keeps comments and exact source locations.
- Multi-file projects and imports.
- Type checking: scoping, overloads, access control, classes, structs, enums,
  and interfaces.
- Symbol, reference, and type lookups for editors and tools.

## Status

| Capability | Status |
| --- | --- |
| Parsing, projects, type checking | ✅ Supported |
| DEL/OSTW → Workshop compilation | 🟡 Partial |
| Workshop → DEL/OSTW reconstruction | ⏳ Not yet |

Compiled output is intended to match the upstream OSTW compiler structurally.
Per-feature evidence is in [`docs/support-matrix.toml`](docs/support-matrix.toml).

Active DEL/OSTW feature development, including the remaining compilation and
reconstruction work, is paused and is not on the current WrightKit execution
lane; see the
[planning decision on #1](https://github.com/wrightkit/deltin-rs/issues/1#issuecomment-5606584680).

## Install

```sh
cargo install --path cli
```

Requires Rust 1.85 or newer.

## Usage

```sh
deltin-rs check <file-or-dir> [--json]
deltin-rs inspect <file> <line>:<col> [--json]
deltin-rs completion <bash|zsh|fish|powershell>
```

See [`docs/cli.md`](docs/cli.md) for the full command reference. Editors and
[Wright](https://github.com/wrightkit/wright) consume `deltin-rs` through its
Rust API.

## Documentation

See [`docs/README.md`](docs/README.md) for architecture, compatibility, and
known limitations.

## Contributing

Read [`AGENTS.md`](AGENTS.md) first.

## License

MIT. Compatibility fixtures keep their recorded upstream provenance and
licensing.
