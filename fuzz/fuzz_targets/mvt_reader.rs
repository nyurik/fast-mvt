#![no_main]

use fast_mvt_fuzz::MvtReaderInput;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: MvtReaderInput| {
    input.fuzz_reader();
});
