# IT-1 / IT-2 / IT-3 harness

Tiny stdio LSP driver:

- `plsp-it1 handshake` — initialize → shutdown (IT-1)
- `plsp-it1 backend` — stock definition / hover / tokens / didChange / ghost / no `$/` FilesSince (IT-2)
- `plsp-it1 fetch` — URL+SHA corpora into a gitignored cache (no submodule mirrors)
- `plsp-it1 progressive` — stdio LSP + Envelope control socket (IT-3). `--mux` is `pending_mux`.

`run-it1.sh` covers the four-distro compose matrix. `run-it2.sh` covers per-language stock LSP. `run-it3.sh` covers P-java / P-py / P-ts progressive cases.

This crate is **not** a workspace member. It does not enter the 95% llvm-cov denominator.

## Linux CI (real IT-1)

1. Publish a musl-static `progressive-lsp` (`xtask dist --libc musl`) that passes `check-static`.
2. Copy that ELF to `integration/artifacts/progressive-lsp` (or set `PLSP_ELF`).
3. `docker compose -f integration/compose.yaml pull`
4. `integration/harness/run-it1.sh matrix`

Images stay userspaces: no `nodejs`, `openjdk`, `python3`, or `php`. Network is off at run time; bind-mount the ELF.

## Darwin host

`run-it1.sh auto` builds the native Mach-O and runs **host_smoke** only. That is not IT-1.1. Docker + musl ELF are the distro gate — same class as the M0 musl gap.
