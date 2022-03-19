#![no_main]
use libfuzzer_sys::fuzz_target;
use testcase::run;

fuzz_target!(|data: &[u8]| {
    run(data);
});
