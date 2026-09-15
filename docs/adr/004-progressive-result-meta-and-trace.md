# ADR 004: Progressive result meta, chain limits, and FetchTrace

**Status:** Accepted (design); **implementation** SEAMS-1…6 + IT-TAM  
**Date:** 2026-09-15  
**Related:** [002-serve-readiness-fsm.md](002-serve-readiness-fsm.md), [lsp-contract.md](../lsp-contract.md), [integ-seams-plan.md](../integration/tier-api-matrix/integ-seams-plan.md)

## Context

Integration (IT-TAM) and future UX need:

- Deterministic **chain limits** (`maxChainIter`, `maxTier`) for tier validation without env hacks alone.
- **Optional** structured metadata on **every** API response (tier, backend language/version, timing).
- **Correlated traces** via `traceId` + control **`FetchTrace`**, not only post-hoc WAL grep.

Today, tier appears on some `Location.data.tier` fields; timing lives in log extras for discover methods only.

## Decision

1. **Chain policy** on `ResolveQuery` (and LSP `progressiveLsp` request fields): optional `maxChainIter`, `maxTier`. Default: unlimited / full chain.

2. **Extended LSP results** when client sets `initializationOptions.progressiveLsp.emitResultMeta` (and optionally `emitTiming`): JSON-RPC `result` becomes `{ "value": <stock>, "progressiveMeta": { ... } }`. When off, wire shape unchanged.

3. **Control unary** responses gain optional `ProgressiveMeta` protobuf (additive fields). Same enable flag on requests.

4. **`FetchTrace(trace_id)`** returns recent `LogRecord` rows captured for that id (in-memory ring, bounded TTL/count).

5. **Unit tests** required for chain policy, wrap/unwrap, trace correlation (see integ-seams-plan §4).

## Consequences

**Positive:** IT-TAM asserts meta and timing in-band; validation seams are productized, not test-only env.

**Negative:** Progressive clients must handle wrapped results; snapshot tests need updating when meta enabled.

**Compatibility:** Stock LSP clients unaffected. Progressive clients opt in explicitly.

## References

- [integration/tier-api-matrix/integ-seams-plan.md](../integration/tier-api-matrix/integ-seams-plan.md)
