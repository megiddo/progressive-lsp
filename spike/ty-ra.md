# Spike: ty and rust-analyzer

Both are Rust engines and belong on the `rust-engine` allocator-matrix cells.

- **ty:** Python T3. Not CPython, pylsp, pyright, or ruff-as-types. Pin inside ty’s version window.
- **rust-analyzer:** Rust T3. rustc **sysroot** and proc-macro `.so` are **project** artifacts. No pack or no sysroot → T1 + say so (no dedicated Rust T2).

**Fail closed:** any `DT_NEEDED` / interpreter → do not ship that pack.

**M3 result:** supervisor + FakeEngineAdapter tests landed. `xtask dist --pack python,rust` writes dest/hash/manifest. Real musl ty/RA binaries remain Linux CI / Docker. Tests do not download LLVM.
