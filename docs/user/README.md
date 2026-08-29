# progressive-lsp

**Language intelligence for many languages on Linux** — go-to-definition, references, hover, symbols, and highlighting — as a single Language Server Protocol (LSP) process you point your editor at.

progressive-lsp is a language-intelligence server: it parses your project, builds an index, and answers the same LSP questions every modern editor already knows how to ask. It is **not** an IDE, and it is **not** SSH, git, a file tree, or a terminal. Those stay in your editor (or remote-IDE host). This process only does language intelligence.

It aims to support **C, C++, C#, Rust, JavaScript, TypeScript, CSS, HTML, Python, PHP, Java, Go, and Zig**. Java is syntax plus heuristics only — there is no Java language server and no JVM. PHP navigation works out of the box; full types need an optional PHP pack.

You care because the **server is one static Linux binary**. You do not need Node, a JDK, or Python installed to *run* it. Optional engine packs (rust-analyzer, clangd, and friends) add richer types when you want them; without a pack you still get highlighting and navigation at a simpler level.

If you are writing an editor host that wants live pack install, file-watch catch-up, or indexing UI beyond stock LSP, see the [progressive.v1 API](progressive-v1-api.md). Neovim, VS Code, and other stock clients can ignore that document.

## Install

Copy a release tarball onto the Linux host (or extract it in place). The default home is `$HOME/.progressivelsp`. Override it with `PROGRESSIVE_LSP_HOME` or `--prefix`.

```text
progressive-lsp install --prefix DIR --packs python,rust,...
progressive-lsp serve
```

`install` places the layout and the packs you named under that prefix. `serve` speaks **stdio LSP** — the same transport every editor already uses.

Typical prefix: `$HOME/.progressivelsp`. After install, the binary is usually `$HOME/.progressivelsp/bin/progressive-lsp`. Put that directory on `PATH`, or spawn the full path from your editor.

On a Mac or Windows laptop, run the **Linux static** binary on a remote Linux box and point your editor’s LSP client at that process. Native macOS/Windows hosts are not the v1 target.

## Point an editor at it

Treat it like any other language server. Stock clients need nothing besides `progressive-lsp serve` on stdio. You do not need protobuf, a control socket, or a special plugin.

### Neovim (stdio)

```lua
vim.lsp.start({
  name = "progressive-lsp",
  cmd = { vim.fn.expand("~/.progressivelsp/bin/progressive-lsp"), "serve" },
  root_dir = vim.fs.root(0, { ".git", ".progressivelsp" }),
})
```

Any lspconfig-style `cmd = { ".../progressive-lsp", "serve" }` setup is the same idea. Leave `--control-socket` and `--mux` off unless you are a [progressive client](progressive-v1-api.md).

## Config that just works

**Empty config is valid.** You can run with no files at all.

Optional files:

| File | Who it’s for |
|---|---|
| `$HOME/.progressivelsp/config.toml` | Your defaults on this machine |
| `<workspace>/.progressivelsp/config.toml` | Project overlay (shareable; safe to commit) |

Keys people actually set:

```toml
packs = ["python", "rust"]
scripts = ["deny.rhai"]
```

Unknown keys are ignored. The project overlay wins over the user-home file for keys it sets.

Cache, logs, and sockets live under the home prefix (`$HOME/.progressivelsp/` or `PROGRESSIVE_LSP_HOME`), not in the git tree. If you commit `<workspace>/.progressivelsp/`, you are committing **config and scripts on purpose** — not caches.

## Optional engine packs

Packs are optional extras next to the core binary: rust-analyzer, clangd, ty (Python), tsgo, gopls, zls, and similar engines for C#, CSS, HTML, and PHP. A missing pack is not a failure — you still get highlighting and navigation, just without that language’s full type engine.

Java never uses a JVM language server. PHP full types need the PHP pack; without it, go-to-definition and references still work.

## Troubleshooting

- The binary must be a **Linux static** build. On a Mac, run it on remote Linux and speak LSP to that process.
- The control socket is **optional**. Stock editors only need `serve` on stdio. See [progressive.v1](progressive-v1-api.md) only if you want extra features.
- Java does **not** need a JDK for the server. Syntax and navigation do not start a JVM.
- Do not expect tsserver, JDT, pylsp, or other Node/Java/Python language servers. This process is the server; packs are statically compiled engines, not those stacks.
- Weak go-to-definition usually means that language’s pack is not installed — install it, or keep using the simpler built-in index.
