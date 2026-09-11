# Spike: Java T3 via static compilation

**Question:** Can we ship a Java types engine as a musl-static ELF that passes `xtask check-static`, with **no JVM / JDT-LS / libjvm at runtime**?

**Status:** wiring landed (JAVA-T3.1 / JAVA-T3.3). **JAVA-T3.2:** x86_64 Linux is fully static. **aarch64:** exception — ship native-image Java T3 that may need the host C library until Graal fully-static ARM exists. Still not a JVM / JDT. See [docs/t3-linux-hosts.md](../docs/t3-linux-hosts.md). C# remains T1/T2.

Related: [docs/language-matrix.md](../docs/language-matrix.md), [docs/host-deps.md](../docs/host-deps.md), [docs/requirements.md](../docs/requirements.md), [csharp-ls.md](csharp-ls.md).

## Why this is not JDT-LS

The old lock was “no Java T3” because the only production-quality Java LS is Eclipse JDT.LS, which is a JVM Equinox product. That lock is **runtime**, not “Java cannot have types.”

Allowed: a **build-time** JDK/Graal inside the pack Docker image (same class as Go/Zig toolchains). Forbidden: `java` on PATH as our T3, a fat JAR we exec, `libjvm`. **x86_64:** no `DT_NEEDED`. **aarch64 exception:** host libc is allowed until Graal fully-static ARM exists.

## Engine choice (ordered)

1. **Primary — GraalVM native-image of a javac-based LSP**
   - Upstream: [georgewfraser/java-language-server](https://github.com/georgewfraser/java-language-server) (Java Compiler API, not JDT). Pin git SHA in `xtask/pack-pins.toml`.
   - Pack build container: GraalVM + musl toolchain + zlib; `native-image --static --libc=musl` (Graal documents fully static musl ELFs that run on `scratch`).
   - Stdio JSON-RPC. Core `EngineAdapter` / `EngineSupervisor` already speak that.
   - New `PackKind` (e.g. `Graal` / `NativeImage`) + `docker/engine-pack-graal.Dockerfile`. Do not overload the Rust/Go/Zig dockerfiles.
2. **Fallback A — thinner javac wrapper** if the full LS cannot native-image (reachability / reflection). Same bar: static musl ELF, stdio LSP, `check-static`.
3. **Fallback B — fail closed and document the miss** (csharp-ls pattern). **Do not** ship a JVM. **Do not** silently leave the product saying Java has no T3 if an ELF can be produced.

Do **not** native-image Eclipse JDT.LS / vscode-java. Do **not** reimplement javac in Rust.

## Pack layout

| Item | Value |
|---|---|
| Pack name | `java` |
| Flavor | **slim** (Java workspaces must not pull clangd) |
| Dest | `target/musl/<triple>/engines/java/<binary>` |
| Census | `pom.xml` / Gradle / Eclipse `.classpath` (`java_markers`) → `java` |
| Darwin `xtask dist` | stub bytes + `DARWIN_CI_GAP.txt` (same as other packs) |

`progressive-lsp-engine` `slim_pack_names` / `binary_name_for_pack` must name this pack. Missing pack → T2 heuristics, never panic.

## Core / IDE wiring (same as Python `ty`)

- `JavaLanguageFactory`: prepend `EngineResolver` when `EngineSupervisor` is ready for `(java, package)`; T2 heuristic (or opt-in TSG) remains; T1 Tree-sitter last.
- `CensusSelector` selects `java` for `java_markers`.
- poc-ide `LanguageCatalog`: Java uses `T2_THEN_T3` (not `T1_T2_CEILING`). C# stays `T1_T2_CEILING`.
- Strip / Discover / `LaunchJournal`: Java T3 is offered in container / native Linux; `t3_not_supported` is **C# only**.
- Tests: mocks only (`FakeEngineAdapter`). No Docker daemon. `check-static` only on a real extracted ELF.

## Proof

1. `xtask pack` extracts a musl ELF for at least the host Linux ISA (Darwin: Docker).
2. `xtask check-static` on that file: no `PT_INTERP`, no `DT_NEEDED`, no `libjvm`.
3. Container folder open on a Java tree: T3 cell is not `not supported`; typed discover is `needs T3` until the engine is ready, then enabled.
4. Conformance T3 for Java stays **0% on Darwin stubs**; Linux CI with the real pack is the place to re-score.

### JAVA-T3.2 (x86_64 static; aarch64 libc exception)

**x86_64 Linux:** fully static native-image. Same binary in production and in an x86_64 container.

**aarch64 Linux:** ship native-image Java T3 that may `DT_NEEDED` libc. Prefer mostly-static (libc only). Not a JVM, JAR, JDT, or `libjvm`. The Linux host needs a matching glibc userspace. Drop the exception when Graal fully-static ARM exists.

**x86_64:** dockerfile pins `ghcr.io/graalvm/native-image-community:25.0.0-muslib-ol9` (tag order is `$version[-muslib][-$platform]`). That image exists on amd64 and can run `native-image --static --libc=musl`. A live extract of [georgewfraser/java-language-server](https://github.com/georgewfraser/java-language-server) `@ 58daaa29a0e2fe22764283607da6801cf8b493b9` (Java 25 + `jdk.compiler` internals) may still fail reachability; Fallback A (thinner javac wrapper) is not in tree yet. Crate tests inject `RecordingDockerPort` only — no Docker daemon, registry, or AWS.

What landed so a later x86_64 ELF drops in:

| Item | Location |
|---|---|
| Pack name / binary | `java` / `javacs` (slim) |
| Pin | `xtask/pack-pins.toml` kind `graal` |
| Dockerfile | `docker/engine-pack-graal.Dockerfile` (`ARG GRAAL_TAG` / `NATIVE_IMAGE_FLAGS`: x86_64 muslib + `--static --libc=musl`, aarch64 `25.0.0-ol9` + `-H:+StaticExecutableWithDynamicLibC`; `org.javacs.Main`; no jlink / JDT) |
| Dest | `target/musl/<triple>/engines/java/javacs` (aarch64 dest is a native-image that may need libc; not a Miss) |
| Darwin `xtask dist` | stub bytes + `DARWIN_CI_GAP.txt` |

**Do not** treat Darwin stubs or `RecordingDockerPort` fixture bytes as a `check-static` green. **Do not** ship a JAR, jlink image, `libjvm`, or host `java`. When a real x86_64 ELF exists, run `xtask check-static` on that dest file only.

**Live proof (POC-JAVA, Darwin arm64 + Docker):** `cargo xtask pack --pack java --target x86_64-unknown-linux-musl` → `check-static` pass on `target/musl/x86_64-unknown-linux-musl/engines/java/javacs`. Same for `aarch64-unknown-linux-musl` → native-image with `libc.so.6` + `ld-linux-aarch64.so.1` only (`check_native_image_libc`, not CLI `check-static`). Upstream pin `@ 58daaa29a0e2fe22764283607da6801cf8b493b9`. Dests gitignored.

## Fail closed

Dynamic interpreter, host `java`, JDT-LS, or `libjvm` → do not ship. aarch64 Java T3 may need host libc until Graal fully-static ARM; x86_64 and every other pack still fail closed on `DT_NEEDED`. Do not cmake LLVM. Do not reopen C# T3.
