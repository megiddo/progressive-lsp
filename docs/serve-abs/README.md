# SERVE-ABS — serve abstraction program

**Status:** ABS-0 signed off on `main`; ABS-1+ on stack branches.  
**Goal:** Deterministic serve-side readiness (ADR 002), unified file events (ADR 003), and an N-tier capability model without mux blocking or timeout-as-semantics.

## Document map

| Doc | Role |
|-----|------|
| [requirements.md](requirements.md) | Normative FR/NFR for ABS-1 … ABS-4 (extends TCACHE FR-6/7) |
| [design.md](design.md) | FSM, `FileEventHub`, `TierRegistry`, crates, proto sketch |
| [implementation-checklist.md](implementation-checklist.md) | Phased WPs, integration tests, phase exits |
| [agent-context.md](agent-context.md) | Meta / phase orchestrator payloads |

## Pointers (keep in sync on sign-off)

| Topic | Location |
|-------|----------|
| Master plan (phases, branch stack) | [architecture/abstraction-targets-plan.md](../architecture/abstraction-targets-plan.md) |
| N-tier conceptual model | [architecture/n-tier-framework.md](../architecture/n-tier-framework.md) |
| ADRs | [002](../adr/002-serve-readiness-fsm.md), [003](../adr/003-file-event-hub.md) |
| TCACHE chain / T3′ | [types-cache/requirements.md](../types-cache/requirements.md), [001](../adr/001-types-cache-overlay.md) |
| Public wire catalog | [progressive-lsp-apis.md](../progressive-lsp-apis.md) |
| Timeout inventory (non-normative) | [spikes/runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md) |
| Milestones + sign-off | [milestones.md](../milestones.md) SERVE-ABS section |
| Program slot in roadmap | [implementation-plan.md](../implementation-plan.md) SERVE-ABS section |

## Branch stack

```text
main
  └── serve-abs-1   # ABS-1 readiness FSM
        └── serve-abs-2   # ABS-2 FileEventHub
              └── serve-abs-3   # ABS-3 tier registry + capabilities
                    └── serve-abs-4   # ABS-4 TierPort (+ human gate)
```
