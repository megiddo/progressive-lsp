# Spike: csharp-ls Native AOT + musl

**Question:** Can csharp-ls publish `linux-musl-x64` / `linux-musl-arm64` Native AOT artifacts that pass `xtask check-static`?

**Fail closed**

- Dynamic interpreter or any `DT_NEEDED` → do not ship. Matrix: C# **T1/T2 ceiling** in v1.
- glibc-static csharp-ls has historically been crashy. Treat glibc-static as a POC gate: segfault → flavor out; musl AOT remains the only supported static path if it works.

**What to try (later, not on M0 host Darwin)**

1. Publish/self-contained AOT for `linux-musl-*` in a pinned container.
2. Run `xtask check-static` on the ELF.
3. Load a tiny `.csproj` fixture without assuming a host `dotnet` at runtime (AOT binary is the engine).

**M0 result:** notes only. No ELF produced on this Darwin workstation. CI Linux must re-run the spike before M4.2.

**M4.2 decision:** treat the Darwin spike as **failed-closed for v1**. Language matrix is a **T1/T2 ceiling**. `lang-csharp` ships Tree-sitter T1 + heuristic T2. Census does not select `csharp-ls`. No host `dotnet` as our runtime. Re-open T3 only if Linux CI produces a `check-static` clean musl AOT ELF.
