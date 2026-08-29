# Spike: T2 Strategy bake-off (heuristics vs stack-graphs vs others)

**Status:** follow-up, **not** done in M2.  
**Default until this spike ships a winner that beats default:** the **heuristic** T2 already in tree (`HeuristicResolver` / Java name-arity-import-hierarchy-scope, plus the other language T2 heuristics).

M2.3 recorded “heuristics win on the fixture.” That was **not** a head-to-head. Stack-graphs never ran. Heuristics scored ≥95% on `fixtures/java-heuristic/` (20 cases written for those heuristics). `StackGraphResolver` is a `NotReady` stub. This spike is the missing measurement **and** the plugin seam so a better T2 can replace heuristics without forking go-to-definition.

Related: [plugin-sdk.md](../plugin-sdk.md) (`LanguageFactory` / `Resolver` chain), [design-patterns.md](../design-patterns.md) (T2 **Strategy**), [language-matrix.md](../language-matrix.md). Integration corpora: [../../integration/02-lsp-backends.md](../../integration/02-lsp-backends.md).

## Why a seam, not a one-shot rewrite

T2 is “good enough navigation while T3 boots” (or the ceiling, for Java). Implementations will change: heuristics today, stack-graphs if it actually wins, oxc-style graphs for JS, something else later.

**Rule:** go-to-definition stays the `Resolver` chain. A T2 backend is a **compiled-in Strategy** behind that trait. Rhai still must not implement `textDocument/definition`. No `dlopen` in v1 (same as other plugins).

```text
Resolver chain (unchanged):  T3 if ready  →  T2 Strategy  →  T1 Tree-sitter
T2 Strategy (swappable):     heuristic (DEFAULT)  |  stack-graphs  |  other
```

Config (sketch; names can match `config.toml` when implemented):

```toml
# Per language; omit = heuristic.
[t2]
java = "heuristic"          # default
# java = "stack-graphs"
# javascript = "oxc"        # already the JS T2 path if present; still a Strategy pick
```

`LanguageFactory::resolver_chain` selects the Strategy. Tests inject a fake T2. Feature flags may omit unused backends from a slim binary (`--features t2-stack-graphs`).

**Invariant:** swapping T2 must not change T1 or T3. Same `ResolveQuery` / `ResolveResult`. `data.tier` stays `graph` for any T2 winner.

## What M2 did not measure

| Claim | Actual |
|---|---|
| Heuristics ≥95% | True on `java-heuristic` only |
| Heuristics beat TSG on correctness | **Not run** |
| Heuristics beat TSG on RSS / index time | **Not run** |
| Java TSG (shipped GitHub rules) covers that fixture | **Not run** |

Do not treat the language-matrix sentence as a benchmark until this spike’s report exists.

## How to measure (this is the spike)

### Pin upstream at build time

Do **not** copy stack-graphs into git as a mirror. Pin `git` URL + **commit SHA** (Cargo git dep or xtask fetch), same as engines. GitHub archived [stack-graphs](https://github.com/github/stack-graphs) (2025-09-09): pin that last SHA, or pin **our fork** if we must patch. Fork = our repo URL + SHA, still not a vendored dump unless the archive disappears.

Shipped TSG (historical): Java, JS, TS, Python. Not C/C++/C#/PHP/HTML/CSS — those stay heuristic or skip the TSG column.

### Same queries, two (or more) Strategies

1. **Fixture A — in-tree:** `fixtures/java-heuristic/` (and PHP/Go T2 fixtures if present). Record hit-rate per Strategy. Heuristics should stay ≥95% here; TSG must not *regress* this set if it is to replace the default.
2. **Fixture B — held-out Java:** a corpus **not** written for our heuristics (IT-2 junit4 pin or similar). Score definition/references/hover the same way as IT-2. This is the one that can show TSG ahead.
3. **Memory / time:** peak RSS and p99 for indexing that package + 100 F12s. FakeClock still for unit tests; this spike may use a deadline like other integration tests.

**Winner rule** (same as the original plan): keep TSG (or another backend) as **default** only if it wins on **correctness** (held-out hit-rate) **and** memory (RSS not worse by more than a documented bound, suggest 20%). Ties or “TSG needs a 50MB graph for a 5% hit-rate bump” → **heuristic stays default**; TSG remains an opt-in Strategy.

Document the table in this folder as `spikes/t2-bakeoff-results.md` when numbers exist. Empty until then.

### Other T2 candidates (same seam)

Not Java-only. Each is another Strategy impl, same pick rule:

| Language | Default today | Alternate to eval |
|---|---|---|
| Java | heuristic | stack-graphs Java TSG |
| JS/TS | oxc if shipped, else T1 | stack-graphs JS/TS TSG |
| Python | T1 (+ ty T3) | stack-graphs Python TSG |
| PHP / Go / Zig | in-tree heuristics | no upstream TSG; skip or a new Strategy later |

## Ship bar

- [ ] `T2Strategy` (or existing `Resolver` Strategy) selectable per language from config; default `heuristic`.
- [ ] `StackGraphResolver` actually loads pinned TSG for at least Java (not `NotReady` stub) when `t2.java = "stack-graphs"`.
- [ ] Bake-off table: fixture A, fixture B, RSS, wall time — heuristics vs TSG.
- [ ] Default in `config.toml` / docs unchanged unless the winner rule fires.
- [ ] Unit tests: fake T2 Strategy; scripts still cannot register definition.
- [ ] No `third_party/` dump unless the pin URL dies.

## Out of scope

- Replacing T3 engines.
- Rhai as a resolver.
- Changing Java’s v1 ceiling (still no JVM).
