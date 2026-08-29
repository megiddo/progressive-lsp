# T2 bake-off results (PD4)

**Date:** 2026-08-29  
**Host:** Darwin aarch64 (not a musl ELF / allocator-matrix cell).  
**Default after this spike:** **heuristic**. Winner rule did not fire.

Pin (not a `third_party/` dump):

| Field | Value |
|---|---|
| URL | `https://github.com/github/stack-graphs.git` |
| SHA | `fcb7705d5b38ae13b3665a9b2c882e5a97243d44` (archive note, 2025-09-09) |
| Java TSG | `languages/tree-sitter-stack-graphs-java/src/stack-graphs.tsg` |
| Fetch | **ok** (git fetch-at-SHA) |

`t2.java = "heuristic"` remains the omit/default. `t2.java = "stack-graphs"` is opt-in. Slim binary omits the runtime (`--features t2-stack-graphs`).

## Winner rule (from the spike)

TSG becomes default only if it wins **held-out correctness** and **memory** (RSS not worse by more than 20%). Ties, fetch/runtime skip, or a large RSS/time tax → heuristic stays default.

## Table

| Fixture | Backend | Hit-rate | Wall | Peak RSS (Darwin `ps`) | Notes |
|---|---|---|---|---|---|
| A `fixtures/java-heuristic/` (20 cases written for heuristics) | heuristic | **20/20 (100%)** | 4.4 ms | ~4.5 MiB sample | ≥95% gate held |
| A | stack-graphs | stitch **skip_runtime** | — | — | TSG **source loaded** at pin SHA. `build_stack_graph_into` panics (`add_scope_node` unwrap) with workspace `tree-sitter-java` 0.23.5 vs pin `=0.23.4`. Not a stitch score. |
| B junit4 `@05fe2a64` (5 held-out defs: TestCase, Assert, fail, runBare, getName) | heuristic | **2/5 (40%)** | (same ingest) | (same process) | Not written for our heuristics |
| B | stack-graphs | stitch **skip_runtime** | — | — | Same panic. Do not treat fallback name-match as a TSG win. |

`--features t2-stack-graphs` was compiled and selected. The crate **ran** (LanguageConfiguration / `StackGraphLanguage::from_source` accepted the pinned TSG). Graph construction aborted per file. A source-loaded name-at-position fallback still answered some queries (A 20/20, B 4/5) after those panics; that path is **not** stack-graph stitching and is **not** used to flip the default. Wall time with the runtime feature on fixture A was ~10.9 s (panic cost), vs 4.4 ms heuristic.

## Decision

- **Correctness (stitch):** TSG column is `skip_runtime`, not a win.
- **Memory / time:** no fair RSS comparison; runtime path is far slower when it panics.
- **Default:** heuristic. TSG remains an opt-in Strategy (`[t2] java = "stack-graphs"`).
- Java v1 ceiling unchanged (no JVM). No `third_party/stack-graphs` vendor.

## How to re-run

```text
cargo test -p progressive-lsp-lang-java --features t2-stack-graphs --lib t2_bakeoff -- --nocapture
```

Cache: `target/pd4-bakeoff/` (gitignored).
