#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::KeySet;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    let mut set = KeySet::default();
    set.merge_str(&text, Path::new("prod.keys"));
    set.merge_str(&text, Path::new("title.keys"));

    let _ = set.max_revision();
    let _ = set.header_key();
    let _ = set.titlekek(0);
    let _ = set.key_area_key_application(0);
    let _ = set.titlekey_count();
});
