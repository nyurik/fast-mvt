//! CPU-instruction-count encode benchmarks (gungraun/Valgrind) — one-shot and deterministic, unlike
//! the wall-clock `encoder` bench. The measured work lives in [`common::encode`] and is shared
//! **verbatim** with that harness; only the wrapper differs.
//!
//! Needs Valgrind plus `cargo install gungraun-runner` (not arm64 / Apple Silicon); `cargo bench
//! --no-run` compiles anywhere. Fixture loading happens in `setup`, excluded from the count. Filter
//! with e.g. `just bench-cpu-encode fast`.

use gungraun::{library_benchmark, library_benchmark_group, main};

mod common;

use common::encode::Enc;
use common::{BenchTile, load_repo_mvt_files};

/// Load the fixture tiles for a case — setup-time work, excluded from the instruction count.
fn setup(enc: Enc) -> (Enc, Vec<BenchTile>) {
    (enc, load_repo_mvt_files(false))
}

#[library_benchmark(setup = setup)]
#[bench::fast(Enc::Fast)]
#[bench::fast_owned(Enc::FastOwned)]
#[bench::mvt(Enc::Mvt)]
#[bench::mvt_owned(Enc::MvtOwned)]
#[bench::tinymvt(Enc::Tiny)]
#[bench::tinymvt_owned(Enc::TinyOwned)]
fn encode((enc, tiles): (Enc, Vec<BenchTile>)) {
    common::encode::run(enc, &tiles);
}

library_benchmark_group!(name = encoder_cpu, benchmarks = [encode]);
main!(library_benchmark_groups = encoder_cpu);
