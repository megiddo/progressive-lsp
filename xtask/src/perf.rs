//! Tiny M5 perf bench. FakeClock is unused here (no debounce wait).
//! Wall times are **host samples**. Allocator-matrix winners stay CI-arch only.

use std::path::Path;
use std::time::Instant;

use std::sync::Arc;

use progressive_lsp_core::{rss_sample_label, sample_rss_bytes, FakeClock, FileId};
use progressive_lsp_index::{IndexService, LanguageIndexer};
use progressive_lsp_lang_java::JavaIndexer;
use progressive_lsp_resolve::{
    Position, QueryKind, ResolveQuery, Resolver, TreeSitterResolver,
};
use progressive_lsp_watch::{FakeWatcher, WatchCoalescer, WatchKind};

pub fn run(_args: &[String]) -> Result<(), String> {
    let rss_before = sample_rss_bytes();
    let reparse = measure_open_buffer_reparse_us();
    let def_p99 = measure_definition_p99_us();
    let burst = measure_burst_10k_us();
    let rss_after = sample_rss_bytes();
    println!("host: {}", std::env::consts::OS);
    println!("arch: {}", std::env::consts::ARCH);
    println!("open-buffer reparse: {reparse} us  (Darwin/host sample; CI required ~10 ms class)");
    println!(
        "T1/T2 definition p99 after index: {def_p99} us  (Darwin/host sample; CI required < 50 ms)"
    );
    println!(
        "10k external-edit burst (FakeWatcher+FakeClock, 1 batch): {burst} us  (Darwin/host sample)"
    );
    match (rss_before, rss_after) {
        (Some(a), Some(b)) => println!(
            "core RSS without engines: {b} bytes (before={a})  [{}]",
            rss_sample_label()
        ),
        (None, None) => println!(
            "core RSS without engines: unavailable on this host  [{}]",
            rss_sample_label()
        ),
        _ => println!(
            "core RSS without engines: {:?} -> {:?}  [{}]",
            rss_before,
            rss_after,
            rss_sample_label()
        ),
    }
    println!(
        "T3 engines are not charged to core. Allocator-matrix winners are recorded only from \
         matching CI arch jobs (see docs/testing.md). This Darwin/laptop run is a sample."
    );
    if reparse >= 10_000 {
        return Err(format!("open-buffer reparse {reparse}us exceeds ~10ms class"));
    }
    if def_p99 >= 50_000 {
        return Err(format!("definition p99 {def_p99}us exceeds 50ms"));
    }
    Ok(())
}

fn measure_open_buffer_reparse_us() -> u128 {
    let mut svc = IndexService::new();
    let path = Path::new("Buf.java");
    let src = "class Buf { int x = 1; void m() { x = 2; } }\n";
    svc.open_buffer(path);
    svc.index_text(path, src, &JavaIndexer, false);
    let change = progressive_lsp_index::InputChange {
        start_byte: src.find('1').unwrap(),
        old_end_byte: src.find('1').unwrap() + 1,
        new_end_byte: src.find('1').unwrap() + 1,
        start_row: 0,
        start_column: src.find('1').unwrap(),
        old_end_row: 0,
        old_end_column: src.find('1').unwrap() + 1,
        new_end_row: 0,
        new_end_column: src.find('1').unwrap() + 1,
        new_text: "3".into(),
    };
    svc.apply_change(path, &change, &JavaIndexer as &dyn LanguageIndexer)
}

fn measure_definition_p99_us() -> u128 {
    let mut svc = IndexService::new();
    let path = Path::new("Def.java");
    let src = "class Def { void target() {} void caller() { target(); } }\n";
    svc.index_text(path, src, &JavaIndexer, false);
    let shared = progressive_lsp_index::SharedIndex::new(svc);
    let resolver = TreeSitterResolver::new(std::sync::Arc::new(shared));
    let pos = Position::new(0, src.find("target()").unwrap() as u32);
    let q = ResolveQuery::new(FileId::new("Def.java"), pos, QueryKind::Definition);
    let mut times = Vec::with_capacity(100);
    for _ in 0..100 {
        let t = Instant::now();
        let _ = resolver.resolve(&q);
        times.push(t.elapsed().as_micros());
    }
    times.sort_unstable();
    times[98]
}

fn measure_burst_10k_us() -> u128 {
    let clock = Arc::new(FakeClock::at_unix_ms(1_000));
    let mut c = WatchCoalescer::with_limits(clock.clone(), 50, 20_000, 64);
    let mut fake = FakeWatcher::new();
    let t = Instant::now();
    for i in 0..10_000 {
        fake.inject_one(format!("f{i}.java"), WatchKind::Modify);
    }
    c.poll_backend(&mut fake);
    clock.advance_ms(50);
    let batch = c.flush_due().expect("window");
    assert_eq!(batch.events.len(), 10_000);
    t.elapsed().as_micros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_perf_prints_and_stays_under_gates() {
        run(&[]).unwrap();
        assert!(measure_open_buffer_reparse_us() < 10_000);
        assert!(measure_definition_p99_us() < 50_000);
    }
}
