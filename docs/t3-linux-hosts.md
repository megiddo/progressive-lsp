# T3 on Linux (container = production)

The POC **Open Folder in Container** path is not a second product. It is a Linux `progressive-lsp serve` on this machine. Anything that cannot run there will not run on a real Linux host of that architecture either.

Related: [host-deps.md](host-deps.md), [language-matrix.md](language-matrix.md), [spike/java-t3.md](../spike/java-t3.md), [poc-ide/architecture.md](poc-ide/architecture.md), [poc-tier-plan.md](poc-tier-plan.md).

## Modes

| Client | Intelligence host | Tiers |
|---|---|---|
| POC Open Folder on Mac | Darwin `progressive-lsp` | T1 + T2 only |
| POC Open Folder in Container | one Linux container `serve` | T1 + T2 + T3 |
| Native Linux / production | Linux `progressive-lsp` | T1 + T2 + T3 |

Never two serves. Native **or** container.

We ship T3 backends for **both** Linux architectures we claim in dist (`x86_64` and `aarch64`). The container ISA is whichever Linux we are pretending to be.

## Paths (same `file:` URIs)

Bind-mount the workspace at the **same path** inside the container. The editor and the server speak the same `file:` URIs as native open.

There is **no** URI rewriter in progressive-lsp or in poc-ide. A special container URI scheme, a `/workspace` remap, or Mac-path vs Linux-path translation in the POC is a defect.

A true remote IDE that syncs files to a *different* Linux path owns that mapping in **the consumer**, not here. poc-ide is not that consumer; it fakes Linux with an identity mount.

## Static T3 by language

Default bar: fully static (no interpreter, no shared libraries). “Works” may still need a pack build.

| Language | x86_64 Linux | aarch64 Linux |
|---|---|---|
| Python, PHP, HTML, CSS, JS, TS, Go, Zig, Rust | **Works** (may require a build) | **Works** (may require a build). Rust types still need a Linux rustc sysroot on that host. |
| C, C++ (clangd) | **Requires investigation and implementation** | **Requires investigation and implementation** |
| Java | **Requires implementation** — fully static | **Exception** — native-image that still needs the host C library, until Graal fully-static ARM exists. No JVM / JDT. |
| C# | **This just can’t work** (Native AOT failed-closed; T1/T2 ceiling) | same |

## Java aarch64 exception

Graal on ARM can emit a native Java T3. It cannot emit a **fully static** one. Until Oracle ships that, aarch64 Java T3 is the best native-image we can get (prefer “everything static except libc”).

That is still **not** a JVM. `java` on PATH, a JAR, JDT-LS, and `libjvm` stay forbidden.

**What changes for the user / operator (not the LSP protocol):**

- The binary needs a matching C library on that Linux (glibc version / distro). Fully static x86_64 Java T3 does not.
- Mismatch looks like: engine will not start, or crashes, on a distro newer/older than the one we built against. Other T3 engines should not care.
- The Linux host (container or production) for **aarch64** cannot be an empty scratch userspace if Java T3 is present; it needs a glibc base (or equivalent) for that one process. x86_64 can stay scratch. Core and every other pack stay fully static.

**What does not change:** resolver chain, census, poc-ide URI identity, one serve, C# ceiling. When Graal fully-static ARM lands, drop this exception and treat that binary like the others.

C and C++ do **not** get this exception. clangd is supposed to be fully static on both architectures.

## POC implication

Dogfood the ISA you care about. ARM container = ARM production, including this Java libc dependency. x86_64 container = fully static Java T3. Do not invent a URI or engine adapter that only the POC understands.
