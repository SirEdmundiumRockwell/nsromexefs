#![no_main]

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::parse_sha256sums;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_sha256sums(&text);
});
