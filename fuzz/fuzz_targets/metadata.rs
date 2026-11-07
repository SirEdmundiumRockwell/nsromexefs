#![no_main]

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::{BktrHeader, Cnmt, Ivfc, RomFsHeader, Ticket};

fuzz_target!(|data: &[u8]| {
    let _ = Cnmt::parse(data);
    let _ = Ticket::parse(data);
    let _ = RomFsHeader::parse(data);
    let _ = Ivfc::parse(data);
    let _ = BktrHeader::parse(data);
});
