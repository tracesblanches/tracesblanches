#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| skimap_fuzz::pack(data));
