# POC-REST live proof notes (orchestrator)

Not a `cargo test`. Records what was run on the dogfood image path after POC-JAVA / POC-IMG.

## REST.4 scenarios

| Scenario | Bar | Proof |
|---|---|---|
| Java tree → T3 | `javacs` in image; aarch64 glibc base | POC-JAVA live pack + POC-IMG `progressive-lsp-runtime:local` (see [../../spike/java-t3.md](../../spike/java-t3.md)) |
| One other slim language | static slim ELF in image | `python`/`ty` staged with image (same as other slim packs) |
| One full-pack language | tsgo / gopls / zls required dest | `xtask pack` extract both triples when Docker available; `RuntimeImagePlan` fail-closed if missing |

## REST.2 clangd

**Honest miss:** no prebuilt `clangd` in `target/pack-cache/` on this orchestrator session → runtime-image **omits** clangd with `HOST-7 miss` (not cmake in default PR). Cache-fill remains a separate HOST-7 orchestrator job.

## REST.3 Rust sysroot

Container ships **rust-analyzer** only. A Darwin rustc sysroot does **not** satisfy RA in Linux container. Tier strip uses `rust_degrade_reason` (`no rustc sysroot; T2 then T1`) — see `progressive-lsp-lang-rust`. Project `sysroot/` under the opened workspace is still required for Rust T3.

## Commands (reference)

```sh
# Dogfood packs (both triples when proving REST.1)
cargo xtask pack --pack gopls --pack tsgo --pack zls --target aarch64-unknown-linux-musl
cargo xtask pack --pack gopls --pack tsgo --pack zls --target x86_64-unknown-linux-musl

# Image (after core + slim + dogfood full dests exist)
cargo xtask runtime-image --both
```

Dest paths stay gitignored under `target/musl/<triple>/engines/`.
