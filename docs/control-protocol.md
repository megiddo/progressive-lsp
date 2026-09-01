# Control protocol (`progressive.v1`)

Optional API. Stock LSP clients never need it. Canonical encoding in v1: **protobuf only**. There is **no** generated `$/` JSON-RPC mirror in v1.

Wire schema lives in [`proto/progressive/v1/control.proto`](../proto/progressive/v1/control.proto). This document is the semantic standard; the `.proto` file is the machine source. Rust types + the `u32be` codec live in `progressive-lsp-control`. Max payload: **16 MiB** (`MAX_PAYLOAD_BYTES`); larger frames fail (no silent truncate).

Related: [lsp-contract.md](lsp-contract.md), [architecture.md](architecture.md), [consumer.md](consumer.md).

## Frames

Length-prefixed:

```text
u32be payload_length | protobuf bytes
```

Max payload: implement a documented cap (suggest 16 MiB) and fail the request if exceeded. Do not silently truncate FilesSince without setting `truncated`. Bind/accept/`PayloadTooLarge`/unknown method emit `LogPort` (`operation=control`) without logging Envelope **bodies** (LOG-7).

`--mux`: outer mux frame is `u8 channel | u32be length | payload` (16 MiB cap). Channel `0` = LSP JSON-RPC. Channel `1` = control; inner payload is the same length-prefixed proto (`u32be | protobuf`).

## Discovery

LSP `initialize` result:

```json
{
  "capabilities": {
    "experimental": {
      "progressiveLsp": {
        "version": "v1",
        "socket": "<absolute path or null>",
        "mux": false
      }
    }
  }
}
```

Unknown experimental capabilities are ignored by stock clients.

## How the process exposes it

| Mode | Behavior |
|---|---|
| `progressive-lsp serve` (default) | LSP on stdio. Server-side `notify` on. Control **off**. |
| `--control-socket PATH` / `--control-fd N` | Protobuf beside LSP. `--control-fd` is parsed but unused (`pending`); LOG-7 emits one `warn` so the drop is not silent. |
| `--mux` | One stdio stream: `lsp` + `control` channels |

Library: `progressive-lsp-control` (consumers may depend).

## RPCs and pushes (v1)

Names are proto service methods. Unary unless marked push.

| Method | Direction | Purpose |
|---|---|---|
| `GetConfig` | req/resp | Snapshot of merged `config.toml` |
| `SetConfig` | req/resp | Patch keys; persist per merge-chain rules |
| `ReloadConfig` | req/resp | Re-read disk; apply |
| `InstallPacks` | req/resp | Same as install crate; hash-gated; no exec on mismatch |
| `WatchSubscribe` | req/resp | Start coalesced watch pushes to this client |
| `WatchBatch` | **push** | create/modify/delete + `overflow` / `need_rescan` |
| `FilesSince` | req/resp | Catch-up since generation N or unix ms; `truncated` flag |
| `IndexStatus` | req/resp | Packages, generations, cache stats |
| `TierStatus` | req/resp | Per-package current `Tier` |
| `TierReady` | **push** | Package upgraded tier |
| `ReloadScripts` | req/resp | Reload Rhai from merge chain |

**FilesSince** is not in LSP. Do not add `workspace/filesSince` or `$/progressive/filesSince` in v1. M1 wires `FilesSince` to the watch journal (`truncated` after overflow or a generation gap). `WatchBatch` is the coalesced push DTO.

## Not in this protocol

Git, PTY, file-tree CRUD, buffer CRDTs, SSH, editor settings, “run this shell”. Those are IDE/consumer problems.

## Errors

gRPC-style is not required. Use a proto `Status { code, message }` on responses. Hash mismatch on install → `InstallError::Hash` equivalent, no binary replace.

## Compatibility

Package name `progressive.v1`. Breaking changes → `progressive.v2` and a new experimental version string. Additive fields: proto3 optional / reserved numbers as usual.
