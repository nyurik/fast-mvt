//! Wall-clock encode benchmarks (criterion), comparing fast-mvt against `mvt` and `tinymvt`.
//!
//! The measured work lives in [`common::encode`] and is shared **verbatim** with the
//! instruction-count harness in `encoder_cpu.rs` — only the harness differs. Run with
//! `just bench-encode`; for deterministic CPU-instruction counts instead, see `just bench-cpu-encode`.

use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use usize_cast::FromUsize;

mod common;

use common::encode::Enc;
use common::load_repo_mvt_files;

fn bench_encode(c: &mut Criterion) {
    let tiles = load_repo_mvt_files(false);
    let bytes = tiles.iter().map(|tile| tile.bytes).sum();

    let mut group = c.benchmark_group("mvt encode");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Bytes(u64::from_usize(bytes)));
    for enc in Enc::ALL {
        group.bench_function(
            format!("{} ({} tiles)", enc.label(), tiles.len()),
            |bench| {
                bench.iter(|| common::encode::run(enc, &tiles));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_encode);
criterion_main!(benches);
