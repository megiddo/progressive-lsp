# Integration tests (design)

System-level tests for a **shipped** progressive-lsp: static Linux binaries + optional engine packs, run against real project trees and real distro userspaces.

These are **not** the crate unit/mutation suite (`docs/testing.md` in the product repo). That suite forbids `sleep`, uses FakeClock, and never vendors clangd’s tests. This suite **does** use containers, wall time, and child engines. Keep the two harnesses separate so unit CI stays fast and hermetic.

## Suites

| ID | File | Proves |
|---|---|---|
| IT-1 | [01-deploy-config.md](01-deploy-config.md) | Drop a binary on Arch / RHEL-family / Debian / Ubuntu; prefix + `config.toml` just work; no host Node/JVM/CPython |
| IT-2 | [02-lsp-backends.md](02-lsp-backends.md) | Each language backend answers vanilla LSP on a **real** codebase |
| IT-3 | [03-extended-protocol.md](03-extended-protocol.md) | A **few** backends plus a progressive client: FilesSince, WatchBatch, config, tiers |

## When they run

- After `xtask dist` produces artifacts for the job’s `arch` × `libc` cell.
- Nightly / release, not every library PR (engine packs are too heavy).
- IT-1 can run on a musl core tarball **without** clangd/tsgo.
- IT-2 T3 rows skip (or mark `skip_pack_missing`) if that pack is not in the dist flavor.
- IT-3 requires control socket (`--control-socket`) and at least core + the packs under test.

## Harness sketch (for the implementer)

Layout in this directory (IT-1 on `pd1`; IT-2 on `pd2`; IT-3 stays empty until `pd3`):

```text
integration/
  README.md             # this file
  01-deploy-config.md
  02-lsp-backends.md
  03-extended-protocol.md
  compose.yaml          # Arch / Rocky / Debian / Ubuntu userspaces
  harness/              # plsp-it1 handshake + backend + run-it1.sh / run-it2.sh
  artifacts/            # CI drops the musl ELF here (not committed)
  corpora/              # pins.json + fetch-at-SHA (PD2); csharp-mini snippet
  expected/             # golden find / ghost siblings per corpus
```

**Darwin:** `harness/run-it1.sh auto` is host_smoke + a gap note when Docker or a musl ELF is missing. Do not treat that as IT-1.1. Linux CI bind-mounts a `check-static` musl ELF and runs the four distros.

**IT-2 Darwin:** `harness/run-it2.sh auto` runs stock stdio against native Mach-O on in-tree supplements + fetched corpora. T3 rows are `skip_pack_missing` when packs are stubs. That is **not** a typed hover green. Linux CI with real musl packs is the T3 gate.

**Client:** a tiny stdio LSP driver (initialize → didOpen → request → shutdown). Progressive tests add a Unix-socket protobuf client using `progressive-lsp-control`. Do not use Neovim as the only gate; a headless driver is reproducible.

**Clock:** integration tests may wait on `workDoneProgress` / `TierReady` with a **deadline** (e.g. 60s), not `sleep(5)` loops. Poll the protocol, not the wall.

**Static bar:** on every distro, `file` / `readelf -d` on the copied ELF must show no interpreter and no `DT_NEEDED` (same as `xtask check-static`).

## Pass / fail reporting

One JSON or JUnit file per suite. Rows: `distro`, `language`, `tier`, `pack_present`, `result`. CI dashboard in M6 can ingest this.

## Out of scope

- macOS/Windows **host** binaries (clients may be Darwin; the server under test is Linux in Docker).
- SSH, git UI, PTY (not this product).
- `$/` JSON-RPC FilesSince (forbidden in v1).
