# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/wrightkit/del-rs/compare/v0.1.1...v0.1.2) - 2026-08-26

### Added

- *(release)* automate crates.io release ([#63](https://github.com/wrightkit/del-rs/pull/63))
- *(project)* adapt chase aliases by target semantics ([#59](https://github.com/wrightkit/del-rs/pull/59))
- *(project)* discover ds.toml entry points
- *(lowering)* bound global array foreach
- *(lowering)* bound scalar rule-local storage
- *(lowering)* materialize bounded switch runtime
- *(cli)* rebaseline and modernize command surface ([#48](https://github.com/wrightkit/del-rs/pull/48))
- *(lowering)* complete core HIR to Workshop WIR
- *(lowering)* add core HIR to Workshop WIR path ([#36](https://github.com/wrightkit/del-rs/pull/36))
- establish the workshop-rs catalog provider seam ([#34](https://github.com/wrightkit/del-rs/pull/34))
- reach Workshop-independent frontend completeness and expose tooling APIs ([#7](https://github.com/wrightkit/del-rs/pull/7))
- define typed DEL HIR and abstract runtime semantics ([#6](https://github.com/wrightkit/del-rs/pull/6))
- implement core DEL/OSTW semantic and type system ([#4](https://github.com/wrightkit/del-rs/pull/4))
- bootstrap DEL/OSTW source frontend and project model ([#3](https://github.com/wrightkit/del-rs/pull/3))

### Fixed

- keep CLI fixture paths Windows-safe
- preserve DEL project overlays and cross-file symbols
- complete owner-backed OSTW lowering
- complete owner-backed OSTW lowering
- bound scalar subroutine parameter storage
- *(semantic)* enforce canonical argument ordering
- *(lowering)* enforce safe switch evaluation
- re-register api module after rebase
- deterministic cross-file playervar resolution (HI006 flake)
- restore deterministic playervar resolution; drop api module from this stack

### Other

- release v0.1.1 ([#64](https://github.com/wrightkit/del-rs/pull/64))
- make del-rs the OSTW source owner
- retire frontend terminology in del-rs
- cut dead internal code surfaced by entropy audit ([#51](https://github.com/wrightkit/del-rs/pull/51))
- define del-rs as a standalone DEL/OSTW implementation ([#50](https://github.com/wrightkit/del-rs/pull/50))
- enforce evidence provenance boundaries ([#35](https://github.com/wrightkit/del-rs/pull/35))
- separate Rust toolchain and cache ownership ([#37](https://github.com/wrightkit/del-rs/pull/37))
- *(ci)* skip docs-only workflow runs ([#28](https://github.com/wrightkit/del-rs/pull/28))
- add evidence-driven DEL compatibility reporting ([#27](https://github.com/wrightkit/del-rs/pull/27))
- streamline README navigation ([#25](https://github.com/wrightkit/del-rs/pull/25))
- present compatibility to DeltinScript/OSTW developers in plain language ([#24](https://github.com/wrightkit/del-rs/pull/24))
- drop issue references from the README ([#22](https://github.com/wrightkit/del-rs/pull/22))
- *(deps)* bump toml from 0.8.23 to 1.1.4+spec-1.1.0 ([#20](https://github.com/wrightkit/del-rs/pull/20))
- *(deps)* bump actions/checkout from 4 to 7 ([#21](https://github.com/wrightkit/del-rs/pull/21))
- *(deps)* add Dependabot config ([#19](https://github.com/wrightkit/del-rs/pull/19))
- *(ci)* constrain Rust cache writes ([#18](https://github.com/wrightkit/del-rs/pull/18))
- rebaseline README and documentation around stable DEL/OSTW compatibility ([#17](https://github.com/wrightkit/del-rs/pull/17))
- establish OSTW/DeltinScript feature inventory and compatibility corpus ([#2](https://github.com/wrightkit/del-rs/pull/2))

## [0.1.1](https://github.com/wrightkit/del-rs/compare/v0.1.0...v0.1.1) - 2026-08-26

### Added

- *(release)* automate crates.io release ([#63](https://github.com/wrightkit/del-rs/pull/63))
- *(project)* adapt chase aliases by target semantics ([#59](https://github.com/wrightkit/del-rs/pull/59))
- *(project)* discover ds.toml entry points
- *(lowering)* bound global array foreach
- *(lowering)* bound scalar rule-local storage
- *(lowering)* materialize bounded switch runtime
- *(cli)* rebaseline and modernize command surface ([#48](https://github.com/wrightkit/del-rs/pull/48))
- *(lowering)* complete core HIR to Workshop WIR
- *(lowering)* add core HIR to Workshop WIR path ([#36](https://github.com/wrightkit/del-rs/pull/36))
- establish the workshop-rs catalog provider seam ([#34](https://github.com/wrightkit/del-rs/pull/34))
- reach Workshop-independent frontend completeness and expose tooling APIs ([#7](https://github.com/wrightkit/del-rs/pull/7))
- define typed DEL HIR and abstract runtime semantics ([#6](https://github.com/wrightkit/del-rs/pull/6))
- implement core DEL/OSTW semantic and type system ([#4](https://github.com/wrightkit/del-rs/pull/4))
- bootstrap DEL/OSTW source frontend and project model ([#3](https://github.com/wrightkit/del-rs/pull/3))

### Fixed

- keep CLI fixture paths Windows-safe
- preserve DEL project overlays and cross-file symbols
- complete owner-backed OSTW lowering
- complete owner-backed OSTW lowering
- bound scalar subroutine parameter storage
- *(semantic)* enforce canonical argument ordering
- *(lowering)* enforce safe switch evaluation
- re-register api module after rebase
- deterministic cross-file playervar resolution (HI006 flake)
- restore deterministic playervar resolution; drop api module from this stack

### Other

- make del-rs the OSTW source owner
- retire frontend terminology in del-rs
- cut dead internal code surfaced by entropy audit ([#51](https://github.com/wrightkit/del-rs/pull/51))
- define del-rs as a standalone DEL/OSTW implementation ([#50](https://github.com/wrightkit/del-rs/pull/50))
- enforce evidence provenance boundaries ([#35](https://github.com/wrightkit/del-rs/pull/35))
- separate Rust toolchain and cache ownership ([#37](https://github.com/wrightkit/del-rs/pull/37))
- *(ci)* skip docs-only workflow runs ([#28](https://github.com/wrightkit/del-rs/pull/28))
- add evidence-driven DEL compatibility reporting ([#27](https://github.com/wrightkit/del-rs/pull/27))
- streamline README navigation ([#25](https://github.com/wrightkit/del-rs/pull/25))
- present compatibility to DeltinScript/OSTW developers in plain language ([#24](https://github.com/wrightkit/del-rs/pull/24))
- drop issue references from the README ([#22](https://github.com/wrightkit/del-rs/pull/22))
- *(deps)* bump toml from 0.8.23 to 1.1.4+spec-1.1.0 ([#20](https://github.com/wrightkit/del-rs/pull/20))
- *(deps)* bump actions/checkout from 4 to 7 ([#21](https://github.com/wrightkit/del-rs/pull/21))
- *(deps)* add Dependabot config ([#19](https://github.com/wrightkit/del-rs/pull/19))
- *(ci)* constrain Rust cache writes ([#18](https://github.com/wrightkit/del-rs/pull/18))
- rebaseline README and documentation around stable DEL/OSTW compatibility ([#17](https://github.com/wrightkit/del-rs/pull/17))
- establish OSTW/DeltinScript feature inventory and compatibility corpus ([#2](https://github.com/wrightkit/del-rs/pull/2))
