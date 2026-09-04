# Agent context policy (host stack)

**Pointer-based context.** Every orchestrator, implementer, hygiene agent, and sub-orchestrator on `host0` → `host7` receives the **same** payload. Do not paste this tree or the pattern table into prompts. Do not grow context “because the parent had it.”

Parent of `host0` is `poc-no-stall` (`63507ff`), stacked after the POC-proof stack. Do not reopen LOG-0–LOG-11. Do not merge `RunLog` with the serve WAL. Do not `git stash`. Do not `git config`. Do not `--no-verify`. Do not force-push `main`. This repo uses **git-branchless** (`git sl`). Do not install Graphite.

## Payload to pass (copy this block)

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/host/agent-context.md
  docs/host-deps.md
  docs/poc-ide/README.md
  docs/poc-ide/architecture.md
  docs/poc-ide/third-party.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/architecture.md
  docs/lsp-contract.md
  docs/consumer.md
  docs/language-matrix.md
MILESTONE: <HOST-N>
BRANCH: <hostN>
PARENT_BRANCH: <poc-no-stall | hostN-1>
STEPS: the 8-step orchestrator loop in this file
Pass CONTEXT_POLICY + READ paths + MILESTONE + BRANCH to every child unchanged.
```

Children that only implement one WP also get that WP id from [../implementation-plan.md](../implementation-plan.md). They still get the same READ list (they will skip files they do not need).

## Orchestrator loop (every milestone)

1. **Last branch clean.** Working tree must be clean before stacking the *next* branch. If dirty, stage and commit **only** files that belong to **this** milestone. Never stash. Never discard user work.
2. **Stack.** `git checkout PARENT_BRANCH && git checkout -b BRANCH` only when the parent is signed off and the child does not exist. Confirm with `git sl`. If `BRANCH` already exists and has the implementation, stay on it and finish sign-off. Do not branch from `main`.
3. **Implement** that milestone’s WPs only (spawn children if needed). Scope table below. Do not start the next hostN from this agent.
4. **Pattern hygiene.** Every new type is a row in [../design-patterns.md](../design-patterns.md). No manager/helper/util layers. Invariant tests name the pattern.
5. **Validate patterns.** If a type has no pattern, refactor or delete it. Do not leave Ad-hoc.
6. **Testing hygiene.** [../testing.md](../testing.md): 95% llvm-cov on crates that exist (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`); 80% mutants on listed crates that changed; no `thread::sleep`; crate-scoped `cargo test -- --test-threads=1` plus composition-root if needed — say what you ran. Darwin: do not fake musl greens. `check-static` only when a shipped ELF changed. Tests never talk to a Docker daemon, registry, or AWS. `FakeRuntime` / `FakeEngineAdapter` only.
7. **Docs.** Sign off in [../milestones.md](../milestones.md). Add WP rows in [../implementation-plan.md](../implementation-plan.md). Keep the stack diagram in [../branching.md](../branching.md).
8. **Commit.** Stage only this milestone. Message style: `Land … so hostN can sign off without starting hostN+1.` Then `git sl`. Do not open the next branch. Do not create a PR.

## Slice scope (do not cross)

| Branch | In | Out |
|---|---|---|
| `host0` | Native vs container File menu; `T3HostOffer`; `LaunchJournal` / `StatusModal`; `RuntimePort` + `FakeRuntime`; `DockerRuntime` probe/image only (start still unwired); core `file_uri`; host stack docs | `PackAdapter` `Command`; runtime image; `docker run` attach; mux client |
| `host1` | `PackAdapter` Linux `Command` spawn; stub still refuses; Darwin still refuses; stderr pipe `ChildIo::lsp_with_stderr_pipe` | Image; attach; mux; pack *builds* |
| `host2` | Real musl **core** ELFs `aarch64` + `x86_64` via `xtask musl`; `check-static` | Engine pack binaries; runtime image; attach |
| `host3` | Slim pack jobs (ty, rust-analyzer, phpantom, biome, superhtml) static musl both triples; pinned SHAs; `check-static` fail-closed | clangd/tsgo/gopls/zls; attach |
| `host4` | Runtime image: copy prebuilt core+slim into `/opt/plsp`; `xtask runtime-image`; never cargo/LLVM in the runtime image | Attach wiring; mux; full packs |
| `host5` | `DockerRuntime.start`: `docker run -i --rm`, bind-mount `$WS:$WS`, `--prefix /opt/plsp`; poc-ide `StdioLsp` talks to docker stdin/stdout; no local Darwin serve | Mux; full packs |
| `host6` | poc-ide mux client + `serve --mux` for control on the same stdio. Unix sockets through Docker Desktop forbidden | Full packs |
| `host7` | Full flavor: clangd, tsgo, gopls, zls. PR CI still must not compile LLVM from scratch | — |

## Locks

- One Linux `serve` per workspace. Native **or** container, never both.
- Bind-mount identity: `-v $HOST_WS:$HOST_WS`. No URI rewriter.
- Prefix inside the image is `/opt/plsp`, not the Mac `~/.progressivelsp`.
- First live T3 proof is Python (ty) or PHP (phpantom) on mounted source. A Darwin rustc sysroot will not satisfy rust-analyzer in the container.
- Unix sockets through Docker Desktop are out. Control is `--mux` on stdio (`host6`).

## Sign-off

The orchestrator does not start `hostN+1`. The parent (stack driver) starts the next orchestrator only when the sign-off checklist in milestones is checked.

## git-branchless

Stacked branches are ordinary `git checkout -b` children. The user may say “stackless”; they mean git-branchless.
