# IT-1 harness

Tiny stdio LSP driver (`plsp-it1 handshake`) plus `run-it1.sh` for the four-distro compose matrix.

This crate is **not** a workspace member. It does not enter the 95% llvm-cov denominator.

## Linux CI (real IT-1)

1. Publish a musl-static `progressive-lsp` (`xtask dist --libc musl`) that passes `check-static`.
2. Copy that ELF to `integration/artifacts/progressive-lsp` (or set `PLSP_ELF`).
3. `docker compose -f integration/compose.yaml pull`
4. `integration/harness/run-it1.sh matrix`

Images stay userspaces: no `nodejs`, `openjdk`, `python3`, or `php`. Network is off at run time; bind-mount the ELF.

## Darwin host

`run-it1.sh auto` builds the native Mach-O and runs **host_smoke** only. That is not IT-1.1. Docker + musl ELF are the distro gate — same class as the M0 musl gap.
