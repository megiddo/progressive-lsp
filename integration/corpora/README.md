# Corpora (PD2)

Pinned **git URL + peeled commit SHA** in `pins.json`. Fetch at CI/harness time into `.cache/` (gitignored). Do **not** submodule-mirror third-party histories.

```sh
integration/harness/target/debug/plsp-it1 fetch \
  --pins integration/corpora/pins.json \
  --cache integration/corpora/.cache
```

`csharp-mini/` is an imported public Microsoft SDK-style two-project `net8.0` snippet (not a git history dump). The fetch-at-SHA C# corpus is `adamralph/bullseye`. C# is a T1/T2 ceiling.

Java in-tree `fixtures/java-heuristic` is a supplement; `junit-team/junit4` is the external Maven proof.
