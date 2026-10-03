# deltin-rs implementation role

The current repository role and ownership contracts are maintained in
[`docs/architecture/`](architecture/README.md).

This path is retained as a compatibility pointer for older links. Do not add
mutable support state or new architecture decisions here. Use:

- [`language-core.md`](architecture/language-core.md) for DEL/OSTW scope,
  project/semantic ownership, and typed implementation rules;
- [`workshop-boundary.md`](architecture/workshop-boundary.md) for runtime/lowering
  ownership and the canonical Workshop boundary;
- [`support-matrix.toml`](support-matrix.toml), [`compatibility.md`](compatibility.md),
  and executable verification for current support reality.
