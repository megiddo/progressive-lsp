# IT-3 — Extended protocol (`progressive.v1`)

**Goal:** A **progressive** client (LSP stdio **plus** protobuf control) works on a few real backends. This is not a second copy of every IT-2 row.

Stock LSP continues to work on the same server process. Control never replaces `textDocument/definition`.

## Backends under test (few)

Three is enough. They hit different control-plane interactions.

| ID | Language / pack | Corpus | Why this one |
|---|---|---|---|
| P-java | Java, **no** T3 pack | Maven multi-package (junit4 pin or `fixtures/java-multi`) | Ingest + `TierReady` without engines; proves control is not “the clangd API” |
| P-py | Python + **ty** | Flask pin (same as IT-2) | T3 handoff: `syntax` → `types`; `InstallPacks` / engines dir |
| P-ts | TypeScript + **tsgo** (oxc T2) | zod or preact pin | Busy tree: WatchBatch + FilesSince overflow/catch-up |

Optional fourth if time: **P-rust** (rust-analyzer) for `on_engine_spawn` skip via script — only if M3+ scripts are shipped.

## Process shape

```text
progressive-lsp serve --prefix $P --control-socket $P/run/control.sock
```

- Stdio: vanilla LSP (same driver as IT-2).
- Unix socket: protobuf frames (`u32be` length + payload). Max 16 MiB.
- After `initialize`, read `capabilities.experimental.progressiveLsp`:
  - `version` = `v1`
  - `socket` = the path passed (absolute)
  - `mux` = `false`

**IT-3.mux (one extra case, Java only):** `--mux` instead of a socket. LSP JSON-RPC on the `lsp` channel; control protobuf on `control`. Same RPC assertions as P-java. Skip if mux framing is not implemented yet; mark `pending_mux` rather than silently testing socket twice.

Default `serve` **without** `--control-socket` must still pass IT-2 (control off). IT-3 does not weaken that.

## Wire dispatch

The harness uses the **Envelope** in [`docs/user/progressive-v1-api.md`](../docs/user/progressive-v1-api.md): after the `u32be` length prefix, protobuf `{ method, request_id, body }`. `method` matches the RPC table (`GetConfig`, `WatchBatch`, …). Replies echo `request_id`; pushes use `request_id == 0`. Integration tests are the first consumer of that envelope; if implementation and the API doc disagree, **fail the test** and fix impl or doc.

`Status.code == 0` is success. Non-zero: do not apply InstallPacks side effects.

## Cases (all three backends except where noted)

### IT-3.1 — Discovery and GetConfig

1. LSP initialize; connect control socket.
2. `GetConfig` → toml matches merge chain (prefix `config.toml` + optional workspace overlay).
3. LSP `textDocument/definition` still works on the entry file (control did not steal stdio).

**Pass:** socket advertised; GetConfig `status` ok; F12 still works.

### IT-3.2 — SetConfig / ReloadConfig

1. `SetConfig` with `patch_toml` that sets `packs` (or a no-op comment-only patch if packs are sticky).
2. `GetConfig` reflects the patch **or** documented persist rule (workspace overlay vs user global).
3. Write `config.toml` on disk, `ReloadConfig`, `GetConfig` shows disk.

**Pass:** live config without killing the LSP process. Invalid TOML → non-zero `Status`, previous config remains.

### IT-3.3 — WatchSubscribe + WatchBatch

1. `WatchSubscribe`.
2. Create/modify/delete a source file in the corpus **on disk** (not didChange).
3. Receive `WatchBatch` push: path + `kind` (`create` / `modify` / `delete`), `generation` monotonic.

**Pass:** at least one event for that path. Duplicate client `didChangeWatchedFiles` + disk watch must **not** double-apply index (definition remains correct; no crash).

**P-ts extra:** many files changed in one burst (100+). Expect **one coalesced** batch or a small number of batches, not 100+ tiny pushes.

### IT-3.4 — Overflow and FilesSince

1. Drive the watcher into overflow (`need_rescan` or `overflow` true on WatchBatch), **or** subscribe late (missed events).
2. `FilesSince` with `since_generation` = last seen (or `since_unix_ms` = 0).
3. Response: `paths` include dirty files; if `truncated`, a second FilesSince or documented rescan still converges.

**Pass:** catch-up does **not** require a full naive walk as the only path (bounded list). **No** LSP method named FilesSince.

**Negative:** JSON-RPC `$ /progressive/filesSince` still unimplemented.

### IT-3.5 — IndexStatus, TierStatus, TierReady

**P-java**

1. After initialize, `IndexStatus` lists packages (Maven modules).
2. `TierReady` pushes as packages finish (`syntax` then `graph`). Stock client also saw `workDoneProgress`.
3. `TierStatus` rows match last TierReady.
4. LSP `Location.data.tier` on a cross-package definition becomes `graph` after the push (query again; no process restart).

**P-py / P-ts**

1. Without pack: tier stays `syntax` (or `graph` if T2 exists for that language).
2. With pack installed **before** start: `TierReady` to `types` for a package; `typeDefinition` / typed hover succeeds.
3. Core remains up if the engine is absent.

### IT-3.6 — InstallPacks (P-py)

1. Start **without** ty in `engines/`.
2. Python F12 still works at T1 (IT-2 contract).
3. `InstallPacks { packs: ["ty"] }` using a **pre-staged** pack tarball on the test FS (no internet). Hash mismatch fixture: wrong bytes → `Status` error, **binary not replaced**, F12 still T1.
4. Correct hash: pack lands under `$P/engines/`; later `TierReady` `types` **or** documented “restart serve to attach engine” — if restart is required, the API doc must say so; the test follows the doc.

### IT-3.7 — ReloadScripts (P-java)

1. Workspace or prefix `scripts/` with an `on_watch` that drops `**/generated/**`.
2. `ReloadScripts`.
3. Modify a file under `generated/`; it must **not** appear in WatchBatch (or must be filtered before dirty-set).
4. `on_bootstrap` Abort script: initialize fails with a clear message (separate process).

Do not test `on_pre_save` (not this product).

## What not to assert

- Git status, PTY, file-tree CRUD, SSH.
- That Neovim implements protobuf.
- Engine internals (clangd unit tests).

## Pass bar

All IT-3.1–3.5 on P-java, P-py, P-ts. IT-3.6 on P-py. IT-3.7 on P-java. IT-3.mux on P-java if mux exists.

One JUnit/JSON report: `backend`, `rpc`, `result`.
