# Types cache — design patterns addendum

Rows to merge into [design-patterns.md](../design-patterns.md) when the corresponding types land. Do not implement types without a row here and in the main table.

| Type | Pattern | Responsibility |
|------|---------|----------------|
| `TypesCacheStore` | Repository + Facade | In-memory query cache + relation graph; generation-aware get/put/invalidate |
| `TypesCacheKey` | Value object | `(QueryKind, FileId, Position, CacheGeneration)` identity |
| `TypesCacheEntry` | Value object | Locations + metadata (`filled_at`, `source`, `engine_generation`) |
| `CacheGeneration` | Value object | Per-file or per-package monotonic generation; tied to index dirty state |
| `TypesCacheResolver` | Chain of Responsibility step | T3′ read path; ≤ 20 ms; never calls engine |
| `TypesCacheBuilder` | Command + background worker | Invokes T3 engine; single-flight; writes store |
| `BuilderQueue` | Command queue | Coalesce miss notifications; priority from `PriorityIndex` |
| `InvalidationPolicy` | Strategy | Maps didChange/watch/engine restart → store invalidation |
| `TypesCachePort` | Port (optional) | Lets tests swap store without session god object |
| `FakeTypesCacheStore` | Test double | Same port as prod |
| `RecordingBuilder` | Test double | Captures enqueued keys without spawning engine |

**Anti-patterns (reject in review):**

- `EngineResolver` on mux chain after TC-3 cutover.
- Request-path `recv_timeout` > liveness budget as “UX”.
- T1/T2 labeled as “the cache” in docs or logs (they are **fallback** tiers).
- Discover logic duplicated in `serve_host`, `poc-ide`, and engine.
