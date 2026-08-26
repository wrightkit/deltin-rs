# Provenance

This file records the pinned upstream references that define what `deltin-rs` means by
OSTW/DeltinScript compatibility, and the licensing guardrails for the compatibility corpus.

## Pinned references

Pinned on **2026-08-16** (UTC-05:00 local). Shallow clones (`--depth 1`) live under
`.upstream-refs/` (git-ignored).

| Reference | URL | Pinned commit | Date of commit | License |
|---|---|---|---|---|
| OSTW / DeltinScript implementation (repo: `ItsDeltin/Overwatch-Script-To-Workshop`) | https://github.com/ItsDeltin/Overwatch-Script-To-Workshop | `817c1db4bace52123f054ffe10d3d8a06052e687` | 2026-08-08 | MIT (see below) |
| OSTW wiki (documentation) | https://github.com/ItsDeltin/Overwatch-Script-To-Workshop.wiki | `e8894b972fae3fa9fd81dab0bb3672cc740a771e` | 2026-08-16 (clone head) | wiki content; see licensing note |

### License detail

The OSTW repository contains one license file:
`overwatch-script-to-workshop/LICENSE` — MIT (Copyright 2026 ItsDeltin). The full text is a
standard MIT grant ("Permission is hereby granted, free of charge..."). The `Deltinteger/`
compiler sources, the VS Code extension, the tests, the bundled `Modules/*.del`, and the
`Examples/` files are all covered by this repository license. SPDX identifier: **MIT**.

## Clone failures / gaps

The task brief suggested two canonical URLs. Both 404:

1. `https://github.com/ianlucas/ostw` — **not found**. The OSTW project moved; the canonical
   repository is `ItsDeltin/Overwatch-Script-To-Workshop` (former owner `ianlucas`, current
   `ItsDeltin`). Verified via `gh search repos` and the repo README ("Deltin's Script To
   Workshop").
2. `https://github.com/DeltinScript/DeltinScript` — **not found**. No GitHub org or repository
   named `DeltinScript` exists (verified via `gh search repos "DeltinScript"` and the GitHub
   API). **DeltinScript is the language implemented by the OSTW repository** (the `Deltinteger/`
   C# compiler); there is no separate reference implementation. All DeltinScript language
   evidence in this corpus therefore comes from `ItsDeltin/Overwatch-Script-To-Workshop`.

No other upstream clone failures. Both clones succeeded; commit SHAs above were recorded with
`git rev-parse HEAD` immediately after cloning.

## Licensing rules for the corpus

- All fixtures under `tests/corpus/` are **imported under the upstream MIT license** with
  attribution. Every `.del` fixture carries a header block:
  `// source: https://github.com/ItsDeltin/Overwatch-Script-To-Workshop/blob/<commit>/<path>`
  and `// license: MIT`, plus an `// expect:` line (see `docs/syntax-notes.md` / `docs/inventory.md`
  for the corpus conventions).
- `tests/corpus/projects/*/*.json` and `*.pathmap` have no comment syntax; their provenance is
  recorded in the per-project `.manifest.md` files.
- The `docs/` files themselves are original deltin-rs analysis and are not copied upstream content,
  but they quote small upstream examples for evidence. Quotes retain their `path@commit`
  references.
- Do not import additional upstream files into the corpus without updating this file and the
  fixture headers.

## Re-pinning

To re-pin (e.g. for a deliberate upstream bump):

```sh
git clone --depth 1 https://github.com/ItsDeltin/Overwatch-Script-To-Workshop .upstream-refs/ostw
git clone --depth 1 https://github.com/ItsDeltin/Overwatch-Script-To-Workshop.wiki .upstream-refs/ostw-wiki
git -C .upstream-refs/ostw rev-parse HEAD
git -C .upstream-refs/ostw-wiki rev-parse HEAD
```

Then update this file, every fixture header, and the `support-matrix.toml` evidence pointers.
A re-pin is a deliberate compatibility-contract decision owned by the architect/PM, not an
implementation detail.
