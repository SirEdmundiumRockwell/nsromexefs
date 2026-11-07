#![no_main]

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::BktrTables;

fuzz_target!(|data: &[u8]| {
    // Formato del corpus: [u32 LE = longitud de la tabla de relocalización]
    // seguido de las dos tablas BKTR. La longitud se acota al resto.
    if data.len() < 4 {
        return;
    }
    let declared = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    let rest = &data[4..];
    let split = declared.min(rest.len());
    let (reloc, subsec) = rest.split_at(split);

    let section_size = rest.len() as u64;
    let _ = BktrTables::parse(reloc, subsec, section_size, 0, section_size);
});
