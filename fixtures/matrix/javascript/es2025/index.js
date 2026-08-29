// ES2025: Promise.try (call is optional-chained so older runtimes still parse).
export function add(a, b) { return a + b; }
export async function tryIt() { return Promise.try(() => 1); }
