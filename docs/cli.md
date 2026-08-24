# del-rs CLI contract

Status: **living reference** · Owner: `del-rs` CLI. This document records the
task-oriented command rebaseline and the CLI-local presentation boundary. The
library diagnostics, semantic APIs, HIR, and evidence schemas remain owned by
their existing modules and contracts.

## Command classification

The command model distinguishes user tasks from inspection and evidence work:

| Surface | Commands | Contract |
| --- | --- | --- |
| Stable user-facing | `check`, `inspect`, `support`, `completion` | Validate source, query semantic information, inspect declared support, or install static shell completion. |
| Developer/debug | `dev parse`, `dev hir` | Inspect parser and HIR stages for agent/developer workflows; these are not stable language UX promises. |
| Maintainer/evidence | `maintainer compatibility` | Run the corpus/evidence report used by maintainers and CI. |

The top-level `parse` and `hir` commands remain accepted as hidden compatibility
aliases for the `dev` commands. The top-level `matrix` command remains a hidden
compatibility alias for `support`, and `compatibility` remains a hidden alias
for `maintainer compatibility`. Hidden aliases preserve existing scripts while
keeping internal stages and evidence workflows out of the documented stable
command list.

The `dev hir --json` and `inspect --json` interfaces preserve the machine-readable
semantic capabilities used by #38. No command in this change compiles DEL/OSTW
to Workshop; that remains outside this CLI contract.

## Migration notes

| Existing invocation | Preferred invocation | Compatibility |
| --- | --- | --- |
| `del-rs parse FILE` | `del-rs dev parse FILE` | Existing top-level form remains accepted. |
| `del-rs hir PATH` | `del-rs dev hir PATH` | Existing top-level form remains accepted. |
| `del-rs matrix` | `del-rs support` | Existing top-level form remains accepted; JSON keeps `command: "matrix"`. |
| `del-rs compatibility` | `del-rs maintainer compatibility` | Existing top-level form remains accepted and keeps report schema 1. |

`check` and `inspect` keep their existing names. `inspect` is deliberately a
best-effort query: it propagates parser and semantic diagnostics in human output and in a
new `diagnostics` JSON field, but returns exit `0` when the query ran, even if
the source has errors. Missing input remains exit `4`; malformed position input
remains exit `2`. `LINE:COL` is one-based, uses the source file's Unicode scalar
columns, accepts the exact end-of-file cursor boundary, and rejects zero,
negative, or out-of-range line/column values before the parsing pipeline runs.
This preserves useful semantic queries for agent workflows without claiming that
an inspect result validates its source.

## Output and presentation

Data commands accept `--json`, which writes exactly one JSON document to stdout.
JSON mode never writes ANSI color, GitHub workflow commands, progress text, or
human summaries to stdout. The opt-in `DEL_DEBUG=1` semantic/HIR trace is
disabled for JSON pipeline commands so stderr remains empty unless the CLI
itself reports an I/O or output failure. Human/debug output keeps the existing
`DEL_DEBUG` behavior.

Human output accepts:

```text
--presentation auto|terminal|plain|github-actions
--color auto|always|never
```

`auto` selects GitHub Actions when `GITHUB_ACTIONS=true`, a terminal only when
stdout is a TTY outside generic `CI=true`, and plain output otherwise. An
explicit presentation value wins over environment detection. `NO_COLOR` makes
`--color auto` non-colored; `--color always` is an explicit override. GitHub
Actions presentation intentionally emits workflow annotations without ANSI.

When source information is available, GitHub Actions presentation emits one
`error`, `warning`, or `notice` annotation per diagnostic with escaped file,
line, column, and message fields. If `GITHUB_STEP_SUMMARY` is set, the concise
command summary is appended to that file. These behaviors are CLI-local; the
DEL library does not know about CI environments.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Successful task; `inspect` also uses this for a completed best-effort query with source diagnostics. |
| `1` | Source/evidence errors, an invalid support matrix, or unexpected compatibility regressions. |
| `2` | Usage error: unknown command/flag or malformed `LINE:COL`. |
| `3` | Internal CLI failure: unexpected panic or an output/summary serialization or write failure. |
| `4` | Input I/O failure, such as an unreadable source path. |

Exit `3` is now reachable through the CLI's panic boundary and fallible JSON,
stdout, stderr, and GitHub summary writes; it is not a source-diagnostic code.

## Static completion

Completion is generated from the same structured `clap` command model used for
parsing and help. It is static output, not a dynamic shell completion protocol:

```sh
del-rs completion bash > del-rs.bash
del-rs completion zsh > _del-rs
del-rs completion fish > del-rs.fish
del-rs completion powershell > del-rs.ps1
```

The generated scripts include the documented stable commands and the accepted
compatibility paths; they do not require a config file, progress subsystem, or
runtime provider.
