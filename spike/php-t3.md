# Spike: PHPantom vs static phpactor

**Preferred:** PHPantom (Rust) — `rust-engine` allocator cell.

**Alternative:** static phpactor + static php **only if** the result is a fully static ELF. Host `php` on PATH is forbidden as T3.

**Not:** intelephense, Node, any `.so` we ship.

**Fail closed:** if neither candidate is `check-static` clean → PHP T2 ceiling.

**M0 result:** notes only. Decision recorded when M4.4 lands.
