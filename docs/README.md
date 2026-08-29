# progressive-lsp documentation

This directory is the **source of truth** for the product. Implement from these files, not from chat transcripts or the archived sketch.

[initial-progressive-lsp-design.md](initial-progressive-lsp-design.md) is an archive of the first sketch. It is **not** current (ruff-as-Python-T3, no watches, no control plane, no allocator matrix).

## Start here

| If you need | Read |
|---|---|
| What this is and is not | [vision.md](vision.md) |
| Must / must-not / NFRs | [requirements.md](requirements.md) |
| Process and data flow | [architecture.md](architecture.md) |
| Types, traits, errors | [detailed-design.md](detailed-design.md) |
| Pattern map (mandatory) | [design-patterns.md](design-patterns.md) |
| Coverage, mutants, alloc matrix | [testing.md](testing.md) |
| Product exits M0–M6 | [milestones.md](milestones.md) |
| Work packages and sign-off | [implementation-plan.md](implementation-plan.md) |
| Stacked branches | [branching.md](branching.md) |

## Contracts and matrices

| If you need | Read |
|---|---|
| Vanilla LSP vs progressive client | [lsp-contract.md](lsp-contract.md) |
| Optional protobuf control API | [control-protocol.md](control-protocol.md) |
| LanguageFactory / scripts | [plugin-sdk.md](plugin-sdk.md) |
| LATEST+2 pins and lag | [language-matrix.md](language-matrix.md) |
| What may exist on the host | [host-deps.md](host-deps.md) |
| How an IDE consumes this | [consumer.md](consumer.md) |
| Per-language per-tier pass % | [conformance.md](conformance.md) |

## Agent rules

1. Do not start milestone `mN+1` until `mN` is signed off ([branching.md](branching.md), [implementation-plan.md](implementation-plan.md)).
2. Every new type maps to a named pattern in [design-patterns.md](design-patterns.md). Ad-hoc layers are a defect.
3. 95% line coverage and 80% mutation kill rate on listed crates from the first library that lands ([testing.md](testing.md)).
4. Shipped ELFs: no dynamic interpreter, no `DT_NEEDED` ([host-deps.md](host-deps.md)).
5. Stock LSP clients must work with stdio only. Do not put FilesSince on `$/` methods in v1 ([lsp-contract.md](lsp-contract.md)).
