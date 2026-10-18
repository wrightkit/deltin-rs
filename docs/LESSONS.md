# Lessons

Short review queue of reusable failure classes from merged deltin-rs work. Each entry names a prevention action and the evidence behind it. Promote an entry into a regression test, an owner contract, or an existing engineering standard, then remove it here. The procedure is the `wrightkit-retro` skill in `wrightkit/.agents`.

### Report the observed failure kind, not the assumed one

- **Rule:** When declared diagnostics distinguish failure kinds, classify by the observed error kind — never collapse every failure of an operation into its most common code. Pin each kind with a regression test at the reporting site.
- **Why:** `Loader::load_file` reported every `fs::read_to_string` failure as PJ002 "missing import target", including I/O failures where the path exists but cannot be read — making the machine-readable diagnostic unreliable for agents, editors, and the standalone CLI.
- **Prevention home:** regression tests per distinct failure kind (the import-site PJ003 span is pinned in #125).
- **Incidents:** [#125](https://github.com/wrightkit/deltin-rs/pull/125) (fixes #124) — observed 2026-10-09
