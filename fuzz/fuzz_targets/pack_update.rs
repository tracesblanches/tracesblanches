#![no_main]
#![allow(
    clippy::disallowed_methods,
    reason = "File::create is in the expansion of libfuzzer_sys::fuzz_target! (RUST_LIBFUZZER_DEBUG_PATH), not a write of ours"
)]

libfuzzer_sys::fuzz_target!(|data: &[u8]| skimap_fuzz::pack_update(data));
