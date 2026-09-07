# POC tier completion plan

Close the gap between **local T1/T2 for every v1 language** and **T3 in a Linux container that is production Linux**. This file is the design plus the implementation order. Checklists live here and are copied onto [milestones.md](milestones.md). Work packages: [implementation-plan.md](implementation-plan.md). Agent payload: [poc-tier/agent-context.md](poc-tier/agent-context.md).

Related: [t2-heuristic-coverage.md](t2-heuristic-coverage.md), [t3-linux-hosts.md](t3-linux-hosts.md), [spike/java-t3.md](../spike/java-t3.md).

## Locks (do not reopen)

| Lock | Meaning |
|---|---|
| One serve | Native **or** container, never both. |
| Mac POC | Open Folder = Darwin T1/T2. Open Folder in Container = one Linux T1/T2/T3. |
| URIs | Identity bind-mount. Same `file:` URIs as native. No rewriter in core or poc-ide. |
| Static T3 | Fully static backends. **Exception:** aarch64 Java T3 may need host libc until Graal fully-static ARM. Prefer “static except libc.” |
| No JVM / JDT / Node / CPython / host `php` as our runtime | Unchanged. |
| C# | T1/T2 ceiling. No csharp-ls pack. |
| Java first | Live Java T3 before other remaining T3 pack work. |
| No `host8` | Stack `poc-uri` on current `main` (`host-cleanup` merged). Do not open `host8`. |
| Unit tests | Mocks only. No Docker daemon, registry, or AWS in `cargo test`. |

## Target (POC)

```text
Open Folder (Mac)              T1 all languages; T2 all v1 languages (heuristics)
Open Folder in Container       same T1/T2, plus T3 for every language except C#
Linux production               same T3 binaries as that container ISA
```

C# T3 never. clangd T3 is the last, hardest remaining pack (LLVM).

## Where we are

| Area | Now |
|---|---|
| T1 all languages | Landed (Tree-sitter in core). |
| T2 | Landed for Java, C#, JS, TS, PHP, Go, Zig. Missing C, C++, Rust, Python, CSS, HTML. |
| T3 wiring | Supervisor + packs named. Catalog offers T3 except C#. |
| T3 live in default container | Some engines already static-build; Java not live; C/C++ clangd cache miss; JS/TS/Go/Zig not in the default slim image. |
| URIs | Identity mount signed off (POC-URI). Native and container send the same `file:` URI via `file_uri` → core `path_to_file_uri`. No rewriter. |

## Stack (on current `main`; `host-cleanup` merged)

```text
main               # host-cleanup + Java T3 wiring / freshness (PR #6 / #7)
  └── poc-uri          # POC-URI: identity file: URIs (native == container)
        └── t2-coverage    # POC-T2: heuristic T2 for C/C++/Rust/Python/CSS/HTML
              └── java-t3      # POC-JAVA: live Java T3 both Linux ISAs
                    └── t3-image     # POC-IMG: image copies Java; aarch64 glibc userspace for javacs
                          └── t3-rest      # POC-REST: remaining T3 engines in the dogfood image
```

Do not start a branch until the parent milestone is signed off. Do not open `host8`. Do not stack `poc-uri` on the old `host-cleanup` ref (that ref lacks PR #7).

---

## POC-URI — identity `file:` URIs

**Why first.** If native and container speak different URIs, later T3 proofs are lying. The container fakes a Linux host with the files at the **same path**.

**In:** poc-ide `file_uri` / initialize `rootUri` / `didOpen` / discover; `DockerRunPlan` mount; tests that native and container plans use the same absolute workspace URI. Grep: no remap, no `/workspace` rewrite, no `file://` host rewrite.

**Out:** engine packs, T2 heuristics, image rebuild.

| ID | Work |
|---|---|
| URI.1 | Inventory: every path that builds or consumes `file:` URIs. Document they share `file_uri` / `path_to_file_uri`. |
| URI.2 | Tests: `DockerRunPlan` `-v WS:WS -w WS`; initialize `rootUri` equals `file_uri(WS)` for native and container attach. Fail closed if a rewriter type appears. |
| URI.3 | Fix any split found. Do not add a rewriter to “fix” it. |

**URI.1 inventory (producers / consumers share the core codec)**

| Site | Role | Codec |
|---|---|---|
| `progressive-lsp-core::path_to_file_uri` | produce | canonical encode |
| `progressive-lsp-core::path_from_file_uri` | consume | canonical decode |
| `FileId::from_uri` | consume | `path_from_file_uri` |
| `progressive-lsp-index` ingest | produce symbol `uri` | `path_to_file_uri` |
| `src/serve_host.rs` `root_from_params` / session didOpen | consume | `path_from_file_uri` |
| `poc-ide::file_uri` | produce initialize / didOpen / discover | wraps `path_to_file_uri` after absolute check |
| `poc-ide::path_from_file_uri` | consume jump locations | wraps core decode after `file:` check |
| `LspClient::initialize` (native + container) | produce `rootUri` | `file_uri(WS)` — same function both attach kinds |
| `DockerRunPlan` | mount identity | `-v $HOST_WS:$HOST_WS -w $HOST_WS` (not a URI type) |
| Integration harness | produce (IT only) | raw `file://` format — not the unit gate |
| Resolve TSG / stack-graph | produce opt-in T2 locations | `format!("file://{path}")` — not native vs container |

No `UriRewriter` / `UriMapper` in poc-ide or serve. URI.3 found no native vs container split: both attach kinds already called `LspClient::initialize` → `file_uri`. The defect was a **duplicate codec** in poc-ide; `file_uri` now wraps core. Identity bind-mount is the product. Do not add a rewriter.

**Exit**

- [x] Native Open Folder and Open Folder in Container send the same `file:` URI for the same absolute path.
- [x] No URI mapper type in poc-ide or serve.
- [x] Unit tests name `file_uri` / `DockerRunPlan`; no daemon.

---

## POC-T2 — heuristic T2 for every v1 language

Design: [t2-heuristic-coverage.md](t2-heuristic-coverage.md). Same `HeuristicResolver`; fill graph facts in each language indexer; factory T2 slot; catalog `has_t2`.

**In:** `progressive-lsp-lang-{c,cpp,rust,python,css,html}`, resolve graph index, poc-ide catalog/strip, matrix fixtures, conformance T2 cells.

**Out:** T3 packs, Docker, C# T3.

| ID | Work |
|---|---|
| T2-COV.1 | C and C++: `#include`, name/arity, containers. Factory T2. poc-ide `has_t2`. Fixtures. |
| T2-COV.2 | Rust and Python: `use`/`mod`/`fn`; `import`/`def`/`class`. TSG stays opt-in. |
| T2-COV.3 | CSS and HTML: selectors / `id`/`class`/`href`. Conformance T2 leaves `N/A`. |

**Exit**

- [ ] Every v1 `languageId` except `plaintext` has `has_t2 == true`.
- [ ] Strip T2 is not `n/a` for C, C++, Rust, Python, CSS, HTML after ingest.
- [ ] Definition/references fixtures per language (Java-heuristic class).
- [ ] Mac Open Folder: T1+T2 for those languages without a container.

---

## POC-JAVA — live Java T3 both Linux ISAs

Priority T3. Wiring (JAVA-T3.1 / JAVA-T3.3) is landed. This milestone produces **real backends**.

**x86_64:** fully static native-image (`check-static` must pass).  
**aarch64:** native-image allowed to need host libc (prefer static except libc). No JAR, no JDT, no `libjvm`, no `java` on PATH.

**In:** Graal dockerfile (both arches), pack extract, `check-static` exception **only** for aarch64 `javacs`, remove “aarch64 Java is a Miss / omit from image.”

**Out:** clangd, tsgo, changing URI rules, C# T3.

| ID | Work |
|---|---|
| JAVA-T3.2a | x86_64: live native-image extract; `check-static` pass. |
| JAVA-T3.2b | aarch64: live native-image extract; libc `DT_NEEDED` allowed; `libjvm` still fail closed. |
| JAVA-T3.2c | Pack/xtask: no `PackOutcome::Miss` for aarch64 java; dest required. Tests still `RecordingDockerPort` (no daemon). |

**Exit**

- [ ] Both dests exist after a live pack (orchestrator proof, not a cargo test).
- [ ] x86_64 `javacs` passes `check-static`.
- [ ] aarch64 `javacs` is native-image, not a JAR; may need libc; no `libjvm`.
- [ ] Missing dest still degrades to T2, never panic.

---

## POC-IMG — runtime image matches production

Copy live `javacs` into the Linux image used by Open Folder in Container. **aarch64 image** that includes Java T3 needs a glibc userspace (not empty scratch) for that one process. **x86_64** may stay scratch. Core and every other pack stay fully static.

**In:** `runtime.Dockerfile` / `RuntimeImagePlan`; `javacs` required on both triples; aarch64 base image choice; `./build lsp` copies Java.

**Out:** implementing clangd from scratch in PR CI; URI rewriter.

| ID | Work |
|---|---|
| IMG.1 | `javacs` required on both image triples (no optional omit). |
| IMG.2 | aarch64 runtime userspace supplies libc for `javacs`; document the base. x86_64 scratch OK. |
| IMG.3 | Unit tests: required copy / missing dest fail closed; aarch64 java not “JAVA-T3.2 omit.” Live `docker build` is orchestrator proof. |

**Exit**

- [ ] Open Folder in Container on ARM dogfoods ARM Java T3 (libc exception).
- [ ] Open Folder in Container on x86_64 dogfoods fully static Java T3.
- [ ] Identity mount unchanged.

---

## POC-REST — remaining T3 in the dogfood image

After Java works, put every other T3 we claim into the **POC dogfood image** (the one `./build run ide` + container open uses). Slim dist for “Java-only tarball” may remain a separate flavor; the POC image is not allowed to hide T3 behind “slim.”

| Language | Engine | Bar |
|---|---|---|
| Python | ty | already static both ISAs |
| PHP | phpantom | already static both ISAs |
| HTML | superhtml | already static both ISAs |
| CSS | biome | already static both ISAs |
| Rust | rust-analyzer | already static; document Linux sysroot in the container (project artifact) |
| JS / TS | tsgo | static Go pack; **include in dogfood image** |
| Go | gopls | static Go pack; **include**; project `go` still project artifact |
| Zig | zls | static Zig pack; **include**; project `zig` still project artifact |
| C / C++ | clangd | fully static both ISAs; cache-fill allowed; **no cmake in default PR** |
| C# | — | out |

| ID | Work |
|---|---|
| REST.1 | Dogfood image includes tsgo, gopls, zls (both ISAs). |
| REST.2 | clangd both ISAs or documented HOST-7-class miss with a close plan (cache-fill). |
| REST.3 | Rust: hover/progress still says so if no Linux sysroot; no Darwin sysroot pretend. |
| REST.4 | POC proof notes: Java tree → T3; one other slim language; one full-pack language. Not a cargo test. |

**Exit**

- [ ] Container T3 offered and spawnable for every v1 language except C#.
- [ ] clangd either present or an honest remaining miss (static still required; no `.so`).
- [ ] C# T3 still `not supported`.

---

## Master checklist (copy progress into milestones)

### Locks still true

- [x] One serve
- [x] No URI rewriter
- [ ] No JVM/JDT as Java T3
- [ ] C# T1/T2 only
- [ ] aarch64 Java libc is the only static exception

### POC-URI

- [x] URI.1 inventory
- [x] URI.2 tests
- [x] URI.3 fixes

### POC-T2

- [ ] T2-COV.1 C/C++
- [ ] T2-COV.2 Rust/Python
- [ ] T2-COV.3 CSS/HTML
- [ ] poc-ide strip/menus
- [ ] conformance T2 cells

### POC-JAVA

- [ ] JAVA-T3.2a x86_64 static
- [ ] JAVA-T3.2b aarch64 native-image + libc
- [ ] JAVA-T3.2c xtask no Miss omit

### POC-IMG

- [ ] IMG.1 javacs required both triples
- [ ] IMG.2 aarch64 glibc userspace
- [ ] IMG.3 tests + live image proof

### POC-REST

- [ ] REST.1 tsgo/gopls/zls in dogfood image
- [ ] REST.2 clangd or honest miss
- [ ] REST.3 rustc sysroot honesty
- [ ] REST.4 live POC notes

### Hygiene (every milestone)

- [ ] `cargo test` scoped; `--test-threads=1`; no `thread::sleep`
- [ ] Patterns named in [design-patterns.md](design-patterns.md)
- [ ] Docs agree (vision, requirements, matrix, host-deps, poc-ide architecture)
- [ ] `check-static` on new fully static dests; aarch64 `javacs` skipped by design
- [ ] Do not commit engine binaries
- [ ] Do not start the next branch from the implementer agent
