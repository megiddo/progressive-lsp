# progressive.v1 API

Extended protocol for authors of a **progressive** client: an editor host that wants features beyond stock LSP (live config, pack install, watch catch-up, indexing UI).

Stock editors (Neovim, VS Code, tests) only need `progressive-lsp serve` on stdio. They ignore everything in this document. Installation, config files, and “just works” behavior live in the [product README](README.md).

This specification describes the **extended control API** (`progressive.v1`). It is not a compiler or crate guide. Each endpoint below is written as: what it does, when to call it, what it reads or writes, how success and error look, and which other endpoints or LSP methods it relates to.

---

## Discovery

Vanilla LSP `initialize` always returns an experimental capability. Stock clients ignore it. Progressive clients read it and opt in.

```json
{
  "capabilities": {
    "experimental": {
      "progressiveLsp": {
        "version": "v1",
        "socket": "/home/you/.progressivelsp/run/control.sock",
        "mux": false
      }
    }
  }
}
```

| Field | Meaning |
|---|---|
| `version` | Protocol generation. v1 clients expect `"v1"`. |
| `socket` | Absolute path of the control socket, or `null` if control is off or you are expected to use mux / a pre-opened fd. |
| `mux` | `true` when this process was started with `--mux` (LSP and control share one pipe). |

**Default `serve`:** LSP on stdio, control **off** (`socket` is `null`, `mux` is `false`). Turning control on does **not** replace LSP. Intelligence (definition, references, hover, tokens) stays on the LSP channel. Control is a second channel beside it.

If you spawn the process yourself:

```text
progressive-lsp serve [--prefix DIR] [--control-socket PATH] [--control-fd N] [--mux]
```

- `--control-socket PATH` — open a Unix socket at `PATH` and speak protobuf there.
- `--control-fd N` — speak protobuf on an inherited file descriptor (same messages as the socket).
- `--mux` — one stdio pipe, two channels (see [Wire](#wire)). `progressiveLsp.mux` will be `true`.

Connect to `socket` after `initialize` succeeds, or spawn already wired with `--control-socket` / `--mux`. Do not wait for a custom LSP method — there is none.

---

## Wire

Canonical encoding is **protobuf**. Package name: `progressive.v1`. There is **no** JSON-RPC `$/` mirror in v1. `FilesSince` is not an LSP method. Do not send `workspace/filesSince` or `$/progressive/filesSince`.

### Frames

Every control payload is length-prefixed:

```text
4-byte big-endian length  |  protobuf bytes
```

Maximum payload: **16 MiB**. Larger frames fail. The server does not silently truncate.

### Messages (`progressive.v1`)

Unary request/response pairs:

`Status`, `GetConfigRequest`, `GetConfigResponse`, `SetConfigRequest`, `SetConfigResponse`, `ReloadConfigRequest`, `ReloadConfigResponse`, `InstallPacksRequest`, `InstallPacksResponse`, `WatchSubscribeRequest`, `WatchSubscribeResponse`, `FilesSinceRequest`, `FilesSinceResponse`, `IndexStatusRequest`, `IndexStatusResponse`, `IndexPackage`, `TierStatusRequest`, `TierStatusResponse`, `TierRow`, `ReloadScriptsRequest`, `ReloadScriptsResponse`.

Server pushes (no request):

`WatchEvent`, `WatchBatch`, `TierReady`.

`Status` is:

```text
code     int32    // 0 = ok; non-zero = error
message  string   // human-readable; empty on success is fine
```

Treat `Status.code == 0` as success. On error, do not apply the write; show `message`.

### `--mux`: one pipe, two channels

`--mux` does not invent a new protocol. It puts **two channels on one stdio stream**:

| Channel | What travels |
|---|---|
| `lsp` | Vanilla LSP JSON-RPC (Content-Length), unchanged |
| `control` | The same length-prefixed protobuf frames as the dedicated socket |

LSP still answers definition, hover, and tokens. Control still does the RPCs in this document. Enabling mux does not turn LSP off and does not encode LSP as protobuf.

Prefer `--mux` when you own the process and want a single pipe. Prefer `--control-socket` (or `--control-fd`) when you want a second path next to stdio LSP. Stock clients never set either flag.

### Dispatch envelope (public contract)

The `.proto` schema defines the messages above. It does **not** yet define a `service` or a `oneof` Request wrapper. Clients still need a way to multiplex RPC types on one socket. **This envelope is the public dispatch contract.** Use it on every control frame (the bytes after the 4-byte length prefix). Method names are case-sensitive and match the [RPC table](#rpc-table) exactly.

```protobuf
// Public API — wrap every control frame in this envelope.
message Envelope {
  string method = 1;      // RPC or push name: "GetConfig", "WatchBatch", …
  uint64 request_id = 2;  // client-chosen; echoed on the matching reply. 0 on server pushes.
  bytes  body = 3;        // protobuf of the message named by method
}
```

| Direction | `method` | `body` |
|---|---|---|
| Client → server | RPC name (`GetConfig`, `SetConfig`, …) | that RPC’s `*Request` message |
| Server → client (reply) | same RPC name | that RPC’s `*Response` message |
| Server → client (push) | `WatchBatch` or `TierReady` | that push message; `request_id` is `0` |

Example: `method = "FilesSince"` and `body` is a `FilesSinceRequest` (generation or unix-ms). The reply uses the same `request_id` and a `FilesSinceResponse` body.

---

## RPC table

Unless noted, every response carries `Status`. `code == 0` means the write (if any) happened. Non-zero means it did not.

### GetConfig

**Purpose.** Snapshot of the **merged** `config.toml` chain.

**When to call.** After connect, before showing settings UI, or after `SetConfig` / `ReloadConfig` to confirm what is live.

**Reads / writes.** Reads only. Merge order: user home (`$PREFIX/config.toml`) → optional per-workspace copy under the home prefix → project overlay (`<workspace>/.progressivelsp/config.toml`). Later files win for keys they set. The project overlay therefore wins over user home. Session `initialize` options (`packs`, `scripts`, `prefix`) may override the documented subset for this process.

**Success / error.** `GetConfigResponse.toml` is the merged snapshot. `code != 0` means the snapshot could not be produced; `toml` may be empty.

**Relates to.** `SetConfig`, `ReloadConfig`. Same files as in the [README](README.md). Not an LSP method.

### SetConfig

**Purpose.** Patch live config keys and persist them on the merge chain.

**When to call.** When the user (or host) changes packs, scripts, or other known keys without editing files by hand.

**Reads / writes.** `SetConfigRequest.patch_toml` is a partial TOML patch, not a full-file replace. Only keys present in the patch are written. Unknown keys are ignored (same rule as on-disk config). Persistence follows merge-chain rules: a project-scoped change lands on the workspace overlay; a user-scoped change lands on the home file.

**Success / error.** `code == 0` — patch applied and persisted. `code != 0` — disk and live config unchanged; read `message`.

**Relates to.** `GetConfig` (read back), `ReloadConfig` (disk wins if someone else edited files). Does not restart LSP.

### ReloadConfig

**Purpose.** Re-read `config.toml` from disk and apply the merge chain.

**When to call.** After an external edit (git pull, another tool, a human in `$EDITOR`), or when you want disk to win over an in-memory `SetConfig` you no longer trust.

**Reads / writes.** Reads the same chain as `GetConfig`. Writes the live session to match disk. Does not invent keys.

**Success / error.** `code == 0` — live config now matches disk. `code != 0` — live config unchanged.

**Relates to.** `GetConfig`, `SetConfig`, `ReloadScripts` (scripts listed in config are not automatically re-executed; call `ReloadScripts` when you also need hooks reloaded).

### InstallPacks

**Purpose.** Same as `progressive-lsp install --prefix DIR --packs …`: place requested engine packs under the prefix.

**When to call.** When the user enables a language engine without restarting the LSP process. Typical packs: `python`, `rust`, and the other names you pass to the CLI.

**Reads / writes.** `InstallPacksRequest.packs` is the list of pack ids. Writes verified artifacts under `$PREFIX/engines/` (and related prefix layout). **Hash-gated:** a digest mismatch is an error. The server does **not** replace or execute a mismatched binary.

**Success / error.** `code == 0` — packs are in place. Non-zero (including hash failure) — no exec of the bad blob; previous binaries stay as they were.

**Relates to.** CLI `install` in the [README](README.md). After a successful install, **restart `serve` to attach the engine** (v1 does not hot-spawn a newly written pack in the same process). Then wait for `TierReady` when that language’s engine comes up. `TierStatus` / `IndexStatus` show the new ceiling. Does not replace `textDocument/definition` — it only makes a richer engine available. Darwin stub packs never attach a types engine; Linux CI with real musl packs is the T3 gate.

### WatchSubscribe

**Purpose.** Start coalesced file-watch **pushes** to this control client.

**When to call.** Once, right after you connect, if you want `WatchBatch` instead of (or in preference to) relying only on server-side LSP notify.

**Reads / writes.** Subscribes this connection. Does not write the project tree. Does not talk to git.

**Success / error.** `code == 0` — you will receive `WatchBatch` pushes. `code != 0` — no subscription; you will not get batches.

**Relates to.** `WatchBatch` (push), `FilesSince` (catch-up). Interacts with the file watch / dirty index that drives reindex — **not** with git. Open buffers still go through LSP `textDocument/didChange`. Stock `workspace/didChangeWatchedFiles` remains optional; the server coalesces. If your host already watches the tree, prefer these control messages and do not force a second OS watch.

### WatchBatch (push)

**Purpose.** Coalesced create / modify / delete notifications.

**When you receive it.** After a successful `WatchSubscribe`, whenever the dirty index has a batch for you.

**Reads / writes.** Push only. Each `WatchEvent` has `path` and `kind` (`create`, `modify`, or `delete`). The batch also carries:

| Field | Meaning |
|---|---|
| `overflow` | The watcher dropped events. You are no longer complete. |
| `need_rescan` | Walk or `FilesSince` from a known point; do not assume the event list is the whole truth. |
| `generation` | Monotonic watch/index generation. Pass it to `FilesSince`. |

**Success / error.** Pushes have no `Status`. Treat `overflow == true` or `need_rescan == true` as “I must catch up.”

**Relates to.** `WatchSubscribe`, `FilesSince`. Not git status. Not LSP `didChangeWatchedFiles` (you may still send that; the server coalesces). On overflow, call `FilesSince` — do not invent an LSP method.

### FilesSince

**Purpose.** Catch-up: “what paths changed since generation *N*, or since this unix time in milliseconds?”

**When to call.** After `WatchBatch.overflow` or `need_rescan`; after reconnect; when you have a last-known `generation` (or a timestamp) and need the gap filled.

**Reads / writes.** Reads the watch journal / dirty index. `FilesSinceRequest` is a `oneof`:

- `since_generation` — catch up after generation *N*
- `since_unix_ms` — catch up after that wall-clock instant

**Success / error.** `code == 0` — `paths` is the catch-up set; `generation` is the new watermark. `truncated == true` means the set is incomplete: call again with the returned `generation`, or rescan. `code != 0` — do not advance your watermark.

**Relates to.** `WatchSubscribe` / `WatchBatch`. **Not in LSP.** There is no `$/` or `workspace/` equivalent in v1.

### IndexStatus

**Purpose.** Snapshot of what is indexed: packages, their generations, and a cache-entry count.

**When to call.** To paint an indexing UI, or to poll after connect before the first `TierReady`.

**Reads / writes.** Reads only. `IndexPackage` is `package_id` + `generation`. `cache_entries` is a cache-size hint, not a file list.

**Success / error.** `code == 0` — rows are current. `code != 0` — ignore `packages`.

**Relates to.** `TierStatus`, `TierReady`. Stock clients see ingest as LSP `workDoneProgress` / `window/workDoneProgress/create` instead. Locations on stock LSP may include `Location.data.tier` (`syntax` \| `graph` \| `types`).

### TierStatus

**Purpose.** Per-package current intelligence **tier**.

**When to call.** With `IndexStatus` when you need “how good is this package right now?” without waiting for a push.

**Reads / writes.** Reads only. Each `TierRow` is `package_id` + `tier`.

`tier` is one of:

| Value | Meaning |
|---|---|
| `syntax` | Tree-sitter / highlighting and basic navigation |
| `graph` | Heuristics / file-level graph (still no full engine) |
| `types` | Full engine pack is answering (rust-analyzer, clangd, ty, …) |

These strings match `Location.data.tier` on stock LSP results.

**Success / error.** `code == 0` — `rows` are current. `code != 0` — ignore `rows`.

**Relates to.** `TierReady` (push when a package upgrades), `IndexStatus`, LSP `workDoneProgress`, `Location.data.tier`. Java stays at syntax/graph — there is no types engine. PHP reaches `types` only if the PHP pack is installed.

### TierReady (push)

**Purpose.** A package just upgraded its tier (for example syntax → types after an engine comes up).

**When you receive it.** After ingest or after `InstallPacks` brings an engine online. You do not subscribe separately; it is part of the control session.

**Reads / writes.** Push only: `package_id` + `tier` (`syntax` \| `graph` \| `types`).

**Success / error.** No `Status`. Refresh tokens / UI for that package. Stock clients get the same moment as `workDoneProgress` completion.

**Relates to.** `TierStatus`, `IndexStatus`, `InstallPacks`, LSP `workDoneProgress`, `Location.data.tier`.

### ReloadScripts

**Purpose.** Reload host scripts from the **same config merge chain** as `GetConfig`.

**When to call.** After you change `scripts` via `SetConfig`, after `ReloadConfig` if script files on disk changed, or when the user asks to re-apply hooks without restarting LSP.

**Reads / writes.** Re-reads script files named in merged config (user home and project overlay). Applies the live hook set. Does not implement go-to-definition; scripts may filter watches or skip work, not replace the resolver.

Hook names (abort meaning):

| Hook | Abort means |
|---|---|
| `on_bootstrap` | Fail `initialize` with a clear message |
| `on_workspace_discover` | Skip those source roots (cannot invent jars that are not on disk) |
| `on_pre_index` | Skip that package |
| `on_post_index` | Cannot unwind intelligence (logging only) |
| `on_watch` | Drop those paths from the coalesced batch |
| `on_engine_spawn` | Skip that engine (`graph` / syntax remain) |
| `on_tier_ready` | Cannot abort intelligence (logging only) |
| `on_install_verify` | Refuse the new binary |

**Success / error.** `code == 0` — new scripts are live. `code != 0` — previous scripts remain.

**Relates to.** `GetConfig` / `SetConfig` / `ReloadConfig` (`scripts` key). `WatchSubscribe` ( `on_watch` ). `InstallPacks` ( `on_install_verify` ). Not LSP. Not editor keybindings.

---

## Client recipes

### 1. Watch, then catch up

1. Complete LSP `initialize` and read `capabilities.experimental.progressiveLsp`.
2. Connect to `socket` (or already be on `--mux` / `--control-fd`).
3. Send `WatchSubscribe`.
4. Handle `WatchBatch` pushes; remember `generation`.
5. If `overflow` or `need_rescan`, call `FilesSince` with that generation (or `since_unix_ms`). If `truncated`, call again or rescan.

Open buffers still use LSP `didChange`. This recipe is for disk edits and reconnect, not for git.

### 2. Indexing UI

1. Call `IndexStatus` and `TierStatus` once after connect.
2. Show progress from those snapshots plus `TierReady` pushes as packages move `syntax` → `graph` → `types`.
3. Optionally also listen to stock LSP `workDoneProgress` so a host that only has the LSP pipe can show the same ingest.

`Location.data.tier` on definition/reference results is the per-hit version of the same three strings.

### 3. Live pack install (no LSP restart)

1. `InstallPacks` with the pack ids (same names as `progressive-lsp install --packs`).
2. On hash failure, keep the old binary; do not restart.
3. On success, keep the LSP session up, then **restart `serve` to attach the engine**. After restart, expect `TierReady` when that language’s engine comes up (`tier` becomes `types` for packages that can use it).
4. Refresh tokens / navigation for those packages. `GetConfig` will show the new `packs` if you also persisted them via `SetConfig`.

---

## Not in this API

These are editor-host or product problems, not `progressive.v1`:

- Git (status, diff, blame, commit)
- PTY / terminal
- File-tree create / rename / delete as a filesystem UI
- SSH / how bytes reach the Linux host
- Editor settings, keybindings, themes
- Implementing go-to-definition (that is the LSP resolver, not a control RPC)

v1 also does **not** expose FilesSince, WatchBatch, or InstallPacks as LSP `$/` methods. Use this protobuf API.

For install, `serve`, and stock-editor setup, go back to the [README](README.md).
