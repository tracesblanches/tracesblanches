#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| skimap_fuzz::pmtiles_vendor(data));
