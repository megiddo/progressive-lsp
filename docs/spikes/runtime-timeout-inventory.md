# Spike: runtime timeout inventory

**Status:** Inventory (2026-09). Read-only audit from [Runtime timeout audit](6f4e600f-4657-488d-b338-c0576a7a4359).

**Scope:** Run-time waits, deadlines, and wall-clock caps in product code and dogfood harness. **Excluded:** xtask/build freshness, compile, Docker image build stamps, agent shell `block_until_ms`.

**Target model (serve):** [ADR 002](../adr/002-serve-readiness-fsm.md) — backend FSM + control plane; mux chain **`Ready` / `NotReady`** only. **POC IDE** mux deadlines are **UX-only** and out of scope for serve correctness (see REQ-NFR-1.4).

---

## Summary (mechanism counts)

| Subsystem | Mechanisms | UX-facing? |
|-----------|------------|------------|
| poc-ide | 9 | Mostly (mux 15s discover; 120s default RPC) |
| progressive-lsp serve + control | 4 | Mixed (80ms control poll; blocking serve loop) |
| progressive-lsp-protocol | 1 | Peer stall if server blocked |
| progressive-lsp-engine | 5 | Yes on sync `supervisor.resolve` / `lsp_child` |
| progressive-lsp-log | 4 | Internal (50ms batch, 5s SQLite busy) |
| progressive-lsp-watch | 1 | `DEFAULT_WINDOW_MS` 50 — **not wired** on live serve |
| progressive-lsp-types-cache | 2 | Worker `recv`; 20ms **tests only** |
| integration/harness | 11 | CI/dogfood only |

No `thread::sleep` in production Rust (hygiene test). Legacy **400 ms / 8 s** `EngineResolver` recv budgets are **removed from mux**; docs in [pattern-audit-report.md](../types-cache/pattern-audit-report.md) §3 still describe the old path historically.

---

## Architectural tension

1. **Serve mux chain** is designed for ≤20 ms steps and `NotReady` (T3′ → T2 → T1).
2. **poc-ide mux** still caps discover at **15s** (`DISCOVER_REQUEST_DEADLINE`) and maps expiry to `IdeError::lsp("timed out waiting for language server response")` — this is **not** tier status.
3. **Sync engine I/O** (`try_engine_discover`, `lsp_child::read_message`, background `supervisor.resolve`) is **unbounded blocking** on pack stdio — stuck javacs blocks a thread; `abort_language` exists but has **no callers**.
4. **Harness** uses second-scale deadlines and ghost **2.05s** polling — acceptable for CI, not a protocol model.
5. **Watch:** production serve uses **`poll_disk_watch`** (immediate batches + **80ms** control read timeout), not `WatchCoalescer` 50ms window.

---

## Gap list (product / dogfood)

| ID | Location | Current behavior | Target |
|----|----------|------------------|--------|
| G1 | `poc-ide/src/mux.rs` | 15s / 120s `wait_timeout` → user error | Non-blocking poll or indefinite wait + **NotReady** / in-flight discover state; no timeout string |
| G2 | `poc-ide` control path | Mux control uses same 120s cap | Status RPC returns **not ready** fields; no wall-clock fail |
| G3 | `src/session.rs` sync engine fallback | Blocking `supervisor.resolve` on empty mux | **Remove** from request path; FR-2.2 miss → `NotReady`; FR-6.1 optional push when T3′ ready |
| G4 | `progressive-lsp-engine/lsp_child.rs` | Unbounded `read_message` | Bounded **step** budget → `NotReady` on worker; never block mux |
| G5 | `supervisor.rs` `abort_language` | Unused | Wire only if needed for **process** health, not UX timeout |
| G6 | Docs | pattern audit §3 implies recv_timeout on mux | Add “post TC-3” errata pointing here |

Harness deadlines (`PLSP_DISCOVER_DEADLINE_MS`, etc.) remain **env/CLI** for CI; not part of serve contract.

---

## Detail by crate

### poc-ide

| Area | Lines (approx) | Notes |
|------|----------------|--------|
| `mux.rs` | 26–40, 162–186, 371–373 | Discover **15s**; default **120s** |
| `mux.rs` | 158–159, 466–509 | Control on mux channel 1 |
| `lsp.rs` | 468–503 | Stdio LSP: **no** timeout, can stall forever |
| `control.rs` | 258–316 | Unix control: blocking |
| IO threads | `lsp_io`, `runtime_io`, `tree_io` | `recv()` until disconnect |

### progressive-lsp

| Area | Lines (approx) | Notes |
|------|----------------|--------|
| `control_socket.rs` | 130–145 | **80ms** read/write timeout → `poll_disk_watch` |
| `lib.rs` + protocol | serve / serve_mux | Blocking framed reads |
| `serve_host.rs` | `poll_disk_watch` | No sleep; immediate watch batches |

### progressive-lsp-engine

| Area | Notes |
|------|--------|
| `lsp_child.rs` | Unbounded RPC; warm thread |
| `backoff.rs` / `supervisor.rs` | Respawn scheduling by wall ms (internal) |

### progressive-lsp-log

| Area | Notes |
|------|--------|
| `batch.rs` / `actor.rs` | **50ms** batch gate; **5s** SQLite busy; `recv_timeout` on mailbox |

### integration/harness

| Binary | Defaults |
|--------|----------|
| `plsp-it1` | 15s / 60s `--deadline-ms`; ghost **2050ms** |
| `discover-container` | init **600s**, discover **15s** |
| `progressive` IT-3 | 30s; control connect **400ms** spin |

---

## Env / CLI knobs (runtime)

| Knob | Where |
|------|--------|
| `--deadline-ms` | `plsp-it1` |
| `--init-deadline-ms` / `--discover-deadline-ms` | `discover-container` |
| (none) | poc-ide discover **15s** hard-coded |

---

## Next steps (human / milestone)

1. Adopt **REQ-NFR-1.4** (client): no wall-clock timeout as tier/discover outcome — see requirements patch on `fix/post-tcache-dogfood` or follow-on branch.
2. Implement **G1–G4** in priority order; revert or replace sync engine fallback (**G3**) with strict `NotReady` + IDE retry on cache ready.
3. Keep harness deadlines; document as CI-only in [integration/README.md](../../integration/README.md).
