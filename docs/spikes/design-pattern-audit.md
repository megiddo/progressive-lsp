# Spike: repository-wide design pattern audit

**Type:** Background agent spike (docs output; no required code changes).  
**Feeds:** TC-1 in [types-cache/implementation-checklist.md](../types-cache/implementation-checklist.md).  
**Output artifact:** [types-cache/pattern-audit-report.md](../types-cache/pattern-audit-report.md) (create on completion).

---

## Goal

For **every crate and significant module** in the workspace, determine:

1. Which types map to a named pattern in [design-patterns.md](../design-patterns.md).
2. Which code is **ad-hoc** (Manager/Helper/Util god modules, duplicate discover logic, timeout-driven “UX”).
3. What **refactor** would align with patterns (P0 = blocks TCACHE; P1 = during TC-4; P2 = defer).

This spike **does not** authorize refactoring `progressive-lsp-lang-*` boundaries without human sign-off.

---

## Agent payload (background spike)

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  docs/design-patterns.md
  docs/detailed-design.md
  docs/architecture.md
  docs/types-cache/design.md
  docs/types-cache/design-patterns-addendum.md
  docs/spikes/design-pattern-audit.md
ROLE: spike agent (design-pattern-audit)
OUTPUT: docs/types-cache/pattern-audit-report.md
DO_NOT: change Rust sources; do not commit unless human asked
METHOD: see steps below
```

---

## Method

### Step 1 — Inventory crates

List all workspace members from root `Cargo.toml` + `integration/harness` (non-member) + `xtask` + `poc-ide` + bins.

Table columns: `crate`, `role (1 line)`, `composition root? (Y/N)`.

### Step 2 — Module pass

For each crate, list `src/*.rs` and major submodules (`src/**/`). For each file:

| Column | Content |
|--------|---------|
| Path | e.g. `src/serve_host.rs` |
| Primary types | pub struct/enum/trait names |
| Pattern (if any) | From design-patterns.md |
| Ad-hoc flags | e.g. mixed concerns, duplicate URI logic, blocking engine on mux |
| Suggested pattern | Facade / Port / Chain step / Repository / etc. |
| Priority | P0 / P1 / P2 / OK |

### Step 3 — Cross-cutting flows

Trace these flows and document **every hop** (file + type):

1. Mux discover: `textDocument/definition|implementation|references`
2. `didOpen` → index + engine inbox + warm
3. Control: `IndexStatus`, `TierReady`, watch batch
4. POC IDE: `DiscoverCommand` → mux (no duplicate resolve)

Flag violations of [types-cache/requirements.md](../types-cache/requirements.md) REQ-NFR-4 and FR-1.4.

### Step 4 — Timing / test surface

List where timing is **not** asserted but should be (TC-5):

- `progressive-lsp-resolve` chain tests
- `src/session.rs` resolve tests
- `integration/harness/discover_container.rs`
- `poc-ide` lsp_io / mux

### Step 5 — Deliverable structure

Write `pattern-audit-report.md` with:

1. Executive summary (≤ 15 bullets).
2. Crate inventory table.
3. Full module table (may be long).
4. P0 refactor list (path, issue, target pattern, suggested WP TC-4.x).
5. design-patterns.md gaps (types in code without table row).
6. Optional mermaid for discover path **as-is** vs **target** (TC-3).

---

## Completion criteria

- [ ] Every workspace crate appears in the report.
- [ ] `serve_host`, `session`, `progressive-lsp-engine`, `progressive-lsp-resolve`, `poc-ide` have module-level rows.
- [ ] P0 list is non-empty or explicitly “none found” with rationale.
- [ ] Human can assign TC-4.1 from P0 without re-reading the repo.

---

## Placeholder report (remove when spike completes)

Spike not yet run. Replace this section in `pattern-audit-report.md` when TC-1.1 completes.
