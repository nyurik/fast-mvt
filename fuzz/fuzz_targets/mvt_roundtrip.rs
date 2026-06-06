#![no_main]

use fast_mvt_fuzz::MvtRoundtripInput;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: MvtRoundtripInput| {
    input.fuzz_roundtrip();
});
