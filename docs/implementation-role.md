# del-rs implementation role

`del-rs` is an independently usable Rust implementation of the
DeltinScript/OSTW language surface. Its durable product boundary includes
parsing, project loading, semantic/type analysis, typed HIR, diagnostics,
tooling, and compiler integration, in addition to any LPP provider process.

## Durable model

```text
DEL / OSTW source
  ↓
del-rs parsing / project loading / semantic analysis
  ↓
DEL semantic model / typed HIR
  ↓
del-rs runtime + compiler lowering
  ↓
workshop-rs canonical WIR / validation / emission
  ↓
Workshop text
```

For the reverse direction:

```text
Workshop text
  ↓
workshop-rs parser / canonical WIR
  ↓
del-rs reconstruction
  ↓
DEL / OSTW source
```

`del-rs` therefore owns the language-specific semantics on both sides of the
Workshop boundary. It deliberately reuses `workshop-rs` instead of becoming a
second raw Workshop implementation.

## Provider

A provider is an integration role through which an implementation can expose
language intelligence to a tooling client such as Wright. LPP is one possible
process boundary. Provider support must not make standalone users depend on
Wright.

### Wright

Wright is a downstream integration/tooling product. It combines `del-rs`,
`opy-rs`, and `workshop-rs` with additional cross-language capabilities such as
lint, analysis, validated source edits, agent tooling, CI/embedding, and
language services.

## Ownership

`del-rs` owns DEL/OSTW syntax/project behavior, semantic/type rules, runtime
semantics, language-specific lowering, diagnostics/provenance, standalone
tooling, compatibility evidence, and Workshop→DEL reconstruction.

`workshop-rs` owns raw Workshop parsing, canonical Workshop identities and
semantics, WIR, validation, settings/localization, and emission.

The dependency direction is `del-rs → workshop-rs`; `workshop-rs` does not
depend back on DEL semantics.

## Current reality

The repository already has substantial standalone parsing, project, semantic,
typed-HIR, and inspection capability. DEL/OSTW → Workshop compilation is only
partially implemented, especially for advanced runtime and project/compiler
surfaces, and Workshop → DEL reconstruction is not yet implemented.

Those are implementation-completeness gaps. They do not change the repository's
durable role as the independently usable DEL/OSTW implementation.

Support claims remain governed by the support matrix and executable evidence.
