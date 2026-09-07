# Agent context policy (POC tier stack)

**Pointer-based context.** Every orchestrator and implementer on `poc-uri` → `t3-rest` gets the **same** payload. Do not paste this tree into prompts.

Parent of `poc-uri` is **current `main`** (`host-cleanup` merged; PR #6 / #7). Do not stack on the old `host-cleanup` ref. Do not open `host8`. Do not reopen HOST-0–HOST-7 WPs. Do not `git stash`. Do not `git config`. Do not `--no-verify`. Do not force-push `main`. git-branchless (`git sl`). No Graphite.

## Payload to pass (copy this block)

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/poc-tier/agent-context.md
  docs/poc-tier-plan.md
  docs/t2-heuristic-coverage.md
  docs/t3-linux-hosts.md
  docs/host-deps.md
  docs/poc-ide/README.md
  docs/poc-ide/architecture.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/language-matrix.md
  spike/java-t3.md
MILESTONE: <POC-URI | POC-T2 | POC-JAVA | POC-IMG | POC-REST>
BRANCH: <poc-uri | t2-coverage | java-t3 | t3-image | t3-rest>
PARENT_BRANCH: <current main | poc-uri | t2-coverage | java-t3 | t3-image>
STEPS: the 8-step orchestrator loop in this file
Pass CONTEXT_POLICY + READ paths + MILESTONE + BRANCH to every child unchanged.
```

Children that implement one WP also get that WP id from [../implementation-plan.md](../implementation-plan.md).

## Orchestrator loop

1. Last branch clean. Never stash. Never discard user work.
2. Stack from signed-off parent only. Confirm `git sl`. `poc-uri` stacks on current `main` (`host-cleanup` already merged). Later slices stack on the signed-off parent branch, not a fresh `main` cut.
3. Implement that milestone’s WPs only.
4. Every new type is a row in [../design-patterns.md](../design-patterns.md).
5. Validate patterns.
6. Testing: [../testing.md](../testing.md). No `thread::sleep`. No Docker daemon in unit tests. `check-static` only on fully static dests; aarch64 `javacs` is the libc exception.
7. Docs: sign off in [../milestones.md](../milestones.md); WPs in [../implementation-plan.md](../implementation-plan.md); keep [../poc-tier-plan.md](../poc-tier-plan.md) checklists in sync.
8. Commit only if the user asked. Message style: `Land … so <milestone> can sign off without starting the next branch.`

## Slice scope (do not cross)

| Branch | In | Out |
|---|---|---|
| `poc-uri` | Identity `file:` URIs; DockerRunPlan mount tests | T2 heuristics; Java pack live extract |
| `t2-coverage` | Heuristic T2 for C/C++/Rust/Python/CSS/HTML | T3 packs; image |
| `java-t3` | Live `javacs` both Linux ISAs | clangd; tsgo; image glibc base (next) |
| `t3-image` | Copy Java into runtime image; aarch64 libc userspace | full packs tsgo/gopls/zls/clangd |
| `t3-rest` | Dogfood image remaining T3 except C# | C# T3; host8; URI rewriter |

## Locks

- One Linux serve. Native or container, never both.
- Bind-mount identity. No URI rewriter.
- Java x86_64 fully static; aarch64 native-image may need libc. No JVM/JDT.
- C# T1/T2 ceiling.
