// TypeScript 5.9 (2026-08 3-release window LATEST).
export type Result<T> = { ok: true; value: T } | { ok: false };
export function id<T>(x: T): T { return x; }
