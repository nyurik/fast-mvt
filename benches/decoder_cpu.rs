//! CPU-instruction-count decode benchmarks (gungraun/Valgrind) — one-shot and deterministic, unlike
//! the wall-clock `decoder` bench. The measured work lives in [`common::decode`] and is shared
//! **verbatim** with that harness; only the wrapper differs.
//!
//! Needs Valgrind plus `cargo install gungraun-runner` (not arm64 / Apple Silicon); `cargo bench
//! --no-run` compiles anywhere. Fixture loading happens in `setup`, excluded from the count. Filter
//! with e.g. `just bench-cpu-decode fast`.

use gungraun::{library_benchmark, library_benchmark_group, main};

mod common;

use common::decode::Dec;
use common::{BenchTile, load_repo_mvt_files};

/// Load the fixture tiles for a case — setup-time work, excluded from the instruction count.
fn setup(dec: Dec) -> (Dec, Vec<BenchTile>) {
    (dec, load_repo_mvt_files(true))
}

#[library_benchmark(setup = setup)]
#[bench::fast(Dec::Fast)]
#[bench::mvt_reader(Dec::MvtReader)]
#[bench::tinymvt(Dec::Tiny)]
fn decode((dec, tiles): (Dec, Vec<BenchTile>)) {
    common::decode::run(dec, &tiles);
}

library_benchmark_group!(name = decoder_cpu, benchmarks = [decode]);
main!(library_benchmark_groups = decoder_cpu);
