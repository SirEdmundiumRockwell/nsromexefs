#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::{Hfs0, Pfs0, Xci};

fuzz_target!(|data: &[u8]| {
    let len = data.len() as u64;

    let mut cursor = Cursor::new(data);
    let _ = Xci::read(&mut cursor, len);

    let mut cursor = Cursor::new(data);
    let _ = Pfs0::read(&mut cursor, len);

    // HFS0 también aparece anidado, así que su base no tiene por qué ser 0.
    let bases = [0, 1, len / 2, len.saturating_sub(0x10)];
    for base in bases {
        let mut cursor = Cursor::new(data);
        let _ = Hfs0::read_at(&mut cursor, base, len);
    }
});
