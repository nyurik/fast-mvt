//! Wall-clock decode benchmarks (criterion), comparing fast-mvt against `mvt-reader` and `tinymvt`.
//!
//! The measured work lives in [`common::decode`] and is shared **verbatim** with the
//! instruction-count harness in `decoder_cpu.rs` — only the harness differs. Run with
//! `just bench-decode`; for deterministic CPU-instruction counts instead, see `just bench-cpu-decode`.

use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use usize_cast::FromUsize;

mod common;

use common::decode::Dec;
use common::load_repo_mvt_files;

fn bench_decode(c: &mut Criterion) {
    let tiles = load_repo_mvt_files(true);
    let bytes = tiles.iter().map(|tile| tile.bytes).sum();

    let mut group = c.benchmark_group("mvt decode");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Bytes(u64::from_usize(bytes)));
    for dec in Dec::ALL {
        group.bench_function(
            format!("{} ({} tiles)", dec.label(), tiles.len()),
            |bench| {
                bench.iter(|| common::decode::run(dec, &tiles));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_decode);
criterion_main!(benches);
