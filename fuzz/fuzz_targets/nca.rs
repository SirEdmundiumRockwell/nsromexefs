#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::NcaHeader;

const HEADER_KEY: [u8; 32] = [0x5A; 32];

fuzz_target!(|data: &[u8]| {
    let len = data.len() as u64;

    // Cabecera en claro y cifrada. La clave fija ejercita la ruta XTS.
    let mut cursor = Cursor::new(data);
    let _ = NcaHeader::read_at(&mut cursor, 0, len, None);

    let mut cursor = Cursor::new(data);
    let _ = NcaHeader::read_at(&mut cursor, 0, len, Some(&HEADER_KEY));

    // Base distinta de cero, como una NCA dentro de un HFS0.
    if data.len() >= 8 {
        let seed = u64::from_le_bytes(data[..8].try_into().unwrap());
        let base = seed % (len + 1);
        let mut cursor = Cursor::new(data);
        let _ = NcaHeader::read_at(&mut cursor, base, len - base, Some(&HEADER_KEY));
    }
});
